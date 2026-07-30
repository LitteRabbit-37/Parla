// Detection de la fenetre active sur Windows.
//
// Reference VoiceInk : PowerMode/ActiveWindowService.swift utilise
// NSWorkspace.frontmostApplication + bundleIdentifier. Sur Windows on passe
// par GetForegroundWindow -> GetWindowThreadProcessId -> QueryFullProcessImageNameW
// pour obtenir le chemin de l'exe et par GetWindowTextW pour le titre.
//
// "bundle id" equivalent = nom de l'exe (sans extension), car on ne veut pas
// dependre du chemin absolu.

use std::path::PathBuf;

use anyhow::{anyhow, Result};
use windows::core::PWSTR;
use windows::Win32::Foundation::{CloseHandle, HWND, MAX_PATH};
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, GetWindowTextLengthW, GetWindowTextW, GetWindowThreadProcessId,
};

#[derive(Debug, Clone)]
pub struct ActiveWindow {
    pub hwnd: isize,
    pub pid: u32,
    pub title: String,
    #[allow(dead_code)]
    pub exe_path: PathBuf,
    /// Nom de l'exe sans extension, en minuscules (ex "chrome", "msedge").
    pub exe_name: String,
}

pub fn foreground_window() -> Result<ActiveWindow> {
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.0.is_null() {
            return Err(anyhow!("aucune fenetre au premier plan"));
        }

        let title = window_title(hwnd);

        let mut pid = 0u32;
        let _tid = GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid == 0 {
            return Err(anyhow!("pid introuvable"));
        }

        let exe_path = process_image_path(pid)?;
        let exe_name = exe_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();

        Ok(ActiveWindow {
            hwnd: hwnd.0 as isize,
            pid,
            title,
            exe_path,
            exe_name,
        })
    }
}

unsafe fn window_title(hwnd: HWND) -> String {
    let len = GetWindowTextLengthW(hwnd);
    if len <= 0 {
        return String::new();
    }
    let mut buf = vec![0u16; (len as usize) + 1];
    let written = GetWindowTextW(hwnd, &mut buf);
    if written <= 0 {
        return String::new();
    }
    String::from_utf16_lossy(&buf[..written as usize])
}

unsafe fn process_image_path(pid: u32) -> Result<PathBuf> {
    let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid)
        .map_err(|e| anyhow!("OpenProcess pid {pid}: {e}"))?;

    let mut buf = vec![0u16; MAX_PATH as usize];
    let mut size = buf.len() as u32;
    let res = QueryFullProcessImageNameW(
        handle,
        PROCESS_NAME_WIN32,
        PWSTR(buf.as_mut_ptr()),
        &mut size,
    );
    let _ = CloseHandle(handle);
    res.map_err(|e| anyhow!("QueryFullProcessImageNameW: {e}"))?;

    let slice = &buf[..size as usize];
    Ok(PathBuf::from(String::from_utf16_lossy(slice)))
}

/// Liste courte des exe de navigateurs supportes. Utilise pour savoir quand
/// declencher l'extraction d'URL.
pub const BROWSER_EXES: &[&str] = &[
    "chrome", "msedge", "brave", "vivaldi", "opera", "firefox", "arc", "zen",
    "browser", // Safari sous Windows n'existe plus, mais Yandex / autres.
];

pub fn is_browser(active: &ActiveWindow) -> bool {
    BROWSER_EXES.iter().any(|e| *e == active.exe_name)
}
