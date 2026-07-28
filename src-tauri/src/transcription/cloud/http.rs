// Helpers HTTP partages par les clients batch.
//
// map_http_err()  : mapping anyhow qui discrimine timeout / connect error
// (pattern repris de enhancement/providers/openai_compat.rs).

use std::path::Path;
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use url::Url;

/// Timeout total pour une requete batch (upload + traitement cote provider).
/// 120s est large : Whisper large sur 30min d'audio prend ~30-60s cote cloud.
pub const BATCH_TIMEOUT: Duration = Duration::from_secs(120);

/// Timeout pour l'etablissement de la connexion TCP/TLS.
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);

/// Request body independent of the HTTP implementation selected by proxy
/// policy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpRequest {
    pub method: String,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl HttpRequest {
    pub fn new(method: impl Into<String>, url: impl Into<String>) -> Self {
        Self {
            method: method.into(),
            url: url.into(),
            headers: Vec::new(),
            body: Vec::new(),
        }
    }

    pub fn header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.push((name.into(), value.into()));
        self
    }

    pub fn body(mut self, body: impl Into<Vec<u8>>) -> Self {
        self.body = body.into();
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpResponse {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

/// Validate headers before transports serialize them into a request block.
///
/// Keep diagnostics value-free: header values commonly contain credentials.
pub fn validate_header(name: &str, value: &str) -> Result<()> {
    if name.is_empty() {
        return Err(anyhow!("http header name must not be empty"));
    }
    if name
        .chars()
        .chain(value.chars())
        .any(|character| matches!(character, '\r' | '\n' | '\0'))
    {
        return Err(anyhow!("http header contains unsafe characters"));
    }
    Ok(())
}

/// HTTP client whose public surface does not expose reqwest or WinHTTP types.
pub struct BatchHttpClient;

impl BatchHttpClient {
    pub fn new(endpoint: &str) -> Result<Self> {
        Url::parse(endpoint).map_err(|e| anyhow!("http endpoint: {e}"))?;
        Ok(Self)
    }

    pub async fn send(&self, request: HttpRequest) -> Result<HttpResponse> {
        self.send_with_timeout(request, BATCH_TIMEOUT).await
    }

    pub async fn send_with_timeout(
        &self,
        request: HttpRequest,
        timeout: Duration,
    ) -> Result<HttpResponse> {
        for (name, value) in &request.headers {
            validate_header(name, value)?;
        }
        let url = Url::parse(&request.url).map_err(|e| {
            let detail = crate::services::download::sanitize_message(&e.to_string());
            anyhow!("http endpoint: {detail}")
        })?;
        let route = crate::services::proxy::route_for_url(&url)?;
        match route {
            crate::services::proxy::ProxyRoute::System => {
                #[cfg(windows)]
                {
                    // Do not wrap this in tokio::time::timeout: cancellation only
                    // drops JoinHandle, while WinHTTP keeps running in the blocking
                    // thread. WinHTTP phase timeouts bound each operation, not whole
                    // request lifetime, so a global deadline would leak detached work.
                    Ok(tokio::task::spawn_blocking(move || {
                        let response = crate::services::winhttp::request(
                            &request.method,
                            &request.url,
                            &request.headers,
                            &request.body,
                            timeout,
                        )?;
                        Ok(HttpResponse {
                            status: response.status,
                            headers: Vec::new(),
                            body: response.body,
                        })
                    })
                    .await
                    .map_err(|e| anyhow!("WinHTTP task: {e}"))
                    .and_then(|result| result.map_err(map_winhttp_err))?)
                }
                #[cfg(not(windows))]
                Err(anyhow!(
                    "Windows system proxy is unsupported on this platform"
                ))
            }
            crate::services::proxy::ProxyRoute::Direct
            | crate::services::proxy::ProxyRoute::Explicit { .. } => {
                let client =
                    crate::services::proxy::apply_for_url(reqwest::Client::builder(), &url)?
                        .timeout(timeout)
                        .connect_timeout(CONNECT_TIMEOUT)
                        .build()?;
                let mut builder = client
                    .request(
                        request
                            .method
                            .parse()
                            .map_err(|e| anyhow!("http method: {e}"))?,
                        url,
                    )
                    .body(request.body);
                for (name, value) in request.headers {
                    builder = builder.header(name, value);
                }
                let response = builder.send().await.map_err(map_http_err)?;
                let status = response.status().as_u16();
                let headers = response
                    .headers()
                    .iter()
                    .map(|(n, v)| (n.to_string(), v.to_str().unwrap_or_default().to_string()))
                    .collect();
                let body = response.bytes().await.map_err(map_http_err)?.to_vec();
                Ok(HttpResponse {
                    status,
                    headers,
                    body,
                })
            }
        }
    }
}

/// Return status errors without leaking credentials, tokens, or authorization
/// headers into logs and user-facing errors.
pub fn http_status_error(status: u16, body: &[u8], request: &HttpRequest) -> anyhow::Error {
    const MAX_BODY: usize = 4096;
    let mut diagnostic = format!("HTTP {status}: {}", request.url);
    for (name, _) in &request.headers {
        // Never include raw outbound header values in a diagnostic. This
        // covers headers whose names are not credential-shaped as well.
        diagnostic.push_str(&format!(" {name}: [REDACTED]"));
    }
    diagnostic.push_str(" response=");
    let response_body = String::from_utf8_lossy(&body[..body.len().min(MAX_BODY)])
        // Normalize quoted JSON separators so shared sanitizer can consume
        // values ending at JSON string delimiters, including repeated keys.
        .replace("\":\"", "=")
        .replace("\": \"", "=")
        .replace("\",\"", ",")
        .replace("\", \"", ",");
    diagnostic.push_str(&response_body);

    // Normalize provider-specific credential spellings to keys understood by
    // the shared sanitizer, including JSON and query-style diagnostics.
    let normalized = diagnostic
        .replace("x-api-key", "api_key")
        .replace("X-Api-Key", "api_key")
        .replace("apiKey", "api_key")
        .replace("accessToken", "access_token")
        .replace("\"api_key\"", "api_key")
        .replace("\"access_token\"", "access_token");

    // Run every diagnostic, including request URL, userinfo, query, and exact
    // outbound header values, through one sanitizer before exposing it.
    let sanitized = crate::services::download::sanitize_message(&normalized);
    anyhow!("{sanitized}")
}

/// Deterministic multipart encoder. Parts are emitted in insertion order.
#[derive(Debug, Clone)]
pub struct MultipartEncoder {
    boundary: String,
    parts: Vec<MultipartPart>,
}

#[derive(Debug, Clone)]
struct MultipartPart {
    name: String,
    filename: Option<String>,
    content_type: Option<String>,
    body: Vec<u8>,
}

impl MultipartEncoder {
    /// Legacy deterministic constructor. Prefer `random` for provider traffic.
    pub fn new(boundary: impl Into<String>) -> Self {
        Self {
            boundary: boundary.into(),
            parts: Vec::new(),
        }
    }

    /// Constructs an encoder with a unique boundary suitable for provider traffic.
    pub fn random() -> Self {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let nonce = NEXT.fetch_add(1, Ordering::Relaxed);
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |duration| duration.as_nanos());
        Self::new(format!("parla-{timestamp:x}-{nonce:x}"))
    }
    pub fn field(mut self, name: impl Into<String>, value: impl Into<Vec<u8>>) -> Self {
        self.parts.push(MultipartPart {
            name: name.into(),
            filename: None,
            content_type: None,
            body: value.into(),
        });
        self
    }
    pub fn file(
        mut self,
        name: impl Into<String>,
        filename: impl Into<String>,
        content_type: impl Into<String>,
        body: impl Into<Vec<u8>>,
    ) -> Self {
        self.parts.push(MultipartPart {
            name: name.into(),
            filename: Some(filename.into()),
            content_type: Some(content_type.into()),
            body: body.into(),
        });
        self
    }
    pub fn content_type(&self) -> String {
        format!("multipart/form-data; boundary={}", self.boundary)
    }
    #[cfg(test)]
    pub fn encode(&self) -> Vec<u8> {
        self.try_encode()
            .expect("multipart boundary occurs in encoded part data")
    }

    /// Encodes multipart data, rejecting boundary collisions.
    pub fn try_encode(&self) -> Result<Vec<u8>> {
        for part in &self.parts {
            validate_multipart_parameter("name", &part.name)?;
            if let Some(filename) = &part.filename {
                validate_multipart_parameter("filename", filename)?;
            }
        }
        let marker = self.boundary.as_bytes();
        if self.parts.iter().any(|part| {
            part.body
                .windows(marker.len())
                .any(|window| window == marker)
        }) {
            return Err(anyhow!("multipart boundary occurs in part body"));
        }
        let mut out = Vec::new();
        for part in &self.parts {
            out.extend_from_slice(
                format!(
                    "--{}\r\nContent-Disposition: form-data; name=\"{}\"",
                    self.boundary, part.name
                )
                .as_bytes(),
            );
            if let Some(filename) = &part.filename {
                out.extend_from_slice(format!("; filename=\"{}\"", filename).as_bytes());
            }
            out.extend_from_slice(b"\r\n");
            if let Some(content_type) = &part.content_type {
                out.extend_from_slice(format!("Content-Type: {content_type}\r\n").as_bytes());
            }
            out.extend_from_slice(b"\r\n");
            out.extend_from_slice(&part.body);
            out.extend_from_slice(b"\r\n");
        }
        out.extend_from_slice(format!("--{}--\r\n", self.boundary).as_bytes());
        Ok(out)
    }
}

fn validate_multipart_parameter(kind: &str, value: &str) -> Result<()> {
    if value
        .chars()
        .any(|character| matches!(character, '\r' | '\n' | '"' | '\\'))
    {
        return Err(anyhow!(
            "multipart {kind} contains unsafe header characters"
        ));
    }
    Ok(())
}

/// Mappe une erreur reqwest en anyhow avec un prefixe discriminant.
/// Le pipeline peut ensuite detecter "timeout" / "network_error" via le
/// texte de l'erreur pour decider retry/backoff.
pub fn map_http_err(e: reqwest::Error) -> anyhow::Error {
    let detail = crate::services::download::sanitize_message(&e.to_string());
    if e.is_timeout() {
        return anyhow!("timeout: {detail}");
    }
    if e.is_connect() {
        return anyhow!("network_error: {detail}");
    }
    anyhow!("http: {detail}")
}

/// Map errors returned by the blocking WinHTTP task to the same retry classes
/// used by the async reqwest transport. Join failures stay separate so task
/// lifecycle errors are not misreported as network failures.
#[cfg(any(test, windows))]
fn map_winhttp_err(error: anyhow::Error) -> anyhow::Error {
    let detail = crate::services::download::sanitize_message(&error.to_string());
    let lower = detail.to_ascii_lowercase();
    if lower.contains("timeout")
        || lower.contains("timed out")
        || lower.contains("12002")
        || lower.contains("258")
    {
        anyhow!("timeout: {detail}")
    } else {
        anyhow!("network_error: {detail}")
    }
}

/// Lit le WAV du disque et extrait le nom de fichier avec fallback.
pub async fn read_wav_with_filename(wav_path: &Path) -> Result<(Vec<u8>, String)> {
    let bytes = tokio::fs::read(wav_path)
        .await
        .with_context(|| format!("lecture {}", wav_path.display()))?;
    let name = wav_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("audio.wav")
        .to_string();
    Ok((bytes, name))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::sync::mpsc::{self, Receiver};
    use std::thread::{self, JoinHandle};

    fn local_server(responses: Vec<&'static str>) -> (String, Receiver<Vec<u8>>, JoinHandle<()>) {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let address = listener.local_addr().unwrap();
        let (sender, receiver) = mpsc::channel();
        let thread = thread::spawn(move || {
            for response in responses {
                let (mut stream, _) = listener.accept().unwrap();
                let request = read_request(&mut stream);
                sender.send(request).unwrap();
                thread::sleep(Duration::from_millis(20));
                let _ = stream.write_all(response.as_bytes());
            }
        });
        (format!("http://{address}"), receiver, thread)
    }

    fn read_request(stream: &mut TcpStream) -> Vec<u8> {
        let mut request = Vec::new();
        let mut buffer = [0; 4096];
        let header_end = loop {
            let count = match stream.read(&mut buffer) {
                Ok(count) => count,
                Err(error)
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted
                    ) =>
                {
                    thread::sleep(Duration::from_millis(1));
                    continue;
                }
                Err(error) => panic!("local server read: {error}"),
            };
            if count == 0 {
                break request.len();
            }
            request.extend_from_slice(&buffer[..count]);
            if let Some(end) = request.windows(4).position(|window| window == b"\r\n\r\n") {
                break end + 4;
            }
        };
        let headers = String::from_utf8_lossy(&request[..header_end]);
        let length = headers
            .lines()
            .find_map(|line| line.strip_prefix("Content-Length: "))
            .and_then(|value| value.trim().parse::<usize>().ok())
            .unwrap_or(0);
        while request.len() < header_end + length {
            let count = match stream.read(&mut buffer) {
                Ok(count) => count,
                Err(error)
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted
                    ) =>
                {
                    thread::sleep(Duration::from_millis(1));
                    continue;
                }
                Err(error) => panic!("local server read: {error}"),
            };
            if count == 0 {
                break;
            }
            request.extend_from_slice(&buffer[..count]);
        }
        request
    }

    #[test]
    fn status_error_redacts_credentials_and_tokens() {
        let request = HttpRequest::new(
            "GET",
            "https://user:proxy-secret@example.test/path?token=url-secret&api_key=url-key",
        )
        .header("Authorization", "Bearer super-secret-token")
        .header("X-Api-Key", "api-key-value");
        let error = http_status_error(
            401,
            br#"{"token":"body-token","token":"second-token","api_key":"body-key","message":"nope"}"#,
            &request,
        );
        let text = error.to_string();
        assert!(text.contains("HTTP 401"));
        assert!(!text.contains("super-secret-token"));
        assert!(!text.contains("api-key-value"));
        assert!(!text.contains("body-token"));
        assert!(!text.contains("second-token"));
        assert!(!text.contains("body-key"));
        assert!(!text.contains("proxy-secret"));
        assert!(!text.contains("url-secret"));
        assert!(!text.contains("url-key"));
    }

    #[test]
    fn status_error_redacts_camel_case_and_header_key_variants_repeatedly() {
        let request = HttpRequest::new(
            "POST",
            "https://example.test/transcribe?apiKey=query-api-key&accessToken=query-access-token&x-api-key=query-x-api-key",
        );
        let error = http_status_error(
            403,
            br#"{"apiKey":"json-api-key","apiKey":"json-api-key-again","accessToken":"json-access-token","accessToken":"json-access-token-again","x-api-key":"json-x-api-key","x-api-key":"json-x-api-key-again"}"#,
            &request,
        );
        let text = error.to_string();
        for secret in [
            "query-api-key",
            "query-access-token",
            "query-x-api-key",
            "json-api-key",
            "json-api-key-again",
            "json-access-token",
            "json-access-token-again",
            "json-x-api-key",
            "json-x-api-key-again",
        ] {
            assert!(!text.contains(secret), "secret leaked: {secret}");
        }
    }

    #[test]
    fn multipart_encoding_is_ordered_and_byte_exact() {
        let body = MultipartEncoder::new("BOUNDARY")
            .field("model", "tiny")
            .file("file", "audio.wav", "audio/wav", b"WAV".to_vec())
            .encode();
        assert_eq!(
            body,
            b"--BOUNDARY\r\nContent-Disposition: form-data; name=\"model\"\r\n\r\ntiny\r\n--BOUNDARY\r\nContent-Disposition: form-data; name=\"file\"; filename=\"audio.wav\"\r\nContent-Type: audio/wav\r\n\r\nWAV\r\n--BOUNDARY--\r\n"
        );
    }

    #[test]
    fn multipart_content_type_uses_boundary() {
        assert_eq!(
            MultipartEncoder::new("test-boundary").content_type(),
            "multipart/form-data; boundary=test-boundary"
        );
    }

    #[test]
    fn multipart_rejects_boundary_collision() {
        let multipart = MultipartEncoder::new("BOUNDARY").field("data", "contains BOUNDARY");
        assert!(multipart.try_encode().is_err());
    }

    #[test]
    fn multipart_rejects_unsafe_names_and_filenames() {
        assert!(MultipartEncoder::new("BOUNDARY")
            .field("bad\r\nX-Injected: yes", "value")
            .try_encode()
            .is_err());
        assert!(MultipartEncoder::new("BOUNDARY")
            .file("file", "bad\\\".wav", "audio/wav", b"WAV".to_vec())
            .try_encode()
            .is_err());
    }

    #[test]
    fn random_multipart_boundary_does_not_collide() {
        let multipart = MultipartEncoder::random().field("data", "payload");
        assert!(multipart.try_encode().is_ok());
    }

    #[test]
    fn header_validation_rejects_empty_names_and_injection_characters() {
        assert!(validate_header("", "value").is_err());
        for (name, value) in [
            ("X-Test\rInjected", "value"),
            ("X-Test", "value\nInjected"),
            ("X-Test", "value\0Injected"),
        ] {
            let error = validate_header(name, value).unwrap_err().to_string();
            assert_eq!(error, "http header contains unsafe characters");
            assert!(!error.contains(value));
        }
    }

    #[tokio::test]
    async fn send_follows_redirect() {
        let (endpoint, requests, server) = local_server(vec![
            "HTTP/1.1 302 Found\r\nLocation: /final\r\nContent-Length: 0\r\n\r\n",
            "HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok",
        ]);
        let _guard = crate::services::proxy::test_runtime_guard(
            Some(crate::services::proxy::ProxyRoute::Direct),
            vec![],
        );
        let response = BatchHttpClient::new(&endpoint)
            .unwrap()
            .send(HttpRequest::new("GET", format!("{endpoint}/start")))
            .await
            .unwrap();
        assert_eq!(response.status, 200);
        assert_eq!(response.body, b"ok");
        let _ = requests.recv().unwrap();
        assert!(requests.recv().is_ok());
        server.join().unwrap();
    }

    #[tokio::test]
    async fn send_rejects_unsafe_headers_before_transport() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        listener.set_nonblocking(true).unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let (sender, receiver) = mpsc::channel();
        let server = thread::spawn(move || {
            let deadline = std::time::Instant::now() + Duration::from_millis(250);
            loop {
                match listener.accept() {
                    Ok(_) => {
                        sender.send(true).unwrap();
                        return;
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        if std::time::Instant::now() >= deadline {
                            sender.send(false).unwrap();
                            return;
                        }
                        thread::sleep(Duration::from_millis(1));
                    }
                    Err(error) => panic!("local server accept: {error}"),
                }
            }
        });
        let _guard = crate::services::proxy::test_runtime_guard(
            Some(crate::services::proxy::ProxyRoute::Direct),
            vec![],
        );
        let result = BatchHttpClient::new(&endpoint)
            .unwrap()
            .send(HttpRequest::new("GET", endpoint).header("X-Test", "unsafe\r\nvalue"))
            .await;
        assert_eq!(
            result.unwrap_err().to_string(),
            "http header contains unsafe characters"
        );
        assert!(!receiver.recv().unwrap());
        server.join().unwrap();
    }

    #[tokio::test]
    async fn send_replays_body_on_temporary_redirect() {
        let (endpoint, requests, server) = local_server(vec![
            "HTTP/1.1 307 Temporary Redirect\r\nLocation: /retry\r\nContent-Length: 0\r\n\r\n",
            "HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok",
        ]);
        let _guard = crate::services::proxy::test_runtime_guard(
            Some(crate::services::proxy::ProxyRoute::Direct),
            vec![],
        );
        let response = BatchHttpClient::new(&endpoint)
            .unwrap()
            .send(HttpRequest::new("POST", format!("{endpoint}/upload")).body(b"audio".to_vec()))
            .await
            .unwrap();
        assert_eq!(response.status, 200);
        let first = requests.recv().unwrap();
        let second = requests.recv().unwrap();
        assert!(String::from_utf8_lossy(&first).ends_with("audio"));
        assert!(String::from_utf8_lossy(&second).ends_with("audio"));
        server.join().unwrap();
    }

    #[tokio::test]
    async fn explicit_proxy_407_diagnostic_redacts_secret() {
        let (proxy, requests, server) = local_server(vec![
            "HTTP/1.1 407 Proxy Authentication Required\r\nContent-Length: 30\r\n\r\n{\"accessToken\":\"proxy-secret\"}",
        ]);
        let _guard = crate::services::proxy::test_runtime_guard(
            Some(crate::services::proxy::ProxyRoute::Explicit {
                url: proxy,
                credentials: None,
            }),
            vec![],
        );
        let request = HttpRequest::new("GET", "http://destination.invalid/transcribe")
            .header("Authorization", "Bearer header-secret");
        let response = BatchHttpClient::new(&request.url)
            .unwrap()
            .send(request.clone())
            .await
            .unwrap();
        assert_eq!(response.status, 407);
        let error = http_status_error(response.status, &response.body, &request).to_string();
        assert!(error.contains("HTTP 407"));
        assert!(!error.contains("proxy-secret"));
        assert!(!error.contains("header-secret"));
        assert!(String::from_utf8_lossy(&requests.recv().unwrap()).contains("destination.invalid"));
        server.join().unwrap();
    }

    #[tokio::test]
    async fn send_timeout_is_classified() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let (shutdown_sender, shutdown_receiver) = mpsc::channel();
        let server = thread::spawn(move || loop {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    let _ = read_request(&mut stream);
                    shutdown_receiver.recv().unwrap();
                    break;
                }
                Err(error) => panic!("local server accept: {error}"),
            }
        });
        let _guard = crate::services::proxy::test_runtime_guard(
            Some(crate::services::proxy::ProxyRoute::Direct),
            vec![],
        );
        let result = BatchHttpClient::new(&endpoint)
            .unwrap()
            .send_with_timeout(HttpRequest::new("GET", endpoint), Duration::from_millis(1))
            .await;
        shutdown_sender.send(()).unwrap();
        server.join().unwrap();
        let error = result.unwrap_err();
        assert!(error.to_string().starts_with("timeout:"));
    }

    #[tokio::test]
    async fn explicit_no_proxy_bypasses_proxy() {
        let (target, requests, target_server) =
            local_server(vec!["HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok"]);
        let proxy_listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let proxy = format!("http://{}", proxy_listener.local_addr().unwrap());
        drop(proxy_listener);
        let target_url = Url::parse(&target).unwrap();
        let host_port = format!(
            "{}:{}",
            target_url.host_str().unwrap(),
            target_url.port_or_known_default().unwrap()
        );
        let _guard = crate::services::proxy::test_runtime_guard(
            Some(crate::services::proxy::ProxyRoute::Explicit {
                url: proxy,
                credentials: None,
            }),
            vec![host_port],
        );
        let response = BatchHttpClient::new(&target)
            .unwrap()
            .send(HttpRequest::new("GET", target.clone()))
            .await
            .unwrap();
        assert_eq!(response.status, 200);
        assert!(String::from_utf8_lossy(&requests.recv().unwrap()).contains("GET / HTTP/1.1"));
        target_server.join().unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn system_route_contract_stays_native_for_batch_transport() {
        let _guard = crate::services::proxy::test_runtime_guard(
            Some(crate::services::proxy::ProxyRoute::System),
            vec!["service.example".into()],
        );
        let route = crate::services::proxy::route_for_url(
            &Url::parse("https://service.example/transcribe").unwrap(),
        )
        .unwrap();

        // WinHTTP owns System routing, including proxy bypass policy. The
        // batch seam must not turn this into reqwest when application-level
        // no-proxy rules contain the destination.
        assert!(matches!(route, crate::services::proxy::ProxyRoute::System));
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn system_batch_boundary_uses_winhttp_without_reqwest_fallback() {
        let (endpoint, requests, server) =
            local_server(vec!["HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok"]);
        let _guard = crate::services::proxy::test_runtime_guard(
            Some(crate::services::proxy::ProxyRoute::System),
            vec![],
        );
        let response = BatchHttpClient::new(&endpoint)
            .unwrap()
            .send(HttpRequest::new("GET", endpoint.clone()))
            .await
            .unwrap();

        // System routing reaches the public batch seam without falling back to
        // reqwest. The response contract stays transport-neutral.
        assert_eq!(response.status, 200);
        assert_eq!(response.body, b"ok");
        assert!(String::from_utf8_lossy(&requests.recv().unwrap()).contains("GET / HTTP/1.1"));
        server.join().unwrap();
    }
}
