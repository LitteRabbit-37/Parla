use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, OnceLock,
};
use std::time::Duration;

use anyhow::{anyhow, Context, Error, Result};
use futures_util::StreamExt;

const BODY_STALL_TIMEOUT: Duration = Duration::from_secs(30);
// Bound connection and response-header setup without limiting time spent
// receiving a healthy large response body.
const REQUEST_SETUP_TIMEOUT: Duration = Duration::from_secs(15 * 60);

#[cfg(windows)]
const MAX_SYSTEM_DOWNLOAD_WORKERS: usize = 4;
#[cfg(windows)]
const SYSTEM_DOWNLOAD_OPERATION_TIMEOUT: Duration = Duration::from_secs(60 * 60);

#[cfg(windows)]
fn system_download_admission() -> &'static Arc<tokio::sync::Semaphore> {
    static ADMISSION: OnceLock<Arc<tokio::sync::Semaphore>> = OnceLock::new();
    ADMISSION.get_or_init(|| Arc::new(tokio::sync::Semaphore::new(MAX_SYSTEM_DOWNLOAD_WORKERS)))
}

struct CancellationGuard {
    flag: Arc<AtomicBool>,
    armed: bool,
}

impl CancellationGuard {
    fn new(flag: Arc<AtomicBool>) -> Self {
        Self { flag, armed: true }
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for CancellationGuard {
    fn drop(&mut self) {
        if self.armed {
            self.flag.store(true, Ordering::SeqCst);
        }
    }
}

fn cancellation_requested(cancel: Option<&Arc<AtomicBool>>) -> bool {
    cancel.is_some_and(|flag| flag.load(Ordering::SeqCst))
}

fn check_cancellation(cancel: Option<&Arc<AtomicBool>>) -> Result<()> {
    if cancellation_requested(cancel) {
        Err(anyhow!("telechargement annule"))
    } else {
        Ok(())
    }
}

async fn rename_unless_cancelled(
    tmp: &Path,
    target: &Path,
    cancel: Option<&Arc<AtomicBool>>,
) -> Result<()> {
    tokio::fs::rename(tmp, target).await?;
    if cancellation_requested(cancel) {
        let cancellation = check_cancellation(cancel).unwrap_err();
        let cleanup = tokio::fs::remove_file(target).await;
        return match cleanup {
            Ok(()) => Err(cancellation),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Err(cancellation),
            Err(error) => Err(cancellation.context(format!(
                "could not remove completed download after cancellation: {error}"
            ))),
        };
    }
    Ok(())
}

/// Aggregates progress when one logical download consists of several files.
#[derive(Debug, Default)]
pub struct DownloadProgressAggregator {
    pub total: u64,
    completed: u64,
    current: Option<(String, u64)>,
}

impl DownloadProgressAggregator {
    pub fn new(total: u64) -> Self {
        Self {
            total,
            ..Default::default()
        }
    }

    pub fn update(&mut self, file: &str, downloaded: u64) -> u64 {
        self.current = Some((file.to_owned(), downloaded));
        self.completed + downloaded
    }

    pub fn complete_file(&mut self, file: &str, size: u64) -> u64 {
        let current = self
            .current
            .take()
            .filter(|(name, _)| name == file)
            .map(|(_, n)| n)
            .unwrap_or(size);
        self.completed += current;
        self.completed
    }

    pub fn complete(&self) -> u64 {
        self.completed
    }
}

/// Download URL to target through configured proxy route, writing atomically.
/// Progress callback receives downloaded bytes and total bytes.
pub async fn download_to_file<F>(
    url: &url::Url,
    target: &Path,
    expected_size: u64,
    cancel: Option<Arc<AtomicBool>>,
    on_progress: F,
) -> Result<u64>
where
    F: FnMut(u64, u64) + Send + 'static,
{
    if let Some(parent) = target.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    // Each invocation gets its own path. In particular, a timed-out WinHTTP
    // task may still be writing after its JoinHandle is dropped, so retrying
    // must never remove or reuse its partial file.
    let tmp = attempt_part_path(target);
    let result = async {
        #[cfg(windows)]
        {
            let system_route = matches!(
                crate::services::proxy::route_for_url(url)?,
                crate::services::proxy::ProxyRoute::System
            );
            if !system_route {
                return download_non_system(
                    url,
                    target,
                    expected_size,
                    cancel.clone(),
                    on_progress,
                    tmp.clone(),
                )
                .await;
            }
            let cancel_for_worker = cancel
                .clone()
                .unwrap_or_else(|| Arc::new(AtomicBool::new(false)));
            if cancel_for_worker.load(Ordering::SeqCst) {
                return Err(anyhow!("telechargement annule"));
            }
            // Keep admission shared across all callers. A detached blocking
            // task can outlive its async owner while WinHTTP finishes its
            // bounded I/O timeout, so unbounded spawning would exhaust the
            // blocking pool under repeated cancellation.
            let semaphore = system_download_admission().clone();
            let permit = loop {
                if cancel_for_worker.load(Ordering::SeqCst) {
                    return Err(anyhow!("telechargement annule"));
                }
                match tokio::time::timeout(
                    Duration::from_millis(100),
                    semaphore.clone().acquire_owned(),
                )
                .await
                {
                    Ok(Ok(permit)) => break permit,
                    Ok(Err(_)) => return Err(anyhow!("system download worker admission closed")),
                    Err(_) => continue,
                }
            };
            // Permit acquisition is the queue boundary. Keep cancellation responsive while
            // waiting, and report queued state through the shared diagnostic path on failure.
            let mut cancellation = CancellationGuard::new(cancel_for_worker.clone());
            let worker_cancel = cancel_for_worker.clone();
            let post_worker_cancel = cancel_for_worker.clone();
            let url_for_worker = url.clone();
            let tmp_for_worker = tmp.clone();
            let result = tokio::time::timeout(
                SYSTEM_DOWNLOAD_OPERATION_TIMEOUT,
                tokio::task::spawn_blocking(move || {
                    use std::io::Write;
                    let _permit = permit;
                    let result = (|| -> Result<u64> {
                        let mut file = std::fs::File::create(&tmp_for_worker)?;
                        let mut callback = on_progress;
                        let downloaded = crate::services::winhttp_download::download(
                            &url_for_worker,
                            expected_size,
                            |chunk, downloaded, total| {
                                if worker_cancel.load(Ordering::SeqCst)
                                    || file.write_all(chunk).is_err()
                                {
                                    return false;
                                }
                                if worker_cancel.load(Ordering::SeqCst) {
                                    return false;
                                }
                                callback(downloaded, total);
                                true
                            },
                        )?;
                        if worker_cancel.load(Ordering::SeqCst) {
                            return Err(anyhow!("telechargement annule"));
                        }
                        file.sync_all()?;
                        Ok(downloaded)
                    })();
                    if result.is_err() {
                        let _ = std::fs::remove_file(&tmp_for_worker);
                    }
                    result
                }),
            )
            .await
            .map_err(|_| anyhow!("system download operation timed out"))?
            .map_err(|e| anyhow!("download worker failed: {e}"))??;
            check_cancellation(Some(&post_worker_cancel))?;
            rename_unless_cancelled(&tmp, target, Some(&post_worker_cancel)).await?;
            cancellation.disarm();
            return Ok(result);
        }

        #[cfg(not(windows))]
        {
            download_non_system(url, target, expected_size, cancel, on_progress, tmp.clone()).await
        }
    };
    let result = result.await;

    if result.is_err() {
        // Covers HTTP/body/write failures and WinHTTP worker cancellation or
        // system errors, including a partial file left by the worker.
        #[cfg(windows)]
        let system_route = matches!(
            crate::services::proxy::route_for_url(url),
            Ok(crate::services::proxy::ProxyRoute::System)
        );
        #[cfg(not(windows))]
        let system_route = false;
        // System worker owns cleanup after cancellation. Removing here could
        // race with its final write while WinHTTP is finishing its I/O timeout.
        if !system_route {
            let _ = tokio::fs::remove_file(&tmp).await;
        }
    }
    result
}

async fn download_non_system<F>(
    url: &url::Url,
    target: &Path,
    expected_size: u64,
    cancel: Option<Arc<AtomicBool>>,
    mut on_progress: F,
    tmp: PathBuf,
) -> Result<u64>
where
    F: FnMut(u64, u64) + Send + 'static,
{
    let client = crate::services::proxy::client_for_url(url)?;
    let response = tokio::time::timeout(REQUEST_SETUP_TIMEOUT, client.get(url.as_str()).send())
        .await
        .map_err(|_| anyhow!("download request setup timed out"))?
        .with_context(|| format!("GET {}", sanitize_message(url.as_str())))?;
    if !response.status().is_success() {
        return Err(anyhow!("HTTP status {}", response.status()));
    }
    let total = response.content_length().unwrap_or(expected_size);
    let mut stream = response.bytes_stream();
    let mut file = tokio::fs::File::create(&tmp).await?;
    let mut downloaded = 0;
    while let Some(chunk) = tokio::time::timeout(BODY_STALL_TIMEOUT, stream.next())
        .await
        .map_err(|_| anyhow!("download body stalled"))?
    {
        check_cancellation(cancel.as_ref())?;
        let bytes = chunk.context("download body")?;
        tokio::io::AsyncWriteExt::write_all(&mut file, &bytes).await?;
        downloaded += bytes.len() as u64;
        on_progress(downloaded, total);
    }
    file.sync_all().await?;
    drop(file);
    check_cancellation(cancel.as_ref())?;
    rename_unless_cancelled(&tmp, target, cancel.as_ref()).await?;
    Ok(downloaded)
}

fn attempt_part_path(target: &Path) -> PathBuf {
    static NEXT_ATTEMPT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let attempt = NEXT_ATTEMPT.fetch_add(1, Ordering::Relaxed);
    let process = std::process::id();
    let name = target
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("download");
    target.with_file_name(format!("{name}.part.{process}.{attempt}"))
}

/// Formats download failures for UI without exposing credentials or URL query data.
pub fn diagnostic(error: &Error) -> String {
    let mut messages = Vec::new();
    for cause in error.chain() {
        let message = sanitize_message(&cause.to_string());
        if !message.is_empty() && messages.last() != Some(&message) {
            messages.push(message);
        }
    }
    messages.join("; ")
}

/// Redact URLs, URL userinfo, query strings, and credential-like values from
/// diagnostics before they reach logs or the frontend.
pub(crate) fn sanitize_message(message: &str) -> String {
    let mut sanitized = message.to_string();
    let mut search_from = 0;
    while let Some(relative) = sanitized[search_from..].find("://") {
        let marker = search_from + relative;
        let scheme_start = sanitized[..marker]
            .char_indices()
            .rev()
            .take_while(|(_, c)| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
            .last()
            .map(|(index, _)| index)
            .unwrap_or(marker);
        let scheme = &sanitized[scheme_start..marker];
        if scheme.is_empty() || !scheme.chars().next().unwrap().is_ascii_alphabetic() {
            search_from = marker + 3;
            continue;
        }
        let start = scheme_start;
        let end = sanitized[start..]
            .find(|c: char| {
                c.is_whitespace() || matches!(c, ')' | ']' | '}' | ',' | ';' | '"' | '\'')
            })
            .map(|offset| start + offset)
            .unwrap_or(sanitized.len());
        if let Ok(mut url) = url::Url::parse(&sanitized[start..end]) {
            let _ = url.set_username("");
            let _ = url.set_password(None);
            url.set_query(None);
            url.set_fragment(None);
            let replacement = url.to_string().trim_end_matches('/').to_owned();
            sanitized.replace_range(start..end, &replacement);
            search_from = start + replacement.len();
        } else {
            search_from = end;
        }
    }
    redact_credential_values(&sanitized)
}

fn redact_credential_values(message: &str) -> String {
    const KEYS: &[&str] = &[
        "password",
        "passwd",
        "token",
        "access_token",
        "api_key",
        "client_secret",
        "secret",
        "authorization",
        "bearer",
    ];
    let mut output = String::with_capacity(message.len());
    let mut cursor = 0;
    while cursor < message.len() {
        let lower = message[cursor..].to_ascii_lowercase();
        let Some((key_start, key)) = KEYS
            .iter()
            .filter_map(|key| lower.find(key).map(|offset| (cursor + offset, *key)))
            .min_by_key(|(start, _)| *start)
        else {
            output.push_str(&message[cursor..]);
            break;
        };
        let key_end = key_start + key.len();
        let boundary = key_start == 0
            || !message.as_bytes()[key_start - 1].is_ascii_alphanumeric()
                && message.as_bytes()[key_start - 1] != b'_';
        if !boundary || key_end >= message.len() {
            output.push_str(&message[cursor..key_end.min(message.len())]);
            cursor = key_end.min(message.len());
            continue;
        }
        let mut value_start = key_end;
        while value_start < message.len() && message.as_bytes()[value_start].is_ascii_whitespace() {
            value_start += 1;
        }
        if value_start >= message.len() || !matches!(message.as_bytes()[value_start], b'=' | b':') {
            output.push_str(&message[cursor..key_end]);
            cursor = key_end;
            continue;
        }
        value_start += 1;
        while value_start < message.len() && message.as_bytes()[value_start].is_ascii_whitespace() {
            value_start += 1;
        }
        if message[value_start..]
            .to_ascii_lowercase()
            .starts_with("bearer ")
        {
            value_start += "bearer ".len();
        }
        let value_end = if message.as_bytes().get(value_start) == Some(&b'"') {
            let mut index = value_start + 1;
            let mut escaped = false;
            while index < message.len() {
                let byte = message.as_bytes()[index];
                if byte == b'"' && !escaped {
                    break;
                }
                if byte == b'\\' {
                    escaped = !escaped;
                } else {
                    escaped = false;
                }
                index += 1;
            }
            index.min(message.len())
        } else {
            message[value_start..]
                .find(|c: char| c.is_whitespace() || matches!(c, '&' | ',' | ';' | ')' | ']' | '}'))
                .map(|offset| value_start + offset)
                .unwrap_or(message.len())
        };
        output.push_str(&message[cursor..value_start]);
        output.push_str("[redacted]");
        cursor = value_end;
    }
    output
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::sync::{atomic::AtomicBool, Arc};

    use super::{attempt_part_path, sanitize_message};

    #[test]
    fn download_attempts_never_reuse_partial_path() {
        let target = Path::new("model.onnx");
        assert_ne!(attempt_part_path(target), attempt_part_path(target));
    }

    #[test]
    fn cancellation_check_reports_shared_flag() {
        let flag = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        assert!(super::check_cancellation(Some(&flag)).is_ok());
        flag.store(true, std::sync::atomic::Ordering::SeqCst);
        assert_eq!(
            super::check_cancellation(Some(&flag))
                .unwrap_err()
                .to_string(),
            "telechargement annule"
        );
    }

    #[tokio::test]
    async fn cancellation_after_rename_removes_completed_target() {
        let root = std::env::temp_dir().join(format!(
            "parla-download-test-{}-{}",
            std::process::id(),
            super::attempt_part_path(Path::new("seed"))
                .file_name()
                .unwrap()
                .to_string_lossy()
        ));
        tokio::fs::create_dir_all(&root).await.unwrap();
        let tmp = root.join("download.part");
        let target: PathBuf = root.join("download");
        tokio::fs::write(&tmp, b"complete").await.unwrap();
        let cancel = Arc::new(AtomicBool::new(true));

        let result = super::rename_unless_cancelled(&tmp, &target, Some(&cancel)).await;

        assert!(result.is_err());
        assert!(!tokio::fs::try_exists(&target).await.unwrap());
        assert!(!tokio::fs::try_exists(&tmp).await.unwrap());
        tokio::fs::remove_dir_all(root).await.unwrap();
    }

    #[test]
    fn sanitizes_arbitrary_scheme_userinfo_and_query() {
        let result =
            sanitize_message("request socks5://user:pass@example.test/path?token=secret&x=y");
        assert_eq!(result, "request socks5://example.test/path");
    }

    #[test]
    fn redacts_credential_keys_case_insensitively() {
        let result =
            sanitize_message("PASSWORD=one; Access_Token: two, Authorization: Bearer three");
        assert_eq!(
            result,
            "PASSWORD=[redacted]; Access_Token: [redacted], Authorization: Bearer [redacted]"
        );
    }

    #[test]
    fn redacts_json_password_value() {
        assert_eq!(
            sanitize_message(r#"response {"password":"secret"}"#),
            r#"response {"password":"[redacted]"}"#
        );
    }

    #[test]
    fn redacts_json_token_value() {
        assert_eq!(
            sanitize_message(r#"response {"token":"secret-token"}"#),
            r#"response {"token":"[redacted]"}"#
        );
    }

    #[test]
    fn redacts_json_authorization_value() {
        assert_eq!(
            sanitize_message(r#"response {"authorization":"Bearer secret"}"#),
            r#"response {"authorization":"[redacted]"}"#
        );
    }

    #[test]
    fn redacts_proxy_and_pac_urls() {
        let result = sanitize_message(
            "PAC https://user:pass@pac.example/config.pac?token=secret; proxy=http://u:p@proxy.example:8080",
        );
        assert_eq!(
            result,
            "PAC https://pac.example/config.pac; proxy=http://proxy.example:8080"
        );
    }
}
