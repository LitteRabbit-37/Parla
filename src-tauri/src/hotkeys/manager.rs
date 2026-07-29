// Manager hotkey - machine a etats qui orchestre la relation touche -> action.
//
// Reference VoiceInk : VoiceInk/HotkeyManager.swift L382-437 processKeyPress.
//
// Modes :
//  - Toggle      : tap ou hold -> toggle recorder.
//  - PushToTalk  : down -> start, up -> stop.
//  - Hybrid      : si press > 500 ms : PTT (stop au release). Si tap court : toggle on + hands-free.

use std::sync::mpsc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use tracing::{debug, info};

use super::keyboard_hook::{HotkeyEvent, HotkeySlot};

/// Seuil exact VoiceInk L92 : 0.5 s.
const HYBRID_PRESS_THRESHOLD: Duration = Duration::from_millis(500);
/// Cooldown anti-rebond apres une action, VoiceInk L89-90.
const ACTION_COOLDOWN: Duration = Duration::from_millis(500);
/// Fenetre pour double-tap Escape (VoiceInk MiniRecorderShortcutManager L43).
const ESC_DOUBLE_TAP_WINDOW: Duration = Duration::from_millis(1500);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum HotkeyMode {
    Toggle,
    PushToTalk,
    #[default]
    Hybrid,
}

/// Action derivee par le manager et consommee par le reste de l'app.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HotkeyAction {
    StartRecording,
    StopRecording,
    CancelRecording,
    /// Transition en mode hands-free (toggle on, restera jusqu'a stop explicite).
    EnterHandsFree,
    /// Selection directe du Nieme profil Power Mode active (Alt+chiffre).
    SelectPowerMode(usize),
}

#[derive(Default)]
struct State {
    is_recording: bool,
    is_hands_free: bool,
    key_down_since: Option<Instant>,
    last_action_at: Option<Instant>,
    esc_first_press_at: Option<Instant>,
}

pub struct HotkeyManager {
    state: Mutex<State>,
    /// Mode applique a chaque slot. Stockes derriere un Mutex pour etre
    /// modifiable a chaud quand l'utilisateur change la config dans
    /// Settings sans avoir a recreer le manager.
    modes: Mutex<HotkeyModes>,
}

#[derive(Debug, Clone, Copy)]
struct HotkeyModes {
    primary: HotkeyMode,
    secondary: HotkeyMode,
}

impl HotkeyManager {
    pub fn with_modes(mode_primary: HotkeyMode, mode_secondary: HotkeyMode) -> Self {
        Self {
            state: Mutex::new(State::default()),
            modes: Mutex::new(HotkeyModes {
                primary: mode_primary,
                secondary: mode_secondary,
            }),
        }
    }

    pub fn update_modes(&self, primary: HotkeyMode, secondary: HotkeyMode) {
        let mut g = self.modes.lock();
        g.primary = primary;
        g.secondary = secondary;
    }

    /// Notifie le manager que l'enregistrement a reellement demarre ou s'est arrete.
    pub fn mark_recording_state(&self, recording: bool) {
        let mut state = self.state.lock();
        state.is_recording = recording;
        if !recording {
            state.is_hands_free = false;
            state.key_down_since = None;
        }
    }

    /// Transforme un evenement hook en action metier. None si rien a faire.
    pub fn handle_event(&self, event: HotkeyEvent) -> Option<HotkeyAction> {
        match event {
            HotkeyEvent::Pressed { slot, timestamp } => self.on_pressed(slot, timestamp),
            HotkeyEvent::Released { slot, timestamp } => self.on_released(slot, timestamp),
            HotkeyEvent::EscapePressed { timestamp } => self.on_escape(timestamp),
            // Selection Power Mode : pass-through direct, sans passer par la
            // machine a etats toggle/PTT/hybrid ni le cooldown (le gating est
            // fait en amont par le hook via POWER_SHORTCUT_COUNT).
            HotkeyEvent::SelectPowerMode { index } => Some(HotkeyAction::SelectPowerMode(index)),
        }
    }

