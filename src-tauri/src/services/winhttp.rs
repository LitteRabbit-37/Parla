//! Small synchronous WinHTTP transport used when Windows owns proxy routing.

use crate::transcription::cloud::streaming::session::{
    StreamingMessage, StreamingSocket, StreamingSocketRead, StreamingSocketWrite,
};
use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::Duration;
use tokio::sync::oneshot;
use windows::core::{Error as WinError, PCWSTR};
use windows::Win32::Networking::WinHttp::*;

const CONNECT_TIMEOUT_MS: i32 = 15_000;
const RECEIVE_TIMEOUT_MS: i32 = 500;
const READ_SIZE: usize = 64 * 1024;
const CONNECT_CLEANUP_TIMEOUT: Duration = Duration::from_secs(10);

enum SendCommand {
    Send {
        buffer_type: WINHTTP_WEB_SOCKET_BUFFER_TYPE,
        data: Vec<u8>,
        reply: oneshot::Sender<Result<()>>,
    },
    Stop {
        reply: oneshot::Sender<()>,
    },
}
enum ReceiveCommand {
    Receive {
        reply: oneshot::Sender<Result<Option<StreamingMessage>>>,
    },
    Stop {
        reply: oneshot::Sender<()>,
    },
}
struct CloseRequest(Option<oneshot::Sender<Result<()>>>);

struct ChannelWrite {
    tx: std::sync::mpsc::Sender<SendCommand>,
    state: Arc<SocketState>,
}
struct ChannelRead {
    tx: std::sync::mpsc::Sender<ReceiveCommand>,
    state: Arc<SocketState>,
}

struct SocketState {
    closed: AtomicBool,
    close_queued: AtomicBool,
    close_tx: std::sync::mpsc::Sender<CloseRequest>,
    send_tx: std::sync::mpsc::Sender<SendCommand>,
    receive_tx: std::sync::mpsc::Sender<ReceiveCommand>,
}

impl SocketState {
    fn queue_close(&self) {
        self.closed.store(true, Ordering::Release);
        if !self.close_queued.swap(true, Ordering::AcqRel) {
            // A disconnected coordinator has already closed its native handle.
            // Do not wait for another task: this is only a best-effort drop path.
            let _ = self.close_tx.send(CloseRequest(None));
        }
    }

    fn claim_close(&self) -> bool {
        self.closed.store(true, Ordering::Release);
        !self.close_queued.swap(true, Ordering::AcqRel)
    }
}

impl Drop for ChannelWrite {
    fn drop(&mut self) {
        self.state.queue_close();
    }
}

impl Drop for ChannelRead {
    fn drop(&mut self) {
        self.state.queue_close();
    }
}

#[async_trait]
impl StreamingSocketWrite for ChannelWrite {
    async fn send_text(&mut self, text: String) -> Result<()> {
        if self.state.closed.load(Ordering::Acquire) {
            return Err(anyhow!("WinHTTP websocket is closed"));
        }
        let (reply, result) = oneshot::channel();
        self.tx
            .send(SendCommand::Send {
                buffer_type: WINHTTP_WEB_SOCKET_UTF8_MESSAGE_BUFFER_TYPE,
                data: text.into_bytes(),
                reply,
            })
            .map_err(|_| anyhow!("WinHTTP websocket send thread stopped"))?;
        result
            .await
            .map_err(|_| anyhow!("WinHTTP websocket send thread stopped"))?
    }
    async fn send_binary(&mut self, data: Vec<u8>) -> Result<()> {
        if self.state.closed.load(Ordering::Acquire) {
            return Err(anyhow!("WinHTTP websocket is closed"));
        }
        let (reply, result) = oneshot::channel();
        self.tx
            .send(SendCommand::Send {
                buffer_type: WINHTTP_WEB_SOCKET_BINARY_MESSAGE_BUFFER_TYPE,
                data,
                reply,
            })
            .map_err(|_| anyhow!("WinHTTP websocket send thread stopped"))?;
        result
            .await
            .map_err(|_| anyhow!("WinHTTP websocket send thread stopped"))?
    }
    async fn close(&mut self) -> Result<()> {
        if !self.state.claim_close() {
            return Ok(());
        }
        let (reply, result) = oneshot::channel();
        if self.state.close_tx.send(CloseRequest(Some(reply))).is_err() {
            // claim_close prevents another close attempt. The coordinator owns
            // the handle and closes it before its channel is disconnected.
            return Err(anyhow!("WinHTTP websocket close coordinator stopped"));
        }
        result
            .await
            .map_err(|_| anyhow!("WinHTTP websocket close coordinator stopped"))?
    }
}

