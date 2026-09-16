// Actions sur la derniere transcription : copier, coller (originale ou
// amelioree), reessayer, ouvrir l'historique.
//
// Reference VoiceInk : Features/History/Workflows/LastTranscriptionService.swift
//  - copyLastTranscription : texte ameliore si present, sinon original,
//    copie dans le presse-papiers + notification.
//  - pasteLastTranscription : texte ORIGINAL colle au curseur apres 0.15 s.
//  - pasteLastEnhancement   : texte ameliore (repli original) colle au
//    curseur apres 0.15 s.
//  - retryLastTranscription : retranscrit le dernier fichier audio avec le
//    modele courant (enhancement compris), copie le resultat dans le
//    presse-papiers et cree une nouvelle entree d'historique.
// Ces actions sont declenchees par le menu tray et par les raccourcis
// additionnels (hotkeys::keyboard_hook::UtilityAction).

use std::path::PathBuf;
use std::time::Duration;

use anyhow::{anyhow, Result};
use tauri::{AppHandle, Emitter, Manager};
use tracing::{info, warn};

use crate::commands::recording::RecorderState;
use crate::db::{transcription as history_repo, Database};
use crate::paste;
use crate::transcription::pipeline;

/// Delai avant le collage (VoiceInk `DispatchQueue.main.asyncAfter(0.15)`) :
/// laisse le temps aux touches du raccourci d'etre relachees pour que le
/// Ctrl+V synthetique ne soit pas combine avec un modificateur encore
/// enfonce.
const PASTE_DELAY: Duration = Duration::from_millis(150);

/// Derniere transcription enregistree (la plus recente par timestamp).
pub fn last_record(app: &AppHandle) -> Result<history_repo::TranscriptionRecord> {
    let db = app
        .try_state::<Database>()
        .ok_or_else(|| anyhow!("database not initialized"))?;
    let page = {
        let conn = db.0.lock();
        history_repo::list_page(&conn, 1, None, None)?
    };
    page.into_iter()
        .next()
        .ok_or_else(|| anyhow!("PARLA_ERR:historyEmpty"))
}

/// Texte ameliore si present et non vide, sinon texte original.
fn preferred_text(rec: &history_repo::TranscriptionRecord) -> String {
    rec.enhanced_text
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or(&rec.text)
        .to_string()
}

fn notice(app: &AppHandle, message: impl Into<String>) {
    let _ = app.emit("tray:notice", message.into());
}

/// Copie la derniere transcription (amelioree de preference) dans le
/// presse-papiers.
pub fn copy_last(app: &AppHandle) -> Result<()> {
    let rec = last_record(app)?;
    let text = preferred_text(&rec);
    if text.trim().is_empty() {
        anyhow::bail!("PARLA_ERR:lastTranscriptionEmpty");
    }
    paste::copy_to_clipboard(&text)?;
    info!(chars = text.len(), "Last transcription copied");
    notice(app, "Last transcription copied");
    Ok(())
}

/// Colle la derniere transcription au curseur. `enhanced` = true colle le
/// texte ameliore (repli original), sinon le texte original.
pub fn paste_last(app: &AppHandle, enhanced: bool) -> Result<()> {
    let rec = last_record(app)?;
    let text = if enhanced {
        preferred_text(&rec)
    } else {
        rec.text.clone()
    };
    if text.trim().is_empty() {
        anyhow::bail!("PARLA_ERR:lastTranscriptionEmpty");
    }
    // La fenetre au premier plan est celle ou l'utilisateur a presse le
    // raccourci : c'est elle qui doit recevoir le Ctrl+V, pas la derniere
    // cible memorisee au dernier enregistrement.
    paste::remember_foreground();
    let restore = pipeline::get_restore_clipboard(app);
    let app = app.clone();
    std::thread::Builder::new()
        .name("parla-paste-last".into())
        .spawn(move || {
            std::thread::sleep(PASTE_DELAY);
            match paste::paste_at_cursor(&text, restore, None) {
                Ok(()) => info!(enhanced, chars = text.len(), "Last transcription pasted"),
                Err(e) => {
                    warn!(error = %e, "paste last transcription failed");
                    notice(&app, format!("Paste failed: {e}"));
                }
            }
        })
        .map_err(|e| anyhow!("thread: {e}"))?;
    Ok(())
}

/// Retranscrit le dernier fichier audio avec le modele courant et copie le
/// resultat. Refuse pendant un enregistrement (le pipeline partage l'etat
/// de session d'historique).
pub fn retry_last(app: &AppHandle) -> Result<()> {
    if app
        .try_state::<RecorderState>()
        .map(|s| s.0.lock().is_some())
        .unwrap_or(false)
    {
        anyhow::bail!("PARLA_ERR:recordingInProgress");
    }
    let rec = last_record(app)?;
    let name = rec
        .audio_file_name
        .filter(|n| !n.is_empty())
        .ok_or_else(|| anyhow!("PARLA_ERR:audioMissing"))?;
    let path: PathBuf = app
        .path()
        .app_local_data_dir()
        .map_err(|e| anyhow!("app_local_data_dir: {e}"))?
        .join("Recordings")
        .join(name);
    if !path.exists() {
        anyhow::bail!("PARLA_ERR:audioMissing");
    }
    info!(path = %path.display(), "Retrying last transcription");
    pipeline::run_retry(app.clone(), path);
    Ok(())
}

/// Affiche la fenetre principale sur l'onglet Historique (equivalent de la
/// fenetre d'historique detachee de VoiceInk).
pub fn open_history(app: &AppHandle) {
    crate::mini_recorder::show_main_window(app.clone(), Some("history".into()));
}
