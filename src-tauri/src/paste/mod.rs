// Paste : injection de texte au curseur via simulation Ctrl+V.
//
// Reference VoiceInk :
// - VoiceInk/CursorPaster.swift (sequence CGEvent Cmd+V, fallback AppleScript
//   pour layouts non-QWERTY)
// - VoiceInk/ClipboardManager.swift (set/restore clipboard avec backup)
//
// Windows equivalents :
// - CGEvent -> SendInput (Win32_UI_Input_KeyboardAndMouse)
// - AppleScript layout workaround -> KEYEVENTF_SCANCODE pour envoyer des
//   scancodes hardware independants du layout (ex. AZERTY, DVORAK, Neo2).
// - NSPasteboard backup -> EnumClipboardFormats + GetClipboardData par format
//   (implemente dans le module clipboard_backup). Preserve images, fichiers
//   et autres formats quand l'utilisateur avait copie autre chose avant la
//   dictee.

use std::sync::{Mutex as StdMutex, OnceLock};
use std::thread::sleep;
use std::time::Duration;

use anyhow::{anyhow, Result};
use arboard::Clipboard;
use parking_lot::Mutex;
use tracing::{debug, warn};

#[cfg(windows)]
mod clipboard_backup;

/// Delai apres le set clipboard avant d'envoyer Ctrl+V (VoiceInk L29 : 0.05 s).
const CLIPBOARD_TO_PASTE_DELAY: Duration = Duration::from_millis(50);
/// Delai minimum avant restore du clipboard (VoiceInk L39 : max(setting, 0.25)).
const MIN_RESTORE_DELAY: Duration = Duration::from_millis(250);

/// HWND de la fenetre cible (l'app sous le curseur) sauvegarde au moment
/// du start de l'enregistrement. Restaure juste avant le SendInput Ctrl+V
/// pour que ce soit l'app cible qui recoit le coup, pas la mini-recorder
/// Parla.
#[cfg(windows)]
static SAVED_FOREGROUND: OnceLock<StdMutex<isize>> = OnceLock::new();

#[cfg(windows)]
pub fn remember_foreground() {
    use windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow;
    let hwnd = unsafe { GetForegroundWindow() };
    let raw = hwnd.0 as isize;
    if raw == 0 {
        return;
    }
    let cell = SAVED_FOREGROUND.get_or_init(|| StdMutex::new(0));
    if let Ok(mut g) = cell.lock() {
        *g = raw;
    }
}

#[cfg(not(windows))]
pub fn remember_foreground() {}

fn clipboard() -> &'static Mutex<Clipboard> {
    static CLIP: OnceLock<Mutex<Clipboard>> = OnceLock::new();
    CLIP.get_or_init(|| Mutex::new(Clipboard::new().expect("clipboard init")))
}

/// Copy text to the system clipboard without pasting anything. Used by the
/// tray menu ("Copy last transcription") and any UI action that only wants
/// to populate the clipboard.
pub fn copy_to_clipboard(text: &str) -> Result<()> {
    clipboard()
        .lock()
        .set_text(text.to_string())
        .map_err(|e| anyhow!("set clipboard: {e}"))
}

