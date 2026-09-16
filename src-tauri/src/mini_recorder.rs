// Mini-recorder window : panel flottant non-activating bottom-center OU top-center,
// plus la fenetre popover du bouton Mode.
//
// Reference VoiceInk :
// - Features/Recording/Presentation/MiniRecorderPanel.swift : NSPanel
//   nonactivatingPanel floating, canBecomeKey=false, bottom-center padding
//   24 px, 300 x 120.
// - Features/Recording/Presentation/NotchRecorderPanel.swift : variante
//   top-center collant au notch MacBook Pro, y = screen.maxY - 200, pill
//   noir (radius top-flat / bottom-rounded), blur material .dark.
// - RecorderPanelStyle : UserDefaults "RecorderType" ∈ {"mini", "notch"},
//   par defaut "mini".
// - Features/Recording/Components/RecorderComponents.swift
//   RecorderModeButton : `.popover(isPresented:arrowEdge: .bottom)` =
//   NSPopover, donc une fenetre separee de 180 pt de large (340 max) ancree
//   sur le bouton, ouverte au survol / clic.
//
// Sur Windows on obtient le meme effet via des WebviewWindow Tauri avec
// decorations=false, always_on_top=true, skip_taskbar=true, resizable=false,
// focused=false, transparent, shadow=false, plus WS_EX_NOACTIVATE. Le
// popover est une seconde fenetre ("recorder-popover"), pre-creee cachee
// avec la bulle pour que son webview soit deja charge quand l'utilisateur
// survole le bouton, puis simplement montree / cachee. Auparavant le
// popover etait rendu DANS la fenetre de la bulle, qu'il fallait agrandir
// puis retrecir a chaque ouverture : c'etait la source des sauts visuels.

use tauri::{AppHandle, LogicalPosition, LogicalSize, Manager, WebviewUrl, WebviewWindowBuilder};
use tauri_plugin_store::StoreExt;
use tracing::{debug, warn};

pub const LABEL: &str = "recorder";
pub const POPOVER_LABEL: &str = "recorder-popover";

const STORE_FILE: &str = "parla.settings.json";
const KEY_STYLE: &str = "recorder_style";

/// Dimensions VoiceInk MiniRecorderPanel.
const WIDTH: f64 = 300.0;
const HEIGHT: f64 = 120.0;
const BOTTOM_PADDING: f64 = 24.0;
/// Position y du Notch recorder depuis le haut. VoiceInk colle au notch
/// (y=screen.maxY-200). Sur Windows, sans notch, on descend de 0 px du
/// haut pour que le pill sorte de l'ecran.
const TOP_PADDING: f64 = 0.0;

/// Fenetre popover du bouton Mode (VoiceInk ModePopover : 180 pt de large,
/// 340 max de haut ; un peu plus large ici pour les badges Alt+N).
const POPOVER_WIDTH: f64 = 240.0;
const POPOVER_HEIGHT: f64 = 320.0;
/// Espace entre la bulle et le popover (fleche NSPopover).
const POPOVER_GAP: f64 = 8.0;

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
    store
        .save()
        .map_err(|e| anyhow::anyhow!("store save: {e}"))
}

