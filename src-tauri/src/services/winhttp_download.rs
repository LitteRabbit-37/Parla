//! Synchronous Windows-native model downloads through system proxy settings.

use anyhow::{anyhow, Result};
use tracing::{debug, info, warn};
use url::Url;
use windows::core::{Error as WinError, PCWSTR};
use windows::Win32::Networking::WinHttp::*;

const IO_TIMEOUT_MS: i32 = 30_000;
const READ_SIZE: usize = 64 * 1024;

/// Download response body using WinHTTP automatic proxy transport.
/// `on_chunk` runs after each successful body read. Returning false aborts
/// without closing handles from another thread.
pub fn download<F>(url: &Url, expected_size: u64, mut on_chunk: F) -> Result<u64>
where
    F: FnMut(&[u8], u64, u64) -> bool,
{
    let host = url
        .host_str()
        .ok_or_else(|| anyhow!("WinHTTP target has no host"))?;
    let port = url.port_or_known_default().unwrap_or(443);
    let secure = url.scheme().eq_ignore_ascii_case("https");
    let target = format!(
        "{}{}",
        url.path(),
        url.query()
            .map(|query| format!("?{query}"))
            .unwrap_or_default()
    );
    let host_wide = wide(host);
    let target_wide = wide(&target);
    let verb_wide = wide("GET");
    let started = std::time::Instant::now();
    info!(
        host,
        port,
        scheme = url.scheme(),
        "WinHTTP system download started"
    );

    let result = unsafe {
        download_inner(
            &host_wide,
            &target_wide,
            &verb_wide,
            port,
            secure,
            expected_size,
            &mut on_chunk,
        )
    };
    match &result {
        Ok(bytes) => info!(
            host,
            port,
            bytes,
            elapsed_ms = started.elapsed().as_millis() as u64,
            "WinHTTP system download completed"
        ),
        Err(error) => {
            let diagnostic = crate::services::download::sanitize_message(&error.to_string());
            warn!(host, port, elapsed_ms = started.elapsed().as_millis() as u64, error = %diagnostic, "WinHTTP system download failed")
        }
    }
    result
}

unsafe fn download_inner<F>(
    host: &[u16],
    target: &[u16],
    verb: &[u16],
    port: u16,
    secure: bool,
    expected_size: u64,
    on_chunk: &mut F,
) -> Result<u64>
where
    F: FnMut(&[u8], u64, u64) -> bool,
{
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
    WinHttpSetTimeouts(
        session.0,
        IO_TIMEOUT_MS,
        IO_TIMEOUT_MS,
        IO_TIMEOUT_MS,
        IO_TIMEOUT_MS,
    )?;

    let policy = WINHTTP_AUTOLOGON_SECURITY_LEVEL_MEDIUM;
    WinHttpSetOption(
        Some(session.0.cast_const()),
        WINHTTP_OPTION_AUTOLOGON_POLICY,
        Some(std::slice::from_raw_parts(
            &policy as *const _ as *const u8,
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
        PCWSTR(verb.as_ptr()),
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
    WinHttpSendRequest(request.0, None, None, 0, 0, 0)?;
    WinHttpReceiveResponse(request.0, std::ptr::null_mut())?;

    let status = query_status(request.0)?;
    info!(status, "WinHTTP system download response received");
    if !(200..300).contains(&status) {
        return Err(anyhow!("WinHTTP HTTP status {status}"));
    }

    let mut buffer = [0u8; READ_SIZE];
    let mut bytes_read_total = 0u64;
    let total = expected_size;
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
        let chunk = &buffer[..read as usize];
        bytes_read_total += chunk.len() as u64;
        debug!(
            bytes = bytes_read_total,
            "WinHTTP system download body activity"
        );
        if !on_chunk(chunk, bytes_read_total, total) {
            return Err(anyhow!("telechargement annule"));
        }
    }
    Ok(bytes_read_total)
}

unsafe fn query_status(request: *mut std::ffi::c_void) -> Result<u32> {
    let mut status = 0u32;
    let mut size = std::mem::size_of::<u32>() as u32;
    WinHttpQueryHeaders(
        request,
        WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
        PCWSTR::null(),
        Some(&mut status as *mut _ as *mut _),
        &mut size,
        std::ptr::null_mut(),
    )?;
    Ok(status)
}

fn win_error(phase: &str) -> anyhow::Error {
    anyhow!("WinHTTP {phase} failed: {}", WinError::from_win32())
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