#[async_trait]
impl StreamingSocketRead for ChannelRead {
    async fn next(&mut self) -> Result<Option<StreamingMessage>> {
        let (reply, result) = oneshot::channel();
        self.tx
            .send(ReceiveCommand::Receive { reply })
            .map_err(|_| anyhow!("WinHTTP websocket receive thread stopped"))?;
        result
            .await
            .map_err(|_| anyhow!("WinHTTP websocket receive thread stopped"))?
    }
}

/// Opens WinHTTP websocket with automatic proxy/WPAD and integrated auth.
/// Dedicated blocking thread owns handle; dropping command channel closes handle.
pub async fn connect_websocket(
    req: &tokio_tungstenite::tungstenite::handshake::client::Request,
    timeout: Duration,
) -> Result<StreamingSocket> {
    let request = req.clone();
    let cancellation = Arc::new(AtomicBool::new(false));
    let worker_cancellation = Arc::clone(&cancellation);
    let (result_tx, mut result_rx) = oneshot::channel();
    std::thread::Builder::new()
        .name("winhttp-websocket-connect".into())
        .spawn(move || {
            let result = connect_blocking(&request, timeout, &worker_cancellation);
            let _ = result_tx.send(result);
        })
        .map_err(|error| anyhow!("start WinHTTP websocket connect thread: {error}"))?;

    match tokio::time::timeout(timeout, &mut result_rx).await {
        Ok(Ok(result)) => result,
        Ok(Err(_)) => Err(anyhow!("WinHTTP websocket connect thread stopped")),
        Err(_) => {
            cancellation.store(true, Ordering::Release);
            // WinHTTP has no safe cross-thread cancellation primitive. The worker
            // owns every native handle and each native phase has a finite timeout.
            // Wait for that worker to release its handles, but never indefinitely.
            match tokio::time::timeout(CONNECT_CLEANUP_TIMEOUT, result_rx).await {
                Ok(Ok(_)) | Ok(Err(_)) => Err(anyhow!(
                    "timeout: WinHTTP websocket connect (>{}s)",
                    timeout.as_secs()
                )),
                Err(_) => Err(anyhow!(
                    "timeout: WinHTTP websocket connect cleanup exceeded {}s",
                    CONNECT_CLEANUP_TIMEOUT.as_secs()
                )),
            }
        }
    }
}

