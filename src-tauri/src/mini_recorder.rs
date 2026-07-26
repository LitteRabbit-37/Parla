// Mini-recorder window : panel flottant non-activating bottom-center OU top-center.
//
// Reference VoiceInk :
// - MiniRecorderPanel.swift : NSPanel nonactivatingPanel floating,
//   canBecomeKey=false, bottom-center padding 24 px, 300 x 120.
// - NotchRecorderPanel.swift : variante top-center collant au notch
//   MacBook Pro, position y = screen.maxY - 200, shape pill noir
//   (radius top-flat / bottom-rounded), blur material .dark.
// - RecorderStyle : UserDefaults key "RecorderType" ∈ {"mini", "notch"},
//   par defaut "mini".
//
// Sur Windows on obtient le meme effet via une WebviewWindow Tauri avec
// decorations=false, always_on_top=true, skip_taskbar=true, resizable=false,
// focused=false, transparent, shadow=false.

use tauri::{AppHandle, LogicalPosition, LogicalSize, Manager, WebviewUrl, WebviewWindowBuilder};
use tauri_plugin_store::StoreExt;
use tracing::{debug, warn};

pub const LABEL: &str = "recorder";

const STORE_FILE: &str = "parla.settings.json";
const KEY_STYLE: &str = "recorder_style";

/// Dimensions VoiceInk MiniRecorderPanel L40-42.
const WIDTH: f64 = 300.0;
const HEIGHT: f64 = 120.0;
const BOTTOM_PADDING: f64 = 24.0;
/// Position y du Notch recorder depuis le haut. VoiceInk colle au notch
/// (y=screen.maxY-200). Sur Windows, sans notch, on descend de 0 px du
/// haut pour que le pill sorte de l'ecran.
const TOP_PADDING: f64 = 0.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecorderStyle {
    Mini,
    Notch,
}

impl RecorderStyle {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Mini => "mini",
            Self::Notch => "notch",
        }
    }
    pub fn parse(s: &str) -> Self {
        if s.eq_ignore_ascii_case("notch") {
            Self::Notch
        } else {
            Self::Mini
        }
    }
}

pub fn get_style(app: &AppHandle) -> RecorderStyle {
    app.store(STORE_FILE)
        .ok()
        .and_then(|s| s.get(KEY_STYLE).and_then(|v| v.as_str().map(String::from)))
        .map(|s| RecorderStyle::parse(&s))
        .unwrap_or(RecorderStyle::Mini)
}

pub fn set_style(app: &AppHandle, style: RecorderStyle) -> anyhow::Result<()> {
    let store = app
        .store(STORE_FILE)
        .map_err(|e| anyhow::anyhow!("store: {e}"))?;
    store.set(KEY_STYLE, serde_json::Value::String(style.as_str().into()));
    store.save().map_err(|e| anyhow::anyhow!("store save: {e}"))
}