    fn mode_for(&self, slot: HotkeySlot) -> HotkeyMode {
        let g = self.modes.lock();
        match slot {
            HotkeySlot::Primary => g.primary,
            HotkeySlot::Secondary => g.secondary,
        }
    }

    fn on_pressed(&self, slot: HotkeySlot, timestamp: Instant) -> Option<HotkeyAction> {
        let mut state = self.state.lock();

        // Cooldown.
        if let Some(last) = state.last_action_at {
            if timestamp.duration_since(last) < ACTION_COOLDOWN {
                debug!("Cooldown actif, press ignore");
                return None;
            }
        }

        state.key_down_since = Some(timestamp);

        let mode = self.mode_for(slot);
        match mode {
            HotkeyMode::PushToTalk => {
                if !state.is_recording {
                    state.is_recording = true;
                    state.last_action_at = Some(timestamp);
                    info!("PTT: start");
                    Some(HotkeyAction::StartRecording)
                } else {
                    None
                }
            }
            HotkeyMode::Toggle | HotkeyMode::Hybrid => {
                // VoiceInk L386-411 : si hands-free actif, un press coupe la session.
                if state.is_hands_free && state.is_recording {
                    state.is_recording = false;
                    state.is_hands_free = false;
                    state.last_action_at = Some(timestamp);
                    info!("Toggle: stop (exit hands-free)");
                    Some(HotkeyAction::StopRecording)
                } else if !state.is_recording {
                    state.is_recording = true;
                    state.last_action_at = Some(timestamp);
                    info!("Toggle/Hybrid: start");
                    Some(HotkeyAction::StartRecording)
                } else {
                    None
                }
            }
        }
    }

    fn on_released(&self, slot: HotkeySlot, timestamp: Instant) -> Option<HotkeyAction> {
        let mut state = self.state.lock();
        let pressed_at = state.key_down_since.take();
        let press_duration = pressed_at
            .map(|t| timestamp.duration_since(t))
            .unwrap_or_default();

        let mode = self.mode_for(slot);
        match mode {
            HotkeyMode::Toggle => {
                // VoiceInk HotkeyManager L414-415 : release en Toggle arme le
                // hands-free pour que la prochaine press coupe la session.
                // Sans ca, un "toggle" pur ne pourrait jamais s'eteindre via press.
                if state.is_recording {
                    state.is_hands_free = true;
                }
                None
            }
            HotkeyMode::PushToTalk => {
                if state.is_recording {
                    state.is_recording = false;
                    state.last_action_at = Some(timestamp);
                    info!("PTT: stop");
                    Some(HotkeyAction::StopRecording)
                } else {
                    None
                }
            }
            HotkeyMode::Hybrid => {
                // VoiceInk L424-432 : hold long = PTT, tap court = hands-free.
                if press_duration >= HYBRID_PRESS_THRESHOLD && state.is_recording {
                    state.is_recording = false;
                    state.last_action_at = Some(timestamp);
                    info!(
                        duration_ms = press_duration.as_millis() as u64,
                        "Hybrid: stop (PTT)"
                    );
                    Some(HotkeyAction::StopRecording)
                } else if state.is_recording {
                    state.is_hands_free = true;
                    info!(
                        duration_ms = press_duration.as_millis() as u64,
                        "Hybrid: hands-free activated"
                    );
                    Some(HotkeyAction::EnterHandsFree)
                } else {
                    None
                }
            }
        }
    }

    fn on_escape(&self, timestamp: Instant) -> Option<HotkeyAction> {
        let mut state = self.state.lock();
        if !state.is_recording {
            return None;
        }
        match state.esc_first_press_at {
            Some(first) if timestamp.duration_since(first) <= ESC_DOUBLE_TAP_WINDOW => {
                state.esc_first_press_at = None;
                state.is_recording = false;
                state.is_hands_free = false;
                state.last_action_at = Some(timestamp);
                info!("Double ESC: cancel recording");
                Some(HotkeyAction::CancelRecording)
            }
            _ => {
                state.esc_first_press_at = Some(timestamp);
                debug!("ESC pressed once, press again within 1500ms to cancel");
                None
            }
        }
    }
}