fn connect_blocking(
    req: &tokio_tungstenite::tungstenite::handshake::client::Request,
    timeout: Duration,
    cancelled: &AtomicBool,
) -> Result<StreamingSocket> {
    let timeout = timeout.min(CONNECT_CLEANUP_TIMEOUT);
    if cancelled.load(Ordering::Acquire) {
        return Err(anyhow!("WinHTTP websocket connect cancelled"));
    }
    let uri = req.uri().to_string();
    let parsed = url::Url::parse(&uri).context("WinHTTP WebSocket URL")?;
    let session = unsafe {
        WinHttpOpen(
            PCWSTR::null(),
            WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY,
            PCWSTR::null(),
            PCWSTR::null(),
            0,
        )
    };
    if session.is_null() {
        return Err(win_error("open session"));
    }
    let session = HandleGuard(session);
    unsafe {
        WinHttpSetTimeouts(
            session.0,
            timeout_ms(timeout),
            timeout_ms(timeout),
            timeout_ms(timeout),
            timeout_ms(timeout),
        )
        .map_err(|_| win_error("set websocket timeouts"))?;
    }
    let policy = WINHTTP_AUTOLOGON_SECURITY_LEVEL_MEDIUM;
    unsafe {
        WinHttpSetOption(
            Some(session.0.cast_const()),
            WINHTTP_OPTION_AUTOLOGON_POLICY,
            Some(std::slice::from_raw_parts(
                &policy as *const _ as *const u8,
                std::mem::size_of_val(&policy),
            )),
        )?;
    }
    let host = wide(
        parsed
            .host_str()
            .ok_or_else(|| anyhow!("WinHTTP URL has no host"))?,
    );
    let connection = unsafe {
        WinHttpConnect(
            session.0,
            PCWSTR(host.as_ptr()),
            parsed.port_or_known_default().unwrap_or(443),
            0,
        )
    };
    if connection.is_null() {
        return Err(win_error("connect"));
    }
    let connection = HandleGuard(connection);
    if cancelled.load(Ordering::Acquire) {
        return Err(anyhow!("WinHTTP websocket connect cancelled"));
    }
    let path = wide(&format!(
        "{}{}",
        parsed.path(),
        parsed.query().map(|q| format!("?{q}")).unwrap_or_default()
    ));
    let flags = if parsed.scheme().eq_ignore_ascii_case("wss") {
        WINHTTP_FLAG_SECURE
    } else {
        WINHTTP_OPEN_REQUEST_FLAGS(0)
    };
    let request_handle = unsafe {
        WinHttpOpenRequest(
            connection.0,
            PCWSTR(wide("GET").as_ptr()),
            PCWSTR(path.as_ptr()),
            PCWSTR::null(),
            PCWSTR::null(),
            std::ptr::null(),
            flags,
        )
    };
    if request_handle.is_null() {
        return Err(win_error("open websocket request"));
    }
    let request_handle = HandleGuard(request_handle);
    for (name, value) in req.headers() {
        if is_filtered_header(name.as_str()) {
            continue;
        }
        let value = value
            .to_str()
            .map_err(|_| anyhow!("WinHTTP websocket header is not valid UTF-8"))?;
        crate::transcription::cloud::http::validate_header(name.as_str(), value)?;
        let header = wide(&format!("{}: {}\r\n", name, value));
        unsafe {
            WinHttpAddRequestHeaders(
                request_handle.0,
                &header[..header.len() - 1],
                WINHTTP_ADDREQ_FLAG_ADD,
            )?;
        }
    }
    unsafe {
        WinHttpSetOption(
            Some(request_handle.0.cast_const()),
            WINHTTP_OPTION_UPGRADE_TO_WEB_SOCKET,
            None,
        )?;
        if cancelled.load(Ordering::Acquire) {
            return Err(anyhow!("WinHTTP websocket connect cancelled"));
        }
        WinHttpSendRequest(request_handle.0, None, None, 0, 0, 0)
            .map_err(|_| win_error("websocket connect"))?;
        WinHttpReceiveResponse(request_handle.0, std::ptr::null_mut())
            .map_err(|_| win_error("websocket connect"))?;
    }
    let status = unsafe { query_status(request_handle.0)? };
    if status != 101 {
        return Err(anyhow!(
            "WinHTTP websocket upgrade rejected with status {status}"
        ));
    }
    if cancelled.load(Ordering::Acquire) {
        return Err(anyhow!("WinHTTP websocket connect cancelled"));
    }
    let websocket = unsafe { WinHttpWebSocketCompleteUpgrade(request_handle.0, None) };
    if websocket.is_null() {
        return Err(win_error("websocket upgrade"));
    }
    // WinHttpWebSocketCompleteUpgrade transfers request ownership to websocket.
    std::mem::forget(request_handle);
    unsafe {
        WinHttpSetTimeouts(
            websocket,
            timeout_ms(timeout),
            RECEIVE_TIMEOUT_MS,
            RECEIVE_TIMEOUT_MS,
            RECEIVE_TIMEOUT_MS,
        )
        .map_err(|_| win_error("set websocket receive timeout"))?;
    }
    let (send_tx, send_rx) = std::sync::mpsc::channel();
    let (receive_tx, receive_rx) = std::sync::mpsc::channel();
    let (close_tx, close_rx) = std::sync::mpsc::channel();
    let state = Arc::new(SocketState {
        closed: AtomicBool::new(false),
        close_queued: AtomicBool::new(false),
        close_tx,
        send_tx: send_tx.clone(),
        receive_tx: receive_tx.clone(),
    });
    let thread_state = Arc::clone(&state);
    let cancel = Arc::new(AtomicBool::new(false));
    let (ready_tx, ready_rx) = std::sync::mpsc::sync_channel(1);
    let websocket = websocket as usize;
    if let Err(error) = std::thread::Builder::new()
        .name("winhttp-websocket-coordinator".into())
        .spawn(move || {
            websocket_coordinator(
                websocket,
                thread_state,
                cancel,
                send_rx,
                receive_rx,
                close_rx,
                ready_tx,
            )
        })
    {
        // Worker does not own handle until spawn succeeds.
        unsafe {
            let _ = WinHttpCloseHandle(websocket as *mut std::ffi::c_void);
        }
        return Err(anyhow!("start WinHTTP websocket thread: {error}"));
    }
    ready_rx
        .recv()
        .map_err(|_| anyhow!("WinHTTP websocket coordinator stopped"))??;
    Ok(StreamingSocket {
        write: Box::new(ChannelWrite {
            tx: send_tx.clone(),
            state: Arc::clone(&state),
        }),
        read: Box::new(ChannelRead {
            tx: receive_tx,
            state,
        }),
    })
}

