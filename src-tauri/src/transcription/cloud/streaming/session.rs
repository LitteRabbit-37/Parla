// Types et trait partages pour les sessions de streaming.
//
// Flux global :
//   1. Le frontend demande "start_cloud_streaming" -> on cree un StreamingHandle.
//   2. Le recorder audio pousse des chunks Int16 via handle.push_audio().
//   3. Chaque chunk passe par un task tokio specifique au provider qui
//      convertit + envoie au WebSocket.
//   4. Le provider emet StreamingEvent::Partial / Committed au fur et a mesure.
//   5. Le frontend demande "finalize_cloud_streaming" -> commit WebSocket,
//      on recupere le texte final.

use std::time::Duration;

use anyhow::{anyhow, Context};
use async_trait::async_trait;
use serde::Serialize;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::{mpsc, oneshot};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::handshake::client::Request as WsRequest;

pub(crate) trait ProxyIo: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> ProxyIo for T {}
type BoxedIo = Box<dyn ProxyIo>;
type WsStream = tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<BoxedIo>>;

/// Timeout pour l'etablissement d'une connexion WebSocket streaming.
/// Une fois connecte, le flux peut durer indefiniment (pas de timeout global).
pub const WS_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

/// Etablit une connexion WebSocket avec un timeout de handshake.
/// Remplace l'appel direct a `tokio_tungstenite::connect_async` pour eviter
/// les hangs indefinis si le handshake bloque (firewall, proxy, DNS lent).
pub async fn connect_ws(req: WsRequest) -> anyhow::Result<WsStream> {
    let url_for_err = crate::services::download::sanitize_message(&req.uri().to_string());
    match tokio::time::timeout(WS_CONNECT_TIMEOUT, connect_ws_inner(req)).await {
        Ok(result) => result,
        Err(_) => Err(anyhow!(
            "timeout: ws connect {url_for_err} (>{}s)",
            WS_CONNECT_TIMEOUT.as_secs()
        )),
    }
}

async fn connect_ws_inner(req: WsRequest) -> anyhow::Result<WsStream> {
    let request_url = req.uri().to_string();
    let url_for_err = crate::services::download::sanitize_message(&request_url);
    let target = url::Url::parse(&request_url).context("WebSocket URL")?;
    let route = crate::services::proxy::route_for_url(&target)?;
    let host = target
        .host_str()
        .ok_or_else(|| anyhow!("WebSocket URL has no host"))?;
    let port = target
        .port_or_known_default()
        .ok_or_else(|| anyhow!("WebSocket URL has no port"))?;
    let addr = if host.contains(':') {
        format!("[{host}]:{port}")
    } else {
        format!("{host}:{port}")
    };
    let socket: BoxedIo = match route {
        crate::services::proxy::ProxyRoute::Direct => Box::new(TcpStream::connect(&addr).await?),
        crate::services::proxy::ProxyRoute::Explicit {
            url: proxy_url,
            credentials,
        } => {
            let proxy = url::Url::parse(&proxy_url)?;
            let proxy_host = proxy
                .host_str()
                .ok_or_else(|| anyhow!("proxy has no host"))?;
            let proxy_port = proxy
                .port_or_known_default()
                .ok_or_else(|| anyhow!("proxy has no port"))?;
            let proxy_addr = if proxy_host.contains(':') {
                format!("[{proxy_host}]:{proxy_port}")
            } else {
                format!("{proxy_host}:{proxy_port}")
            };
            match proxy.scheme() {
                "socks5" => Box::new(
                    match credentials {
                        Some(c) => {
                            tokio_socks::tcp::Socks5Stream::connect_with_password(
                                proxy_addr.as_str(),
                                addr,
                                &c.username,
                                &c.password,
                            )
                            .await?
                        }
                        None => {
                            tokio_socks::tcp::Socks5Stream::connect(proxy_addr.as_str(), addr)
                                .await?
                        }
                    }
                    .into_inner(),
                ),
                "http" | "https" => {
                    let raw = TcpStream::connect(&proxy_addr).await?;
                    let mut stream: BoxedIo = if proxy.scheme() == "https" {
                        let roots = tokio_rustls::rustls::RootCertStore::from_iter(
                            webpki_roots::TLS_SERVER_ROOTS.iter().cloned(),
                        );
                        let config = tokio_rustls::rustls::ClientConfig::builder()
                            .with_root_certificates(roots)
                            .with_no_client_auth();
                        let name =
                            tokio_rustls::rustls::pki_types::ServerName::try_from(proxy_host)
                                .map_err(|_| anyhow!("invalid HTTPS proxy host"))?
                                .to_owned();
                        Box::new(
                            tokio_rustls::TlsConnector::from(std::sync::Arc::new(config))
                                .connect(name, raw)
                                .await?,
                        )
                    } else {
                        Box::new(raw)
                    };
                    let mut connect = format!("CONNECT {addr} HTTP/1.1\r\nHost: {addr}\r\n");
                    if let Some(c) = credentials {
                        use base64::Engine;
                        let token = base64::engine::general_purpose::STANDARD
                            .encode(format!("{}:{}", c.username, c.password));
                        connect.push_str(&format!("Proxy-Authorization: Basic {token}\r\n"));
                    }
                    connect.push_str("\r\n");
                    stream.write_all(connect.as_bytes()).await?;
                    let mut response = Vec::new();
                    let mut buf = [0u8; 512];
                    while !response.windows(4).any(|w| w == b"\r\n\r\n") {
                        let n = stream.read(&mut buf).await?;
                        if n == 0 || response.len() > 16 * 1024 {
                            return Err(anyhow!("proxy CONNECT response invalid"));
                        }
                        response.extend_from_slice(&buf[..n]);
                    }
                    let first = std::str::from_utf8(&response)?.lines().next().unwrap_or("");
                    if !first.contains(" 200 ") {
                        return Err(anyhow!("proxy CONNECT failed"));
                    }
                    Box::new(stream)
                }
                scheme => return Err(anyhow!("unsupported WebSocket proxy scheme: {scheme}")),
            }
        }
        crate::services::proxy::ProxyRoute::System => {
            return Err(anyhow!(
                "Windows system proxy is not supported for streaming WebSocket connections"
            ));
        }
    };
    tokio_tungstenite::client_async_tls_with_config(req, socket, None, None)
        .await
        .map(|(stream, _)| stream)
        .map_err(|e| anyhow!("ws connect {url_for_err}: {e}"))
}

