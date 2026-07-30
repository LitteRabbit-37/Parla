// Extraction de l'URL du navigateur actif via UI Automation.
//
// Reference VoiceInk : BrowserURLService.swift utilise osascript (AppleScript)
// sur macOS. Sur Windows on passe par UI Automation (UIA) pour lire la barre
// d'adresse du navigateur. Cette approche marche pour les Chromium (Chrome,
// Edge, Brave, Vivaldi, Opera, Arc) et Firefox (qui expose aussi un Edit UIA
// sur sa barre d'adresse).
//
// Limites :
// - La barre d'adresse doit etre visible (pas en fullscreen strict).
// - Certains navigateurs tronquent la partie `https://` dans la valeur
//   affichee ; on la reajoute si absente.
// - En cas d'echec (UIA lent, fenetre inhabituelle), on renvoie None.

use anyhow::Result;
use uiautomation::controls::ControlType;
use uiautomation::types::{TreeScope, UIProperty};
use uiautomation::UIAutomation;

use super::active_window::ActiveWindow;

/// Tente d'extraire l'URL courante du navigateur decrit par `active`.
/// Renvoie None si la fenetre n'est pas un navigateur supporte ou si UIA
/// n'arrive pas a trouver la barre d'adresse.
pub fn extract_url(active: &ActiveWindow) -> Option<String> {
    if !super::active_window::is_browser(active) {
        return None;
    }
    try_extract(active).unwrap_or_default()
}

fn try_extract(active: &ActiveWindow) -> Result<Option<String>> {
    let automation = UIAutomation::new()?;
    let hwnd = windows::Win32::Foundation::HWND(active.hwnd as *mut _);
    // Note: uiautomation crate a sa propre version de windows::HWND.
    // On passe par le handle raw i32/isize via element_from_handle.
    let handle = uiautomation::types::Handle::from(active.hwnd);
    let root = automation.element_from_handle(handle)?;
    let _ = hwnd;

    // Recherche un descendant de type Edit. On prend le premier trouve.
    // Les Chromium marquent l'omnibox comme Edit avec le nom "Address and
    // search bar". Firefox marque "Search with <engine> or enter address".
    let matcher = automation
        .create_matcher()
        .from(root)
        .control_type(ControlType::Edit)
        .depth(16);
    let edit = matcher.find_first().ok();

    let Some(edit) = edit else {
        return Ok(None);
    };

    // Lire la valeur via ValuePattern si disponible.
    if let Ok(vp) = edit.get_pattern::<uiautomation::patterns::UIValuePattern>() {
        if let Ok(raw) = vp.get_value() {
            let t = raw.trim();
            if !t.is_empty() {
                return Ok(Some(normalize_url(t)));
            }
        }
    }

    // Fallback : propriete Name (certains navigateurs n'ont pas ValuePattern).
    if let Ok(name) = edit.get_property_value(UIProperty::Name) {
        if let Ok(s) = name.get_string() {
            let t = s.trim();
            if !t.is_empty() {
                return Ok(Some(normalize_url(t)));
            }
        }
    }

    // Indique a UIA qu'on a cherche sur le scope descendants (utilise pour
    // eviter la gene du borrow checker dans la lib).
    let _ = TreeScope::Descendants;
    Ok(None)
}

/// Normalise une URL affichee : ajoute https:// si absent, strip trailing
/// whitespace. Volontairement minimaliste (matcher est deja tolerant).
pub fn normalize_url(raw: &str) -> String {
    let t = raw.trim();
    if t.starts_with("http://") || t.starts_with("https://") || t.starts_with("about:") {
        return t.to_string();
    }
    format!("https://{t}")
}

/// Normalisation utilisee par le matcher (VoiceInk `cleanURL`) :
/// lowercase, strip https/http/www, trim.
pub fn clean_for_match(raw: &str) -> String {
    let mut s = raw.trim().to_ascii_lowercase();
    for prefix in ["https://", "http://"] {
        if let Some(rest) = s.strip_prefix(prefix) {
            s = rest.to_string();
        }
    }
    if let Some(rest) = s.strip_prefix("www.") {
        s = rest.to_string();
    }
    s
}