fn websocket_coordinator(
    handle: usize,
    state: Arc<SocketState>,
    cancel: Arc<AtomicBool>,
    send_rx: std::sync::mpsc::Receiver<SendCommand>,
    receive_rx: std::sync::mpsc::Receiver<ReceiveCommand>,
    close_rx: std::sync::mpsc::Receiver<CloseRequest>,
    ready_tx: std::sync::mpsc::SyncSender<Result<()>>,
) {
    let send_handle = handle;
    let receive_handle = handle;
    let send_join = std::thread::Builder::new()
        .name("winhttp-websocket-send".into())
        .spawn(move || {
            while let Ok(command) = send_rx.recv() {
                match command {
                    SendCommand::Send {
                        buffer_type,
                        data,
                        reply,
                    } => {
                        let result = unsafe {
                            WinHttpWebSocketSend(send_handle as _, buffer_type, Some(&data))
                        };
                        let result = winhttp_status("websocket send", result);
                        let _ = reply.send(result);
                    }
                    SendCommand::Stop { reply } => {
                        let _ = reply.send(());
                        break;
                    }
                }
            }
        });
    let send_join = match send_join {
        Ok(join) => join,
        Err(error) => {
            let _ = ready_tx.send(Err(anyhow!("start WinHTTP websocket send thread: {error}")));
            unsafe {
                let _ = WinHttpCloseHandle(handle as _);
            }
            return;
        }
    };
    let receive_cancel = Arc::clone(&cancel);
    let receive_join = std::thread::Builder::new()
        .name("winhttp-websocket-receive".into())
        .spawn(move || {
            while let Ok(command) = receive_rx.recv() {
                match command {
                    ReceiveCommand::Receive { reply } => {
                        let _ = reply.send(receive_message(receive_handle as _, &receive_cancel));
                    }
                    ReceiveCommand::Stop { reply } => {
                        let _ = reply.send(());
                        break;
                    }
                }
            }
        });
    let receive_join = match receive_join {
        Ok(join) => join,
        Err(error) => {
            cancel.store(true, Ordering::Release);
            let (done, result) = oneshot::channel();
            let _ = state.send_tx.send(SendCommand::Stop { reply: done });
            let _ = result.blocking_recv();
            let _ = send_join.join();
            let _ = ready_tx.send(Err(anyhow!(
                "start WinHTTP websocket receive thread: {error}"
            )));
            unsafe {
                let _ = WinHttpCloseHandle(handle as _);
            }
            return;
        }
    };
    let _ = ready_tx.send(Ok(()));
    let reply = match close_rx.recv() {
        Ok(CloseRequest(reply)) => reply,
        Err(_) => None,
    };
    state.closed.store(true, Ordering::Release);
    cancel.store(true, Ordering::Release);
    let (send_done, send_result) = oneshot::channel();
    let (receive_done, receive_result) = oneshot::channel();
    let _ = state.send_tx.send(SendCommand::Stop { reply: send_done });
    let _ = state.receive_tx.send(ReceiveCommand::Stop {
        reply: receive_done,
    });
    let _ = send_result.blocking_recv();
    let _ = receive_result.blocking_recv();
    let _ = send_join.join();
    let _ = receive_join.join();
    let result = unsafe {
        WinHttpWebSocketClose(
            handle as _,
            WINHTTP_WEB_SOCKET_SUCCESS_CLOSE_STATUS.0 as u16,
            None,
            0,
        )
    };
    let result = winhttp_status("websocket close", result);
    if let Some(reply) = reply {
        let _ = reply.send(result);
    }
    unsafe {
        let _ = WinHttpCloseHandle(handle as _);
    }
}

