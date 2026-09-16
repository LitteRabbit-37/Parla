// Hook clavier bas niveau (WH_KEYBOARD_LL) configurable a chaud.
//
// Cree un thread dedie qui installe le hook et pompe une boucle de messages.
// Les evenements KeyDown / KeyUp des touches surveillees sont pousses dans un
// channel mpsc consomme par le HotkeyManager. Le hook vit jusqu'a la fin du
// processus mais sa table de touches surveillees (`watched`) peut etre mise
// a jour a la volee via `update_watched(...)`, sans re-installer le hook.
//
// Reference VoiceInk : HotkeyManager.swift L133-144 keyCodes modifiers macOS
// + selectedHotkey1 / selectedHotkey2 changeables a chaud.
//
// Equivalents Windows (Virtual Key Codes) :
//   rightOption  -> Right Alt   = VK_RMENU    0xA5
//   leftOption   -> Left Alt    = VK_LMENU    0xA4
//   leftControl  -> Left Ctrl   = VK_LCONTROL 0xA2
//   rightControl -> Right Ctrl  = VK_RCONTROL 0xA3
//   rightCommand -> Right Win   = VK_RWIN     0x5C
//   rightShift   -> Right Shift = VK_RSHIFT   0xA1
//   leftShift    -> Left Shift  = VK_LSHIFT   0xA0
//
// Trois sortes de triggers supportes :
//   - None     : disable ce slot (l'utilisateur n'a pas configure de hotkey).
//   - Modifier : touche modifier seule (ex Right Alt) - retro-compat 0.1.x.
//   - Combo    : combinaison libre type Ctrl+Alt+R, F13, etc.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use tracing::{debug, error, info, warn};

#[cfg(windows)]
use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
#[cfg(windows)]
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GetMessageW, SetWindowsHookExW, TranslateMessage,
    UnhookWindowsHookEx, KBDLLHOOKSTRUCT, MSG, WH_KEYBOARD_LL, WM_KEYDOWN, WM_KEYUP,
    WM_SYSKEYDOWN, WM_SYSKEYUP,
};

/// Touches modifier surveillees individuellement (legacy enum, conserve pour
/// retro-compatibilite avec les tests et la config 0.1.x ou seul un modifier
/// pouvait etre choisi).
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum HotkeyOption {
    #[default]
    None,
    RightAlt,
    LeftAlt,
    LeftCtrl,
    RightCtrl,
    RightWin,
    RightShift,
    LeftShift,
}

impl HotkeyOption {
    pub fn vk(self) -> Option<u32> {
        match self {
            HotkeyOption::None => None,
            HotkeyOption::RightAlt => Some(0xA5),
            HotkeyOption::LeftAlt => Some(0xA4),
            HotkeyOption::LeftCtrl => Some(0xA2),
            HotkeyOption::RightCtrl => Some(0xA3),
            HotkeyOption::RightWin => Some(0x5C),
            HotkeyOption::RightShift => Some(0xA1),
            HotkeyOption::LeftShift => Some(0xA0),
        }
    }
}

/// Definition d'un trigger pour un slot (primary ou secondary). Sert a
/// representer les trois cas user : pas de trigger, modifier-only, ou
/// combo libre.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum HotkeyTrigger {
    None,
    Modifier { option: HotkeyOption },
    Combo {
        vk: u32,
        ctrl: bool,
        alt: bool,
        shift: bool,
        win: bool,
    },
}

impl Default for HotkeyTrigger {
    fn default() -> Self {
        HotkeyTrigger::None
    }
}

impl HotkeyTrigger {
    /// Default Parla 0.1.x : Right Alt en modifier seul.
    pub fn default_primary() -> Self {
        HotkeyTrigger::Modifier {
            option: HotkeyOption::RightAlt,
        }
    }
}

/// Slot identifiant qui a fire l'evenement (utilise par HotkeyManager pour
/// appliquer le mode toggle/PTT/hybrid associe).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HotkeySlot {
    Primary,
    Secondary,
}