/// Backup le contenu actuel du clipboard (tous formats sur Windows), pose
/// `text`, envoie Ctrl+V, puis restore le clipboard complet.
///
/// Si `restore` est false (VoiceInk `restoreClipboardAfterPaste` = off), on
/// laisse juste le texte injecte dans le clipboard.
///
/// Windows : utilise clipboard_backup::backup_all + restore_all pour preserver
/// les images, fichiers, html, rtf, etc. que l'utilisateur aurait eu dans
/// son clipboard avant la dictee. Sur les autres plateformes, fallback texte
/// uniquement via arboard.
pub fn paste_at_cursor(text: &str, restore: bool, restore_delay: Option<Duration>) -> Result<()> {
    #[cfg(windows)]
    let backup = if restore {
        match clipboard_backup::backup_all() {
            Ok(b) => Some(b),
            Err(e) => {
                warn!("clipboard backup: {e}, fallback texte uniquement");
                clipboard()
                    .lock()
                    .get_text()
                    .ok()
                    .map(clipboard_backup::Backup::text_only)
            }
        }
    } else {
        None
    };

    #[cfg(not(windows))]
    let backup = if restore {
        clipboard().lock().get_text().ok()
    } else {
        None
    };

    clipboard()
        .lock()
        .set_text(text.to_string())
        .map_err(|e| anyhow!("set clipboard: {e}"))?;

    // Laisse le temps a l'OS de propager le set avant d'envoyer Ctrl+V.
    sleep(CLIPBOARD_TO_PASTE_DELAY);

    // Re-donne le focus a l'app cible que l'utilisateur etait en train
    // d'utiliser quand il a declenche le hotkey. Sinon Ctrl+V part dans
    // la fenetre Parla mini-recorder.
    #[cfg(windows)]
    restore_foreground();

    send_ctrl_v()?;

    if let Some(prev) = backup {
        let delay = restore_delay
            .unwrap_or(MIN_RESTORE_DELAY)
            .max(MIN_RESTORE_DELAY);
        sleep(delay);
        #[cfg(windows)]
        {
            if let Err(e) = clipboard_backup::restore_all(&prev) {
                warn!("restore clipboard multi-format: {e}");
            }
        }
        #[cfg(not(windows))]
        {
            if let Err(e) = clipboard().lock().set_text(prev) {
                warn!("restore clipboard: {e}");
            }
        }
    }

    Ok(())
}

#[cfg(windows)]
fn send_ctrl_v() -> Result<()> {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        MapVirtualKeyW, SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS,
        KEYEVENTF_KEYUP, KEYEVENTF_SCANCODE, MAPVK_VK_TO_VSC, VK_CONTROL, VK_V,
    };

    // Scancodes via MapVirtualKey pour etre insensible au layout clavier.
    // Cf CursorPaster L55-73 : sur mac l'AppleScript contourne les problemes
    // de layouts non-QWERTY, sur Windows on evite carrement en envoyant le
    // scancode hardware direct.
    let ctrl_sc = unsafe { MapVirtualKeyW(VK_CONTROL.0 as u32, MAPVK_VK_TO_VSC) } as u16;
    let v_sc = unsafe { MapVirtualKeyW(VK_V.0 as u32, MAPVK_VK_TO_VSC) } as u16;

    fn ki(scan: u16, flags: KEYBD_EVENT_FLAGS) -> INPUT {
        INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: windows::Win32::UI::Input::KeyboardAndMouse::VIRTUAL_KEY(0),
                    wScan: scan,
                    dwFlags: flags,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        }
    }

    let inputs = [
        ki(ctrl_sc, KEYEVENTF_SCANCODE),
        ki(v_sc, KEYEVENTF_SCANCODE),
        ki(v_sc, KEYEVENTF_SCANCODE | KEYEVENTF_KEYUP),
        ki(ctrl_sc, KEYEVENTF_SCANCODE | KEYEVENTF_KEYUP),
    ];

    let sent = unsafe { SendInput(&inputs, std::mem::size_of::<INPUT>() as i32) };
    if sent as usize != inputs.len() {
        return Err(anyhow!("SendInput a envoye {sent}/{} events", inputs.len()));
    }
    debug!("Ctrl+V envoye via SendInput (scancode)");
    Ok(())
}

#[cfg(not(windows))]
fn send_ctrl_v() -> Result<()> {
    Err(anyhow!("paste non implemente sur cette plateforme"))
}

#[cfg(windows)]
fn restore_foreground() {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{IsWindow, SetForegroundWindow};
    let Some(cell) = SAVED_FOREGROUND.get() else {
        return;
    };
    let Ok(g) = cell.lock() else {
        return;
    };
    if *g == 0 {
        return;
    }
    let hwnd = HWND(*g as *mut _);
    unsafe {
        if !IsWindow(Some(hwnd)).as_bool() {
            return;
        }
        let _ = SetForegroundWindow(hwnd);
    }
}