/// Cree la fenetre recorder si elle n'existe pas, sinon la ramene au premier plan.
/// Pre-cree aussi la fenetre popover (cachee).
pub fn ensure_open(app: &AppHandle) {
    let style = get_style(app);
    if let Some(existing) = app.get_webview_window(LABEL) {
        reposition(&existing, style);
        let _ = existing.show();
        ensure_popover_window(app);
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
    ensure_popover_window(app);
}

/// Cree la fenetre popover cachee si elle n'existe pas. Elle vit tant que
/// la bulle vit (fermee dans `close`).
fn ensure_popover_window(app: &AppHandle) -> Option<tauri::WebviewWindow> {
    if let Some(existing) = app.get_webview_window(POPOVER_LABEL) {
        return Some(existing);
    }
    let url = WebviewUrl::App("index.html#recorder-popover".into());
    let builder = WebviewWindowBuilder::new(app, POPOVER_LABEL, url)
        .title("Parla Recorder Popover")
        .inner_size(POPOVER_WIDTH, POPOVER_HEIGHT)
        .resizable(false)
        .decorations(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .focused(false)
        .shadow(false)
        .transparent(true)
        .visible(false);
    match builder.build() {
        Ok(w) => {
            apply_no_activate_style(&w);
            debug!("Fenetre popover recorder creee (cachee)");
            Some(w)
        }
        Err(e) => {
            warn!("Impossible de creer la fenetre popover: {e}");
            None
        }
    }
}

/// Ajoute les styles Windows WS_EX_NOACTIVATE + WS_EX_TOOLWINDOW pour que
/// la fenetre ne prenne jamais le focus, meme quand on clique dessus.
/// Equivalent du nonactivatingPanel macOS utilise par VoiceInk
/// (MiniRecorderPanel.swift). Sans ce flag Windows, ouvrir la pill
/// vole brievement le focus a l'app cible (Notepad/VS Code) et casse le
/// paste Ctrl+V au stop.
fn apply_no_activate_style(window: &tauri::WebviewWindow) {
    #[cfg(windows)]
    {
        use windows::Win32::Foundation::HWND;
        use windows::Win32::UI::WindowsAndMessaging::{
            GetWindowLongPtrW, SetWindowLongPtrW, GWL_EXSTYLE, WS_EX_NOACTIVATE,
            WS_EX_TOOLWINDOW,
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
    if let Ok(Some(monitor)) = window.primary_monitor() {
        let size = monitor.size();
        let scale = monitor.scale_factor();
        let work_w = size.width as f64 / scale;
        let work_h = size.height as f64 / scale;
        let x = (work_w - WIDTH) / 2.0;
        let y = match style {
            RecorderStyle::Mini => work_h - HEIGHT - BOTTOM_PADDING,
            RecorderStyle::Notch => TOP_PADDING,
        };
        let _ = window.set_position(LogicalPosition::new(x, y));
        let _ = window.set_size(LogicalSize::new(WIDTH, HEIGHT));
    }
}

/// Montre le popover du bouton Mode, ancre sur la bulle. Les ancres sont en
/// pixels logiques relatifs a la fenetre recorder : `anchor_x` = centre du
/// bouton, `anchor_top` / `anchor_bottom` = bords de la pilule. Mini : le
/// popover se pose au-dessus de la pilule ; notch : en dessous.
#[tauri::command]
pub fn open_recorder_popover(app: AppHandle, anchor_x: f64, anchor_top: f64, anchor_bottom: f64) {
    let Some(recorder) = app.get_webview_window(LABEL) else {
        return;
    };
    let Some(popover) = ensure_popover_window(&app) else {
        return;
    };
    let scale = recorder.scale_factor().unwrap_or(1.0);
    let Ok(pos) = recorder.outer_position() else {
        return;
    };
    let rec_x = pos.x as f64 / scale;
    let rec_y = pos.y as f64 / scale;

    let mut x = rec_x + anchor_x - POPOVER_WIDTH / 2.0;
    let y = match get_style(&app) {
        RecorderStyle::Mini => rec_y + anchor_top - POPOVER_GAP - POPOVER_HEIGHT,
        RecorderStyle::Notch => rec_y + anchor_bottom + POPOVER_GAP,
    };
    // Garde le popover dans l'ecran de la bulle.
    if let Ok(Some(monitor)) = recorder.current_monitor() {
        let mscale = monitor.scale_factor();
        let mx = monitor.position().x as f64 / mscale;
        let mw = monitor.size().width as f64 / mscale;
        x = x.max(mx).min(mx + mw - POPOVER_WIDTH);
    }
    let _ = popover.set_position(LogicalPosition::new(x, y.max(0.0)));
    let _ = popover.set_size(LogicalSize::new(POPOVER_WIDTH, POPOVER_HEIGHT));
    let _ = popover.show();
}

/// Cache le popover (il reste charge pour la prochaine ouverture).
#[tauri::command]
pub fn close_recorder_popover(app: AppHandle) {
    if let Some(popover) = app.get_webview_window(POPOVER_LABEL) {
        let _ = popover.hide();
    }
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

/// Ferme la bulle et son popover.
pub fn close(app: &AppHandle) {
    if let Some(popover) = app.get_webview_window(POPOVER_LABEL) {
        let _ = popover.close();
    }
    if let Some(win) = app.get_webview_window(LABEL) {
        let _ = win.close();
    }
}