/// Actions utilitaires declenchables par un raccourci global, en plus des
/// deux slots d'enregistrement.
///
/// Reference VoiceInk : Features/Shortcuts/Models/ShortcutAction.swift
/// `globalUtilityActions` (pasteLastTranscription, pasteLastEnhancement,
/// retryLastTranscription, openHistoryWindow). `CopyLastTranscription` est
/// un ajout Parla : VoiceInk n'expose "Copy Last Transcription" que comme
/// accelerateur de menu (Shift+Cmd+C, actif menu ouvert uniquement), pas
/// comme raccourci global.
///
/// Une action utilitaire fire au KeyDown et l'evenement clavier est
/// consomme (VoiceInk ShortcutMonitor renvoie nil au tap CGEvent), sinon
/// Ctrl+Shift+C serait aussi recu par l'application cible.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum UtilityAction {
    CopyLastTranscription,
    PasteLastTranscription,
    PasteLastEnhancement,
    RetryLastTranscription,
    OpenHistory,
}

impl UtilityAction {
    pub const ALL: [UtilityAction; 5] = [
        UtilityAction::CopyLastTranscription,
        UtilityAction::PasteLastTranscription,
        UtilityAction::PasteLastEnhancement,
        UtilityAction::RetryLastTranscription,
        UtilityAction::OpenHistory,
    ];
}

#[derive(Debug, Clone)]
pub enum HotkeyEvent {
    /// Le trigger surveille a ete presse (transition KeyDown).
    Pressed {
        slot: HotkeySlot,
        timestamp: Instant,
    },
    /// Le trigger surveille a ete relache (transition KeyUp).
    Released {
        slot: HotkeySlot,
        timestamp: Instant,
    },
    /// La touche Escape a ete pressee (pour le double-tap cancel).
    EscapePressed { timestamp: Instant },
    /// Alt+chiffre : selection directe du Nieme profil Power Mode active
    /// (index 0-base). Reference VoiceInk MiniRecorderShortcutManager
    /// selectPowerMode1..10 = Option+1..0. Pas de timestamp : la selection
    /// n'a pas de logique de timing (pas de cooldown, une fire par pression
    /// physique grace au gate !was_down cote hook).
    SelectPowerMode { index: usize },
    /// Un raccourci utilitaire (coller / copier / reessayer / historique)
    /// a ete presse. Fire une fois par pression physique, evenement consomme.
    Utility { action: UtilityAction },
    /// Le raccourci d'annulation personnalise a ete presse pendant un
    /// enregistrement (remplace le double-Echap quand il est configure).
    /// Reference VoiceInk RecorderPanelShortcutManager `.cancelRecorder`.
    CancelRequested { timestamp: Instant },
}

/// Vrai pendant un enregistrement (hotkey ou tray). Sert au hook a n'armer
/// le raccourci d'annulation personnalise que lorsque le recorder est
/// visible (VoiceInk : `visibleRecorderMonitor` demarre/stoppe avec
/// `isRecorderPanelVisible`).
static RECORDING_ACTIVE: AtomicBool = AtomicBool::new(false);

pub fn set_recording_active(active: bool) {
    RECORDING_ACTIVE.store(active, Ordering::Relaxed);
}

pub fn is_recording_active() -> bool {
    RECORDING_ACTIVE.load(Ordering::Relaxed)
}

/// Nombre de profils Power Mode selectionnables via Alt+chiffre pendant que
/// le mini-recorder est visible. 0 = raccourcis desactives (aucune touche
/// Alt+chiffre n'est capturee). Mis a jour au start/stop d'un enregistrement.
///
/// Reference VoiceInk : MiniRecorderShortcutManager n'enregistre les
/// shortcuts selectPowerMode1..10 que pendant la visibilite du recorder et
/// uniquement pour les configurations activees (enabledConfigurations).
static POWER_SHORTCUT_COUNT: AtomicUsize = AtomicUsize::new(0);

/// Active (n>0) ou desactive (n=0) la capture des raccourcis Alt+chiffre de
/// selection Power Mode, en fixant le nombre de profils adressables.
pub fn set_power_shortcut_count(n: usize) {
    POWER_SHORTCUT_COUNT.store(n, Ordering::Relaxed);
}

/// Mappe un VK de chiffre de la rangee du haut vers un index de profil.
/// 1->0, 2->1, ... 9->8, 0->9 (comme VoiceInk .one..-.zero). Le pave
/// numerique (0x60-0x69) est volontairement ignore pour ne pas interferer
/// avec les Alt-codes Windows.
fn power_mode_digit_index(vk: u32) -> Option<usize> {
    match vk {
        0x31..=0x39 => Some((vk - 0x31) as usize), // '1'..'9' -> 0..8
        0x30 => Some(9),                            // '0' -> 10e profil
        _ => None,
    }
}