/// Boucle de traitement : consomme les evenements hook et retourne les actions.
/// A faire tourner dans un thread dedie.
pub fn dispatch_loop<F>(
    rx: mpsc::Receiver<HotkeyEvent>,
    manager: std::sync::Arc<HotkeyManager>,
    mut on_action: F,
) where
    F: FnMut(HotkeyAction) + Send + 'static,
{
    while let Ok(event) = rx.recv() {
        if let Some(action) = manager.handle_event(event) {
            on_action(action);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SLOT: HotkeySlot = HotkeySlot::Primary;

    fn pressed(t: Instant) -> HotkeyEvent {
        HotkeyEvent::Pressed {
            slot: SLOT,
            timestamp: t,
        }
    }
    fn released(t: Instant) -> HotkeyEvent {
        HotkeyEvent::Released {
            slot: SLOT,
            timestamp: t,
        }
    }
    fn esc(t: Instant) -> HotkeyEvent {
        HotkeyEvent::EscapePressed { timestamp: t }
    }

    #[test]
    fn toggle_press_starts_then_stops() {
        let m = HotkeyManager::with_modes(HotkeyMode::Toggle, HotkeyMode::Toggle);
        let t0 = Instant::now();
        assert_eq!(
            m.handle_event(pressed(t0)),
            Some(HotkeyAction::StartRecording)
        );
        assert_eq!(
            m.handle_event(released(t0 + Duration::from_millis(100))),
            None
        );
        // Cooldown : second press doit etre ignore pendant 500ms.
        assert_eq!(
            m.handle_event(pressed(t0 + Duration::from_millis(300))),
            None,
            "cooldown actif"
        );
        // Apres cooldown : stop.
        assert_eq!(
            m.handle_event(pressed(t0 + Duration::from_millis(700))),
            Some(HotkeyAction::StopRecording)
        );
    }

    #[test]
    fn toggle_release_is_noop() {
        let m = HotkeyManager::with_modes(HotkeyMode::Toggle, HotkeyMode::Toggle);
        let t0 = Instant::now();
        m.handle_event(pressed(t0));
        // En toggle, le release ne doit jamais produire d'action.
        assert_eq!(m.handle_event(released(t0 + Duration::from_secs(5))), None);
    }

    #[test]
    fn ptt_press_starts_release_stops() {
        let m = HotkeyManager::with_modes(HotkeyMode::PushToTalk, HotkeyMode::PushToTalk);
        let t0 = Instant::now();
        assert_eq!(
            m.handle_event(pressed(t0)),
            Some(HotkeyAction::StartRecording)
        );
        assert_eq!(
            m.handle_event(released(t0 + Duration::from_millis(200))),
            Some(HotkeyAction::StopRecording)
        );
    }

    #[test]
    fn hybrid_long_press_is_ptt() {
        let m = HotkeyManager::with_modes(HotkeyMode::Hybrid, HotkeyMode::Hybrid);
        let t0 = Instant::now();
        m.handle_event(pressed(t0));
        // Hold >= 500ms : comportement PTT, release = stop.
        assert_eq!(
            m.handle_event(released(t0 + Duration::from_millis(600))),
            Some(HotkeyAction::StopRecording)
        );
    }

    #[test]
    fn hybrid_short_press_enters_hands_free() {
        let m = HotkeyManager::with_modes(HotkeyMode::Hybrid, HotkeyMode::Hybrid);
        let t0 = Instant::now();
        m.handle_event(pressed(t0));
        // Tap court (<500ms) : active hands-free au release.
        assert_eq!(
            m.handle_event(released(t0 + Duration::from_millis(100))),
            Some(HotkeyAction::EnterHandsFree)
        );
    }

    #[test]
    fn hybrid_hands_free_next_press_stops() {
        let m = HotkeyManager::with_modes(HotkeyMode::Hybrid, HotkeyMode::Hybrid);
        let t0 = Instant::now();
        m.handle_event(pressed(t0));
        m.handle_event(released(t0 + Duration::from_millis(100)));
        // Un nouveau press (apres cooldown) doit terminer la session hands-free.
        assert_eq!(
            m.handle_event(pressed(t0 + Duration::from_millis(700))),
            Some(HotkeyAction::StopRecording)
        );
    }

    #[test]
    fn secondary_slot_uses_its_own_mode() {
        // Primary = Toggle, Secondary = PushToTalk : verify les deux slots
        // sont independants.
        let m = HotkeyManager::with_modes(HotkeyMode::Toggle, HotkeyMode::PushToTalk);
        let t0 = Instant::now();
        // Press secondary -> PTT start
        assert_eq!(
            m.handle_event(HotkeyEvent::Pressed {
                slot: HotkeySlot::Secondary,
                timestamp: t0
            }),
            Some(HotkeyAction::StartRecording)
        );
        // Release secondary apres 100ms -> PTT stop
        assert_eq!(
            m.handle_event(HotkeyEvent::Released {
                slot: HotkeySlot::Secondary,
                timestamp: t0 + Duration::from_millis(100),
            }),
            Some(HotkeyAction::StopRecording)
        );
    }

    #[test]
    fn select_power_mode_passes_through_index() {
        // SelectPowerMode contourne la machine a etats (toggle/PTT) et le
        // cooldown : il retourne toujours l'action avec l'index, quel que
        // soit l'etat courant du manager.
        let m = HotkeyManager::with_modes(HotkeyMode::Hybrid, HotkeyMode::Hybrid);
        assert_eq!(
            m.handle_event(HotkeyEvent::SelectPowerMode { index: 0 }),
            Some(HotkeyAction::SelectPowerMode(0))
        );
        assert_eq!(
            m.handle_event(HotkeyEvent::SelectPowerMode { index: 9 }),
            Some(HotkeyAction::SelectPowerMode(9))
        );
    }

    #[test]
    fn double_esc_cancels_when_recording() {
        let m = HotkeyManager::with_modes(HotkeyMode::Toggle, HotkeyMode::Toggle);
        let t0 = Instant::now();
        m.handle_event(pressed(t0));
        // Premier ESC : pas d'action (arm le double-tap).
        assert_eq!(m.handle_event(esc(t0 + Duration::from_millis(100))), None);
        // Second ESC dans la fenetre : cancel.
        assert_eq!(
            m.handle_event(esc(t0 + Duration::from_millis(500))),
            Some(HotkeyAction::CancelRecording)
        );
    }

    #[test]
    fn single_esc_does_not_cancel_when_not_recording() {
        let m = HotkeyManager::with_modes(HotkeyMode::Toggle, HotkeyMode::Toggle);
        let t0 = Instant::now();
        // Pas d'enregistrement en cours : ESC ne fait rien.
        assert_eq!(m.handle_event(esc(t0)), None);
        assert_eq!(m.handle_event(esc(t0 + Duration::from_millis(500))), None);
    }

    #[test]
    fn double_esc_outside_window_does_not_cancel() {
        let m = HotkeyManager::with_modes(HotkeyMode::Toggle, HotkeyMode::Toggle);
        let t0 = Instant::now();
        m.handle_event(pressed(t0));
        m.handle_event(esc(t0 + Duration::from_millis(100)));
        // Second ESC 2s plus tard : hors fenetre (1500ms), pas de cancel.
        assert_eq!(m.handle_event(esc(t0 + Duration::from_millis(2100))), None);
    }

    #[test]
    fn mark_recording_state_resets_hands_free() {
        let m = HotkeyManager::with_modes(HotkeyMode::Hybrid, HotkeyMode::Hybrid);
        let t0 = Instant::now();
        m.handle_event(pressed(t0));
        m.handle_event(released(t0 + Duration::from_millis(100)));
        // Hands-free etait actif. Simule un stop externe.
        m.mark_recording_state(false);
        // Un press immediat ne doit rien produire car state cleared et cooldown inactif.
        // En realite le cooldown reste actif mais le state est reset.
        assert_eq!(
            m.handle_event(pressed(t0 + Duration::from_millis(700))),
            Some(HotkeyAction::StartRecording),
            "apres reset state, un nouveau cycle commence"
        );
    }
}