/// Variante prenant directement une string URL, pour les providers qui n'ont
/// pas besoin d'ajouter de headers custom avant le connect.
pub async fn connect_ws_url(url: &str) -> anyhow::Result<WsStream> {
    let url_for_err = crate::services::download::sanitize_message(url);
    let req = url
        .into_client_request()
        .map_err(|e| anyhow!("ws url parse {url_for_err}: {e}"))?;
    connect_ws(req).await
}

/// Evenements emis par une session de streaming vers le frontend.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StreamingEvent {
    /// Connexion etablie et handshake reussi.
    SessionStarted,
    /// Texte partiel (non final) en cours de reconnaissance.
    Partial { text: String },
    /// Morceau final commite. Les morceaux commit s'accumulent.
    Committed { text: String },
    /// Erreur remontee par le provider (non fatale ou fatale).
    Error { message: String },
}

/// Configuration de la session.
#[derive(Debug, Clone)]
pub struct StreamingConfig {
    pub model: String,
    pub language: Option<String>,
    pub custom_vocabulary: Vec<String>,
}

/// Handle utilise par le pipeline pour pousser de l'audio et demander
/// la finalisation.
pub struct StreamingHandle {
    audio_tx: mpsc::UnboundedSender<Vec<i16>>,
    finalize_tx: Option<oneshot::Sender<()>>,
    done_rx: oneshot::Receiver<anyhow::Result<String>>,
}

impl StreamingHandle {
    pub fn new(
        audio_tx: mpsc::UnboundedSender<Vec<i16>>,
        finalize_tx: oneshot::Sender<()>,
        done_rx: oneshot::Receiver<anyhow::Result<String>>,
    ) -> Self {
        Self {
            audio_tx,
            finalize_tx: Some(finalize_tx),
            done_rx,
        }
    }

    /// Retourne un sender clone pour que le recorder puisse pousser l'audio
    /// directement sans passer par le state Tauri (hot-path a 30-100 Hz).
    pub fn audio_sender(&self) -> mpsc::UnboundedSender<Vec<i16>> {
        self.audio_tx.clone()
    }

    /// Envoie le signal de finalisation et attend le texte final.
    pub async fn finalize(mut self) -> anyhow::Result<String> {
        if let Some(tx) = self.finalize_tx.take() {
            let _ = tx.send(());
        }
        match self.done_rx.await {
            Ok(r) => r,
            Err(_) => Err(anyhow::anyhow!("streaming task a panic")),
        }
    }
}

/// Canaux internes passes aux run() de chaque provider.
pub struct StreamingChannels {
    pub audio_rx: mpsc::UnboundedReceiver<Vec<i16>>,
    pub finalize_rx: oneshot::Receiver<()>,
}

/// Trait implemente par chaque provider streaming.
#[async_trait]
pub trait StreamingProvider: Send + Sync {
    fn id(&self) -> &'static str;

    /// Execute la session du debut a la fin. Emet les evenements via on_event.
    /// Retourne le texte final concatene a la cloture du WebSocket.
    async fn run(
        &self,
        api_key: String,
        config: StreamingConfig,
        channels: StreamingChannels,
        on_event: Box<dyn Fn(StreamingEvent) + Send + Sync>,
    ) -> anyhow::Result<String>;
}

/// Draine les messages WebSocket jusqu'a un timeout ou fermeture.
/// Utilise apres envoi du commit pour recuperer les derniers transcripts.
/// Chaque message texte est passe au callback fourni.
pub async fn drain_ws_messages<S, F>(read: &mut S, timeout: std::time::Duration, mut on_text: F)
where
    S: futures_util::StreamExt<
            Item = Result<
                tokio_tungstenite::tungstenite::Message,
                tokio_tungstenite::tungstenite::Error,
            >,
        > + Unpin,
    F: FnMut(&str),
{
    use tokio_tungstenite::tungstenite::Message;
    let deadline = tokio::time::Instant::now() + timeout;
    while tokio::time::Instant::now() < deadline {
        let remaining = deadline - tokio::time::Instant::now();
        match tokio::time::timeout(remaining, read.next()).await {
            Ok(Some(Ok(Message::Text(t)))) => on_text(&t),
            Ok(Some(Ok(Message::Close(_)))) | Ok(None) | Err(_) => break,
            _ => {}
        }
    }
}

/// Formatte un buffer i16 en bytes little-endian.
pub fn i16_to_le_bytes(chunk: &[i16]) -> Vec<u8> {
    let mut out = Vec::with_capacity(chunk.len() * 2);
    for &s in chunk {
        out.extend_from_slice(&s.to_le_bytes());
    }
    out
}

/// Encode un chunk i16 LE en base64 (pour les providers JSON).
pub fn i16_to_base64(chunk: &[i16]) -> String {
    use base64::{engine::general_purpose::STANDARD as B64, Engine};
    B64.encode(i16_to_le_bytes(chunk))
}