fn receive_message(
    handle: *mut std::ffi::c_void,
    cancelled: &AtomicBool,
) -> Result<Option<StreamingMessage>> {
    let mut data = Vec::new();
    let mut message_type = None;
    loop {
        if cancelled.load(Ordering::Acquire) {
            return Ok(None);
        }
        let mut chunk = vec![0; READ_SIZE];
        let mut read = 0;
        let mut kind = WINHTTP_WEB_SOCKET_BINARY_MESSAGE_BUFFER_TYPE;
        let status = unsafe {
            WinHttpWebSocketReceive(
                handle,
                chunk.as_mut_ptr().cast(),
                chunk.len() as u32,
                &mut read,
                &mut kind,
            )
        };
        if status != 0 {
            let error = WinError::from_win32();
            if error.code().0 == 12002 {
                if cancelled.load(Ordering::Acquire) {
                    return Ok(None);
                }
                continue;
            }
            return Err(win_error("websocket receive"));
        }
        let bytes = &chunk[..read as usize];
        // WinHTTP 0.60 exposes only message, fragment, and close buffer types;
        // protocol Ping/Pong frames are handled internally and never surface here.
        let (is_text, is_final) = match kind {
            WINHTTP_WEB_SOCKET_CLOSE_BUFFER_TYPE => return Ok(Some(StreamingMessage::Close)),
            WINHTTP_WEB_SOCKET_UTF8_MESSAGE_BUFFER_TYPE => (true, true),
            WINHTTP_WEB_SOCKET_BINARY_MESSAGE_BUFFER_TYPE => (false, true),
            WINHTTP_WEB_SOCKET_UTF8_FRAGMENT_BUFFER_TYPE => (true, false),
            WINHTTP_WEB_SOCKET_BINARY_FRAGMENT_BUFFER_TYPE => (false, false),
            _ => return Err(anyhow!("WinHTTP websocket received unknown frame type")),
        };
        if let Some(expected) = message_type {
            if expected != is_text {
                return Err(anyhow!(
                    "WinHTTP websocket received mixed text and binary fragments"
                ));
            }
        } else {
            message_type = Some(is_text);
        }
        data.extend_from_slice(bytes);
        if is_final {
            break;
        }
    }
    Ok(Some(if message_type.unwrap_or(false) {
        StreamingMessage::Text(
            String::from_utf8(data)
                .map_err(|_| anyhow!("WinHTTP websocket text message is not valid UTF-8"))?,
        )
    } else {
        StreamingMessage::Binary(data)
    }))
}

fn timeout_ms(timeout: Duration) -> i32 {
    timeout.as_millis().max(1).min(i32::MAX as u128) as i32
}

pub struct Response {
    pub status: u16,
    pub body: Vec<u8>,
}

struct RequestParts<'a> {
    host: &'a [u16],
    target: &'a [u16],
    method: &'a [u16],
    headers: &'a [u16],
    port: u16,
    secure: bool,
    body: &'a [u8],
    timeout: Duration,
}

pub fn request(
    method: &str,
    url: &str,
    headers: &[(String, String)],
    body: &[u8],
    timeout: Duration,
) -> Result<Response> {
    let url = url::Url::parse(url).map_err(|e| {
        let detail = crate::services::download::sanitize_message(&e.to_string());
        anyhow!("WinHTTP URL: {detail}")
    })?;
    let host = url
        .host_str()
        .ok_or_else(|| anyhow!("WinHTTP URL has no host"))?;
    let target = format!(
        "{}{}",
        url.path(),
        url.query().map(|q| format!("?{q}")).unwrap_or_default()
    );
    let host = wide(host);
    let target = wide(&target);
    let method = wide(method);
    let header_text = headers
        .iter()
        .try_fold(String::new(), |mut block, (name, value)| {
            crate::transcription::cloud::http::validate_header(name, value)?;
            block.push_str(name);
            block.push_str(": ");
            block.push_str(value);
            block.push_str("\r\n");
            Ok::<_, anyhow::Error>(block)
        })?;
    let header_text = wide(&header_text);
    unsafe {
        request_inner(&RequestParts {
            host: &host,
            target: &target,
            method: &method,
            headers: &header_text,
            port: url.port_or_known_default().unwrap_or(443),
            secure: url.scheme().eq_ignore_ascii_case("https"),
            body,
            timeout,
        })
    }
}