/// Cree la fenetre recorder si elle n'existe pas, sinon la ramene au premier plan.
pub fn ensure_open(app: &AppHandle) {
    let style = get_style(app);
    if let Some(existing) = app.get_webview_window(LABEL) {
        reposition(&existing, style);
        let _ = existing.show();
        return;
    }

    // URL frontend : meme bundle, route hash pour detecter la vue.
    // WebviewUrl::App est resolu par Tauri vers le serveur dev en mode dev
    // (http://localhost:1420/...) et vers les assets bundle en release.
    let url = WebviewUrl::App("index.html#recorder".into());

    let builder = WebviewWindowBuilder::new(app, LABEL, url)
        .title("Parla Recorder")
        .inner_size(WIDTH, HEIGHT)
        .resizable(false)
        .decorations(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .focused(false)
        .shadow(false)
        .transparent(true);

    let window = match builder.build() {
        Ok(w) => w,
        Err(e) => {
            warn!("Impossible de creer la fenetre recorder: {e}");
            return;
        }
    };

    apply_no_activate_style(&window);
    reposition(&window, style);
    debug!(style = style.as_str(), "Fenetre recorder creee");
}

/// Ajoute les styles Windows WS_EX_NOACTIVATE + WS_EX_TOOLWINDOW pour que
/// la fenetre ne prenne jamais le focus, meme quand on clique dessus.
/// Equivalent du nonactivatingPanel macOS utilise par VoiceInk
/// (MiniRecorderPanel.swift L18-31). Sans ce flag Windows, ouvrir la pill
/// vole brievement le focus a l'app cible (Notepad/VS Code) et casse le
/// paste Ctrl+V au stop.
fn apply_no_activate_style(window: &tauri::WebviewWindow) {
    #[cfg(windows)]
    {
        use windows::Win32::Foundation::HWND;
        use windows::Win32::UI::WindowsAndMessaging::{
            GetWindowLongPtrW, SetWindowLongPtrW, GWL_EXSTYLE, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
        };
        let Ok(raw_hwnd) = window.hwnd() else {
            warn!("hwnd() recorder: indisponible");
            return;
        };
        let hwnd = HWND(raw_hwnd.0 as *mut _);
        unsafe {
            let cur = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
            let extra = (WS_EX_NOACTIVATE.0 | WS_EX_TOOLWINDOW.0) as isize;
            SetWindowLongPtrW(hwnd, GWL_EXSTYLE, cur | extra);
        }
    }
    #[cfg(not(windows))]
    {
        let _ = window;
    }
}

fn reposition(window: &tauri::WebviewWindow, style: RecorderStyle) {
    reposition_with_height(window, style, HEIGHT);
}

/// Resize the recorder window while keeping the pill anchored to its
/// original edge (mini = bottom, notch = top). When a dropdown popover
/// needs to render inside the webview (Radix UI), we call this with a
/// taller height so Radix has room to place the content upward. The
/// transparent background makes the new space invisible. We restore
/// HEIGHT when the popover closes.
fn reposition_with_height(window: &tauri::WebviewWindow, style: RecorderStyle, height: f64) {
    if let Ok(Some(monitor)) = window.primary_monitor() {
        let size = monitor.size();
        let scale = monitor.scale_factor();
        let work_w = size.width as f64 / scale;
        let work_h = size.height as f64 / scale;
        let x = (work_w - WIDTH) / 2.0;
        let y = match style {
            RecorderStyle::Mini => work_h - height - BOTTOM_PADDING,
            RecorderStyle::Notch => TOP_PADDING,
        };
        let _ = window.set_position(LogicalPosition::new(x, y));
        let _ = window.set_size(LogicalSize::new(WIDTH, height));
    }
}

/// Tauri command bound in lib.rs. Called from the frontend when a
/// dropdown opens or closes inside the mini recorder. Keeps the pill
/// anchored to its style-specific edge (bottom for mini, top for notch).
#[tauri::command]
pub fn resize_recorder_window(app: AppHandle, height: f64) {
    let Some(win) = app.get_webview_window(LABEL) else {
        return;
    };
    let clamped = height.clamp(HEIGHT, 600.0);
    let style = get_style(&app);
    reposition_with_height(&win, style, clamped);
}

/// Show and focus the main window, optionally navigating to a panel.
/// Callable from any webview (e.g. the mini recorder's Power Mode empty
/// state link). Panel id matches the frontend `View` union.
#[tauri::command]
pub fn show_main_window(app: AppHandle, panel: Option<String>) {
    use tauri::Emitter;
    if let Some(win) = app.get_webview_window("main") {
        let _ = win.show();
        let _ = win.unminimize();
        let _ = win.set_focus();
    }
    if let Some(p) = panel {
        let _ = app.emit("tray:navigate", p);
    }
}

pub fn close(app: &AppHandle) {
    if let Some(win) = app.get_webview_window(LABEL) {
        let _ = win.close();
    }
}