#[allow(dead_code)] // certains champs uniquement utilises sous cfg(windows)
struct WatchedKeys {
    primary: HotkeyTrigger,
    secondary: HotkeyTrigger,
    /// Etat courant (down/up) pour chaque VK (0..255). Utilise par les
    /// triggers Combo qui doivent verifier les modifiers presents.
    key_state: [bool; 256],
    /// Slot actuellement en train de fire (au hold). Utile pour emettre
    /// le bon Released meme si un Combo se "casse" car un modifier est
    /// relache avant le VK final.
    primary_pressed: bool,
    secondary_pressed: bool,
    /// Timestamp du dernier LCtrl DOWN - filtre AltGr.
    last_lctrl_down: Option<Instant>,
    /// Indique qu'un RAlt DOWN a ete drop comme AltGr -> il faut drop
    /// le RAlt UP correspondant.
    altgr_in_progress: bool,
    /// Raccourcis utilitaires globaux (action -> trigger). Les entrees
    /// `HotkeyTrigger::None` sont filtrees a la mise a jour.
    utilities: Vec<(UtilityAction, HotkeyTrigger)>,
    /// Raccourci d'annulation personnalise. `None` = comportement par
    /// defaut double-Echap (gere par HotkeyManager via EscapePressed).
    cancel: HotkeyTrigger,
    /// VK final d'un combo utilitaire / cancel consomme au KeyDown : le
    /// KeyUp correspondant est consomme aussi pour que l'app cible ne
    /// voie jamais une touche "relachee sans avoir ete pressee".
    consumed_vk: Option<u32>,
}

#[cfg(windows)]
struct HookContext {
    tx: mpsc::Sender<HotkeyEvent>,
    watched: Mutex<WatchedKeys>,
}

#[cfg(windows)]
static HOOK_CONTEXT: OnceLock<HookContext> = OnceLock::new();

/// Installe le hook clavier bas niveau dans un thread dedie.
/// Retourne le receiver des evenements. Le hook vit jusqu'a la fin du
/// processus.
///
/// La table des touches surveillees (`HOOK_CONTEXT`) est initialisee de
/// maniere synchrone AVANT de lancer le thread, pour qu'un
/// `update_watched` / `update_utilities` appele juste apres ne tombe pas
/// dans le vide pendant que le thread demarre.
pub fn install_hook(
    primary: HotkeyTrigger,
    secondary: HotkeyTrigger,
    utilities: Vec<(UtilityAction, HotkeyTrigger)>,
    cancel: HotkeyTrigger,
) -> mpsc::Receiver<HotkeyEvent> {
    let (tx, rx) = mpsc::channel();

    #[cfg(windows)]
    {
        let _ = HOOK_CONTEXT.set(HookContext {
            tx,
            watched: Mutex::new(WatchedKeys {
                primary,
                secondary,
                key_state: [false; 256],
                primary_pressed: false,
                secondary_pressed: false,
                last_lctrl_down: None,
                altgr_in_progress: false,
                utilities: utilities
                    .into_iter()
                    .filter(|(_, t)| !matches!(t, HotkeyTrigger::None))
                    .collect(),
                cancel,
                consumed_vk: None,
            }),
        });
        let _ = std::thread::Builder::new()
            .name("parla-hotkey-hook".into())
            .spawn(run_hook_thread);
    }
    #[cfg(not(windows))]
    {
        let _ = (tx, primary, secondary, utilities, cancel);
    }

    rx
}

/// Met a jour les triggers surveilles sans re-installer le hook. Appele
/// quand l'utilisateur change son raccourci dans Settings.
pub fn update_watched(primary: HotkeyTrigger, secondary: HotkeyTrigger) {
    #[cfg(windows)]
    {
        if let Some(ctx) = HOOK_CONTEXT.get() {
            let mut g = ctx.watched.lock();
            // Reset les flags pressed pour eviter qu'un Released orphan
            // ne fire avec un nouveau slot.
            g.primary_pressed = false;
            g.secondary_pressed = false;
            g.primary = primary;
            g.secondary = secondary;
            info!(
                primary = ?primary,
                secondary = ?secondary,
                "Hotkeys mises a jour a chaud"
            );
        }
    }
    #[cfg(not(windows))]
    {
        let _ = (primary, secondary);
    }
}

