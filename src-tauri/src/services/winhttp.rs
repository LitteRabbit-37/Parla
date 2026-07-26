//! Small synchronous WinHTTP transport used when Windows owns proxy routing.

use anyhow::{anyhow, Result};
use std::time::Duration;
use windows::core::{Error as WinError, PCWSTR};
use windows::Win32::Networking::WinHttp::*;

const CONNECT_TIMEOUT_MS: i32 = 15_000;
const READ_SIZE: usize = 64 * 1024;

pub struct Response {
    pub status: u16,
    pub body: Vec<u8>,
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
        request_inner(
            &host,
            &target,
            &method,
            &header_text,
            url.port_or_known_default().unwrap_or(443),
            url.scheme().eq_ignore_ascii_case("https"),
            body,
            timeout,
        )
    }
}

unsafe fn request_inner(
    host: &[u16],
    target: &[u16],
    method: &[u16],
    headers: &[u16],
    port: u16,
    secure: bool,
    body: &[u8],
    timeout: Duration,
) -> Result<Response> {
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
    let timeout_ms = timeout
        .as_millis()
        .max((!timeout.is_zero()) as u128)
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
    let connection = WinHttpConnect(session.0, PCWSTR(host.as_ptr()), port, 0);
    if connection.is_null() {
        return Err(win_error("connect"));
    }
    let connection = HandleGuard(connection);
    let flags = if secure {
        WINHTTP_FLAG_SECURE
    } else {
        WINHTTP_OPEN_REQUEST_FLAGS(0)
    };
    let request = WinHttpOpenRequest(
        connection.0,
        PCWSTR(method.as_ptr()),
        PCWSTR(target.as_ptr()),
        PCWSTR::null(),
        PCWSTR::null(),
        std::ptr::null(),
        flags,
    );
    if request.is_null() {
        return Err(win_error("open request"));
    }
    let request = HandleGuard(request);
    let body_len = u32::try_from(body.len())
        .map_err(|_| anyhow!("WinHTTP request body is too large (maximum 4 GiB)"))?;
    let header_ptr = (!headers.is_empty()).then_some(&headers[..headers.len() - 1]);
    WinHttpSendRequest(
        request.0,
        header_ptr,
        (!body.is_empty()).then_some(body.as_ptr().cast()),
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
    let detail = crate::services::download::sanitize_message(&error.to_string());
    anyhow!("WinHTTP {phase} failed: {detail}")
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