unsafe fn request_inner(parts: &RequestParts<'_>) -> Result<Response> {
    let session = WinHttpOpen(
        PCWSTR::null(),
        WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY,
        PCWSTR::null(),
        PCWSTR::null(),
        0,
    );
    if session.is_null() {
        return Err(win_error("open session"));
    }
    let session = HandleGuard(session);
    let timeout_ms = parts
        .timeout
        .as_millis()
        .max((!parts.timeout.is_zero()) as u128)
        .min(i32::MAX as u128) as i32;
    WinHttpSetTimeouts(
        session.0,
        CONNECT_TIMEOUT_MS,
        timeout_ms,
        timeout_ms,
        timeout_ms,
    )?;
    let policy = WINHTTP_AUTOLOGON_SECURITY_LEVEL_MEDIUM;
    WinHttpSetOption(
        Some(session.0.cast_const()),
        WINHTTP_OPTION_AUTOLOGON_POLICY,
        Some(std::slice::from_raw_parts(
            (&policy as *const _) as *const u8,
            std::mem::size_of_val(&policy),
        )),
    )?;
    let connection = WinHttpConnect(session.0, PCWSTR(parts.host.as_ptr()), parts.port, 0);
    if connection.is_null() {
        return Err(win_error("connect"));
    }
    let connection = HandleGuard(connection);
    let flags = if parts.secure {
        WINHTTP_FLAG_SECURE
    } else {
        WINHTTP_OPEN_REQUEST_FLAGS(0)
    };
    let request = WinHttpOpenRequest(
        connection.0,
        PCWSTR(parts.method.as_ptr()),
        PCWSTR(parts.target.as_ptr()),
        PCWSTR::null(),
        PCWSTR::null(),
        std::ptr::null(),
        flags,
    );
    if request.is_null() {
        return Err(win_error("open request"));
    }
    let request = HandleGuard(request);
    let body_len = u32::try_from(parts.body.len())
        .map_err(|_| anyhow!("WinHTTP request body is too large (maximum 4 GiB)"))?;
    let header_ptr =
        (!parts.headers.is_empty()).then_some(&parts.headers[..parts.headers.len() - 1]);
    WinHttpSendRequest(
        request.0,
        header_ptr,
        (!parts.body.is_empty()).then_some(parts.body.as_ptr().cast()),
        body_len,
        body_len,
        0,
    )?;
    WinHttpReceiveResponse(request.0, std::ptr::null_mut())?;
    let status = query_status(request.0)? as u16;
    let mut result = Vec::new();
    let mut buffer = [0u8; READ_SIZE];
    loop {
        let mut read = 0u32;
        WinHttpReadData(
            request.0,
            buffer.as_mut_ptr().cast(),
            buffer.len() as u32,
            &mut read,
        )?;
        if read == 0 {
            break;
        }
        result.extend_from_slice(&buffer[..read as usize]);
    }
    Ok(Response {
        status,
        body: result,
    })
}

unsafe fn query_status(request: *mut std::ffi::c_void) -> Result<u32> {
    let mut status = 0u32;
    let mut size = std::mem::size_of::<u32>() as u32;
    WinHttpQueryHeaders(
        request,
        WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
        PCWSTR::null(),
        Some((&mut status as *mut u32).cast()),
        &mut size,
        std::ptr::null_mut(),
    )?;
    Ok(status)
}

fn win_error(phase: &str) -> anyhow::Error {
    let error = WinError::from_win32();
    if error.code().0 == 12002 {
        return anyhow!("WinHTTP {phase} timed out");
    }
    let detail = crate::services::download::sanitize_message(&error.to_string());
    anyhow!("WinHTTP {phase} failed: {detail}")
}

fn winhttp_status(phase: &str, status: u32) -> Result<()> {
    if status == 0 {
        Ok(())
    } else if status == 12002 {
        Err(anyhow!("WinHTTP {phase} timed out"))
    } else {
        let detail = crate::services::download::sanitize_message(&format!("status {status}"));
        Err(anyhow!("WinHTTP {phase} failed with {detail}"))
    }
}

struct HandleGuard(*mut std::ffi::c_void);
impl Drop for HandleGuard {
    fn drop(&mut self) {
        unsafe {
            let _ = WinHttpCloseHandle(self.0);
        }
    }
}
fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}

fn is_filtered_header(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "connection" | "upgrade" | "sec-websocket-key" | "sec-websocket-version" | "host"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filters_websocket_handshake_headers_case_insensitively() {
        assert!(is_filtered_header("Upgrade"));
        assert!(is_filtered_header("SEC-WebSocket-Key"));
        assert!(!is_filtered_header("Authorization"));
    }
}