/// Met a jour les raccourcis utilitaires et le raccourci d'annulation
/// personnalise sans re-installer le hook. Meme contrat que
/// `update_watched` : appele au boot et a chaque sauvegarde de la config.
pub fn update_utilities(utilities: Vec<(UtilityAction, HotkeyTrigger)>, cancel: HotkeyTrigger) {
    #[cfg(windows)]
    {
        if let Some(ctx) = HOOK_CONTEXT.get() {
            let mut g = ctx.watched.lock();
            g.consumed_vk = None;
            g.utilities = utilities
                .into_iter()
                .filter(|(_, t)| !matches!(t, HotkeyTrigger::None))
                .collect();
            g.cancel = cancel;
            info!(
                utilities = g.utilities.len(),
                cancel = ?g.cancel,
                "Raccourcis utilitaires mis a jour a chaud"
            );
        }
    }
    #[cfg(not(windows))]
    {
        let _ = (utilities, cancel);
    }
}

#[cfg(windows)]
fn run_hook_thread() {
    let hook = unsafe { SetWindowsHookExW(WH_KEYBOARD_LL, Some(low_level_proc), None, 0) };
    let hook = match hook {
        Ok(h) => h,
        Err(e) => {
            error!("SetWindowsHookExW a echoue: {e:?}");
            return;
        }
    };

    debug!("Hook WH_KEYBOARD_LL installe");

    unsafe {
        let mut msg = MSG::default();
        loop {
            let ret = GetMessageW(&mut msg, None, 0, 0);
            match ret.0 {
                -1 => {
                    error!("GetMessageW a retourne -1");
                    break;
                }
                0 => break,
                _ => {
                    let _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
            }
        }
        if let Err(e) = UnhookWindowsHookEx(hook) {
            warn!("UnhookWindowsHookEx: {e:?}");
        }
    }

    debug!("Thread hook clavier termine");
}

#[cfg(windows)]
unsafe extern "system" fn low_level_proc(
    n_code: i32,
    w_param: WPARAM,
    l_param: LPARAM,
) -> LRESULT {
    if n_code >= 0 {
        let info = &*(l_param.0 as *const KBDLLHOOKSTRUCT);
        let vk = info.vkCode;
        let message = w_param.0 as u32;
        let is_down = message == WM_KEYDOWN || message == WM_SYSKEYDOWN;
        let is_up = message == WM_KEYUP || message == WM_SYSKEYUP;

        if is_down || is_up {
            if let Some(ctx) = HOOK_CONTEXT.get() {
                if handle_key(ctx, vk, is_down) {
                    // Evenement consomme (Alt+chiffre Power Mode) : on ne le
                    // propage pas a l'app cible, sinon le chiffre serait tape
                    // dans le document sous le curseur.
                    return LRESULT(1);
                }
            }
        }
    }
    CallNextHookEx(None, n_code, w_param, l_param)
}

/// Fenetre de detection AltGr : Windows synthetise un LCtrl DOWN 0-1ms
/// avant chaque RAlt DOWN sur AZERTY/QWERTZ. 5ms est large vs le delai
/// observable et bien sous ce qu'un humain peut faire a la main.
#[cfg(windows)]
const ALTGR_GATE: Duration = Duration::from_millis(5);

const VK_LCONTROL: u32 = 0xA2;
const VK_RCONTROL: u32 = 0xA3;
const VK_LMENU: u32 = 0xA4;
const VK_RMENU: u32 = 0xA5;
const VK_LSHIFT: u32 = 0xA0;
const VK_RSHIFT: u32 = 0xA1;
const VK_LWIN: u32 = 0x5B;
const VK_RWIN: u32 = 0x5C;
const VK_ESCAPE: u32 = 0x1B;

///
/// Retourne `true` si l'evenement doit etre consomme (non propage a l'app
/// cible). C'est le cas pour Alt+chiffre de selection Power Mode, pour les
/// raccourcis utilitaires (coller / copier / reessayer / historique) et pour
/// le raccourci d'annulation personnalise ; les triggers record et Escape
/// retournent `false` pour preserver le comportement historique (l'app
/// recoit bien la touche).
#[cfg(windows)]
fn handle_key(ctx: &HookContext, vk: u32, is_down: bool) -> bool {
    let now = Instant::now();
    let mut watched = ctx.watched.lock();
    let idx = vk as usize & 0xFF;
    let was_down = watched.key_state[idx];
    watched.key_state[idx] = is_down;

    if vk == VK_LCONTROL && is_down {
        watched.last_lctrl_down = Some(now);
    }

    // Filtre AltGr : RAlt synthetise apres LCtrl en moins de 5ms.
    if vk == VK_RMENU {
        if is_down {
            let is_altgr = watched
                .last_lctrl_down
                .map(|t| now.duration_since(t) < ALTGR_GATE)
                .unwrap_or(false);
            if is_altgr {
                watched.altgr_in_progress = true;
                return false;
            }
        } else if watched.altgr_in_progress {
            watched.altgr_in_progress = false;
            return false;
        }
    }

    // KeyUp du VK final d'un combo consomme au KeyDown : consomme aussi.
    if !is_down && watched.consumed_vk == Some(vk) {
        watched.consumed_vk = None;
        return true;
    }

    // Raccourci d'annulation personnalise : uniquement pendant un
    // enregistrement (VoiceInk : monitor actif tant que le recorder est
    // visible). Quand il est configure, il remplace le double-Echap.
    if is_down && !was_down && is_recording_active() {
        let mods_now = current_modifiers(&watched.key_state);
        if !matches!(watched.cancel, HotkeyTrigger::None)
            && trigger_matches(watched.cancel, vk, mods_now)
        {
            watched.consumed_vk = Some(vk);
            let _ = ctx.tx.send(HotkeyEvent::CancelRequested { timestamp: now });
            return true;
        }
    }

    // Raccourcis utilitaires globaux (VoiceInk ShortcutMonitor : evenement
    // supprime quand un raccourci matche). Une fire par pression physique.
    if is_down && !was_down && !watched.utilities.is_empty() {
        let mods_now = current_modifiers(&watched.key_state);
        let hit = watched
            .utilities
            .iter()
            .find(|(_, t)| trigger_matches(*t, vk, mods_now))
            .map(|(a, _)| *a);
        if let Some(action) = hit {
            watched.consumed_vk = Some(vk);
            let _ = ctx.tx.send(HotkeyEvent::Utility { action });
            return true;
        }
    }

    // Escape pour double-tap cancel - independant des triggers.
    if vk == VK_ESCAPE && is_down && !was_down {
        let _ = ctx.tx.send(HotkeyEvent::EscapePressed { timestamp: now });
        return false;
    }

    // Selection Power Mode par Alt+chiffre pendant que le mini-recorder est
    // visible (POWER_SHORTCUT_COUNT > 0). VoiceInk : selectPowerMode1..10 =
    // Option+1..0. On ne capture QUE Alt seul (sans Ctrl/Shift/Win) pour ne
    // pas voler AltGr+chiffre (AZERTY, Ctrl est down) ni les combos
    // applicatifs, et on laisse passer si le chiffre correspond deja a un
    // hotkey record configure (pour ne pas hijacker le raccourci de l'user).
    let count = POWER_SHORTCUT_COUNT.load(Ordering::Relaxed);
    if count > 0 && is_down && !was_down {
        if let Some(index) = power_mode_digit_index(vk) {
            let mods = current_modifiers(&watched.key_state);
            let is_alt_only = mods.alt && !mods.ctrl && !mods.shift && !mods.win;
            let clashes_trigger = trigger_matches(watched.primary, vk, mods)
                || trigger_matches(watched.secondary, vk, mods);
            if is_alt_only && !clashes_trigger && index < count {
                let _ = ctx.tx.send(HotkeyEvent::SelectPowerMode { index });
                return true;
            }
        }
    }

    // Match du VK contre les deux slots. Important : on teste primary
    // ET secondary, on n'arrete pas au premier match (l'utilisateur
    // peut configurer deux triggers qui partagent un VK terminal mais
    // avec des modifiers differents - cas rare mais autorise).
    let primary = watched.primary;
    let secondary = watched.secondary;
    let mods = current_modifiers(&watched.key_state);

    let primary_matches = trigger_matches(primary, vk, mods);
    let secondary_matches = trigger_matches(secondary, vk, mods);

    // Pour un Combo, un Pressed fire UNIQUEMENT quand le VK final tombe
    // ET tous les modifiers requis sont down. Un Released fire quand le
    // VK final remonte OU n'importe quel modifier requis remonte
    // (sinon on serait bloque en hold).

    if primary_matches && is_down && !watched.primary_pressed {
        watched.primary_pressed = true;
        let _ = ctx.tx.send(HotkeyEvent::Pressed {
            slot: HotkeySlot::Primary,
            timestamp: now,
        });
    } else if watched.primary_pressed && release_check(primary, vk, is_down, mods) {
        watched.primary_pressed = false;
        let _ = ctx.tx.send(HotkeyEvent::Released {
            slot: HotkeySlot::Primary,
            timestamp: now,
        });
    }

    if secondary_matches && is_down && !watched.secondary_pressed {
        watched.secondary_pressed = true;
        let _ = ctx.tx.send(HotkeyEvent::Pressed {
            slot: HotkeySlot::Secondary,
            timestamp: now,
        });
    } else if watched.secondary_pressed && release_check(secondary, vk, is_down, mods) {
        watched.secondary_pressed = false;
        let _ = ctx.tx.send(HotkeyEvent::Released {
            slot: HotkeySlot::Secondary,
            timestamp: now,
        });
    }

    // Aucun evenement Power Mode consomme : on laisse la touche se propager.
    false
}

/// Libelle lisible d'un trigger, pour le menu tray et les badges UI.
/// Format Windows : "Ctrl+Shift+C", "F13", "Right Alt". `None` pour un
/// trigger `None`.
pub fn trigger_label(trigger: HotkeyTrigger) -> Option<String> {
    match trigger {
        HotkeyTrigger::None => None,
        HotkeyTrigger::Modifier { option } => {
            let s = match option {
                HotkeyOption::None => return None,
                HotkeyOption::RightAlt => "Right Alt",
                HotkeyOption::LeftAlt => "Left Alt",
                HotkeyOption::LeftCtrl => "Left Ctrl",
                HotkeyOption::RightCtrl => "Right Ctrl",
                HotkeyOption::RightWin => "Right Win",
                HotkeyOption::RightShift => "Right Shift",
                HotkeyOption::LeftShift => "Left Shift",
            };
            Some(s.to_string())
        }
        HotkeyTrigger::Combo {
            vk,
            ctrl,
            alt,
            shift,
            win,
        } => {
            let mut parts: Vec<String> = Vec::new();
            if ctrl {
                parts.push("Ctrl".into());
            }
            if alt {
                parts.push("Alt".into());
            }
            if shift {
                parts.push("Shift".into());
            }
            if win {
                parts.push("Win".into());
            }
            parts.push(vk_name(vk));
            Some(parts.join("+"))
        }
    }
}

/// Nom d'une touche a partir de son Virtual Key code. Aligne sur la table
/// `VK_NAMES` de src/components/HotkeyRecorder.tsx pour que le tray et
/// les reglages affichent le meme libelle.
pub fn vk_name(vk: u32) -> String {
    let fixed = match vk {
        0x08 => Some("Backspace"),
        0x09 => Some("Tab"),
        0x0D => Some("Enter"),
        0x13 => Some("Pause"),
        0x14 => Some("CapsLock"),
        0x1B => Some("Esc"),
        0x20 => Some("Space"),
        0x21 => Some("PageUp"),
        0x22 => Some("PageDown"),
        0x23 => Some("End"),
        0x24 => Some("Home"),
        0x25 => Some("Left"),
        0x26 => Some("Up"),
        0x27 => Some("Right"),
        0x28 => Some("Down"),
        0x2C => Some("PrintScreen"),
        0x2D => Some("Insert"),
        0x2E => Some("Delete"),
        _ => None,
    };
    if let Some(f) = fixed {
        return f.to_string();
    }
    if (0x30..=0x39).contains(&vk) || (0x41..=0x5A).contains(&vk) {
        return char::from_u32(vk).map(|c| c.to_string()).unwrap_or_default();
    }
    if (0x70..=0x87).contains(&vk) {
        return format!("F{}", vk - 0x70 + 1);
    }
    if (0x60..=0x69).contains(&vk) {
        return format!("Numpad{}", vk - 0x60);
    }
    format!("VK_{:02X}", vk)
}

/// Etat courant des modifiers (lus depuis `key_state`).
#[derive(Debug, Clone, Copy)]
struct Modifiers {
    ctrl: bool,
    alt: bool,
    shift: bool,
    win: bool,
}

fn current_modifiers(key_state: &[bool; 256]) -> Modifiers {
    Modifiers {
        ctrl: key_state[VK_LCONTROL as usize] || key_state[VK_RCONTROL as usize],
        alt: key_state[VK_LMENU as usize] || key_state[VK_RMENU as usize],
        shift: key_state[VK_LSHIFT as usize] || key_state[VK_RSHIFT as usize],
        win: key_state[VK_LWIN as usize] || key_state[VK_RWIN as usize],
    }
}

/// Teste si un trigger se declenche pour ce VK + les modifiers actifs.
fn trigger_matches(trigger: HotkeyTrigger, vk: u32, mods: Modifiers) -> bool {
    match trigger {
        HotkeyTrigger::None => false,
        HotkeyTrigger::Modifier { option } => option.vk() == Some(vk),
        HotkeyTrigger::Combo {
            vk: combo_vk,
            ctrl,
            alt,
            shift,
            win,
        } => {
            // Pour matcher un combo : le VK final doit correspondre ET
            // tous les modifiers requis doivent etre down. Les modifiers
            // SANS check (ex pas requis) doivent ne pas etre presents
            // pour eviter des matches accidentels (Ctrl+R != Ctrl+Alt+R).
            combo_vk == vk
                && ctrl == mods.ctrl
                && alt == mods.alt
                && shift == mods.shift
                && win == mods.win
        }
    }
}

/// Verifie si un evenement correspond a un release du trigger en cours.
/// Pour un Modifier : release du VK lui-meme. Pour un Combo : release
/// du VK final OU release d'un des modifiers requis.
fn release_check(trigger: HotkeyTrigger, vk: u32, is_down: bool, mods: Modifiers) -> bool {
    if is_down {
        return false;
    }
    match trigger {
        HotkeyTrigger::None => false,
        HotkeyTrigger::Modifier { option } => option.vk() == Some(vk),
        HotkeyTrigger::Combo {
            vk: combo_vk,
            ctrl,
            alt,
            shift,
            win,
        } => {
            if combo_vk == vk {
                return true;
            }
            // Si l'un des modifiers requis remonte, le combo casse.
            (ctrl && !mods.ctrl)
                || (alt && !mods.alt)
                || (shift && !mods.shift)
                || (win && !mods.win)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{power_mode_digit_index, trigger_label, vk_name, HotkeyOption, HotkeyTrigger};

    #[test]
    fn trigger_label_formats_combo_and_modifier() {
        let combo = HotkeyTrigger::Combo {
            vk: 0x43,
            ctrl: true,
            alt: false,
            shift: true,
            win: false,
        };
        assert_eq!(trigger_label(combo).as_deref(), Some("Ctrl+Shift+C"));
        let f13 = HotkeyTrigger::Combo {
            vk: 0x7C,
            ctrl: false,
            alt: false,
            shift: false,
            win: false,
        };
        assert_eq!(trigger_label(f13).as_deref(), Some("F13"));
        let m = HotkeyTrigger::Modifier {
            option: HotkeyOption::RightAlt,
        };
        assert_eq!(trigger_label(m).as_deref(), Some("Right Alt"));
        assert_eq!(trigger_label(HotkeyTrigger::None), None);
    }

    #[test]
    fn vk_name_covers_common_keys() {
        assert_eq!(vk_name(0x41), "A");
        assert_eq!(vk_name(0x30), "0");
        assert_eq!(vk_name(0x70), "F1");
        assert_eq!(vk_name(0x87), "F24");
        assert_eq!(vk_name(0x20), "Space");
        assert_eq!(vk_name(0xBA), "VK_BA");
    }

    #[test]
    fn digit_index_maps_top_row_1_to_9() {
        // '1'..'9' (VK 0x31..0x39) -> index 0..8 (VoiceInk selectPowerMode1..9).
        for (vk, expected) in (0x31u32..=0x39).zip(0usize..) {
            assert_eq!(power_mode_digit_index(vk), Some(expected));
        }
    }

    #[test]
    fn digit_index_maps_zero_to_tenth_slot() {
        // '0' (VK 0x30) adresse le 10e profil -> index 9 (selectPowerMode10).
        assert_eq!(power_mode_digit_index(0x30), Some(9));
    }

    #[test]
    fn digit_index_ignores_numpad_and_other_keys() {
        // Pave numerique (0x60-0x69) ignore pour ne pas capter les Alt-codes.
        assert_eq!(power_mode_digit_index(0x60), None); // Numpad 0
        assert_eq!(power_mode_digit_index(0x69), None); // Numpad 9
        // Lettres et autres VK non concernes.
        assert_eq!(power_mode_digit_index(0x41), None); // 'A'
        assert_eq!(power_mode_digit_index(0x1B), None); // Escape
    }
}
