// Backup et restore multi-format du clipboard Windows.
//
// Reference VoiceInk : CursorPaster.swift L13-49 utilise
// `pasteboard.pasteboardItems` (NSPasteboard) pour enumerer tous les types
// disponibles et copier leur data brute. A la restore : `clearContents` puis
// re-set de chaque (type, data).
//
// Equivalent Windows : EnumClipboardFormats + GetClipboardData par format +
// GlobalLock/GlobalSize/memcpy. A la restore : OpenClipboard + EmptyClipboard
// + GlobalAlloc(GMEM_MOVEABLE) + SetClipboardData par format.
//
// Formats supportes : ceux dont le handle Win32 pointe sur un buffer memoire
// partageable (CF_TEXT, CF_UNICODETEXT, CF_OEMTEXT, CF_HDROP, CF_DIB,
// CF_DIBV5, CF_RIFF, CF_WAVE, formats enregistres comme "HTML Format",
// "Rich Text Format", etc.).
//
// Formats NON supportes (handles opaques dont la duplication necessite des
// APIs specifiques) : CF_METAFILEPICT, CF_ENHMETAFILE, CF_PALETTE,
// CF_OWNERDISPLAY. On les skippe silencieusement - VoiceInk cote macOS ne
// les gere pas non plus.

use std::ffi::c_void;
use std::ptr;

use anyhow::{anyhow, Result};
use tracing::{debug, warn};
use windows::Win32::Foundation::{HANDLE, HGLOBAL, HWND};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, EnumClipboardFormats, GetClipboardData, OpenClipboard,
    SetClipboardData,
};
use windows::Win32::System::Memory::{
    GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock, GMEM_MOVEABLE,
};

/// Format enregistre pour lequel on a sauvegarde les bytes.
#[derive(Debug, Clone)]
pub struct ClipboardEntry {
    pub format: u32,
    pub bytes: Vec<u8>,
}

/// Contenu complet du clipboard a un instant T, pret a etre restaure.
#[derive(Debug, Clone, Default)]
pub struct Backup {
    pub entries: Vec<ClipboardEntry>,
}

impl Backup {
    /// Fabrique un backup ne contenant que le texte UTF-16 (CF_UNICODETEXT).
    /// Utilise comme fallback si backup_all echoue.
    pub fn text_only(text: String) -> Self {
        // CF_UNICODETEXT = 13, bytes = utf-16 LE + null terminator (2 bytes).
        let mut bytes: Vec<u8> = text.encode_utf16().flat_map(|u| u.to_le_bytes()).collect();
        bytes.push(0);
        bytes.push(0);
        Self {
            entries: vec![ClipboardEntry { format: 13, bytes }],
        }
    }
}

/// Formats dont le handle n'est pas un HGLOBAL byte-copyable (skippe).
fn is_skipped_format(format: u32) -> bool {
    // CF_METAFILEPICT = 3, CF_ENHMETAFILE = 14, CF_PALETTE = 9,
    // CF_OWNERDISPLAY = 0x80. Ces handles necessitent des APIs specifiques
    // (CopyEnhMetaFile, SelectPalette, etc.) pour etre dupliques.
    matches!(format, 3 | 14 | 9 | 0x80)
}

/// Ouvre le clipboard, enumere tous les formats, copie les bytes de chacun.
pub fn backup_all() -> Result<Backup> {
    unsafe {
        OpenClipboard(Some(HWND(ptr::null_mut()))).map_err(|e| anyhow!("OpenClipboard: {e}"))?;
    }

    let mut entries = Vec::new();
    let mut fmt = 0u32;
    let result = (|| -> Result<()> {
        loop {
            fmt = unsafe { EnumClipboardFormats(fmt) };
            if fmt == 0 {
                break;
            }
            if is_skipped_format(fmt) {
                debug!(
                    format = fmt,
                    "format clipboard skip (handle non byte-copyable)"
                );
                continue;
            }
            match read_format_bytes(fmt) {
                Ok(bytes) => {
                    entries.push(ClipboardEntry { format: fmt, bytes });
                }
                Err(e) => {
                    warn!(format = fmt, error = %e, "clipboard read echec, skip");
                }
            }
        }
        Ok(())
    })();

    unsafe {
        let _ = CloseClipboard();
    }
    result?;
    Ok(Backup { entries })
}

fn read_format_bytes(format: u32) -> Result<Vec<u8>> {
    unsafe {
        let handle =
            GetClipboardData(format).map_err(|e| anyhow!("GetClipboardData {format}: {e}"))?;
        if handle.is_invalid() {
            return Err(anyhow!("handle clipboard invalide"));
        }
        let hglobal = HGLOBAL(handle.0);
        let size = GlobalSize(hglobal);
        if size == 0 {
            return Ok(Vec::new());
        }
        let ptr = GlobalLock(hglobal) as *const u8;
        if ptr.is_null() {
            return Err(anyhow!("GlobalLock a echoue"));
        }
        let mut buf = vec![0u8; size];
        ptr::copy_nonoverlapping(ptr, buf.as_mut_ptr(), size);
        let _ = GlobalUnlock(hglobal);
        Ok(buf)
    }
}

/// Efface le clipboard puis re-ecrit tous les formats sauvegardes.
pub fn restore_all(backup: &Backup) -> Result<()> {
    unsafe {
        OpenClipboard(Some(HWND(ptr::null_mut())))
            .map_err(|e| anyhow!("OpenClipboard (restore): {e}"))?;
    }

    let result = (|| -> Result<()> {
        unsafe {
            EmptyClipboard().map_err(|e| anyhow!("EmptyClipboard: {e}"))?;
        }
        for entry in &backup.entries {
            if let Err(e) = write_format_bytes(entry.format, &entry.bytes) {
                warn!(format = entry.format, error = %e, "clipboard write echec, skip");
            }
        }
        Ok(())
    })();

    unsafe {
        let _ = CloseClipboard();
    }
    result
}

fn write_format_bytes(format: u32, bytes: &[u8]) -> Result<()> {
    // Alloue un bloc memoire partage. GMEM_MOVEABLE est necessaire pour
    // SetClipboardData (le systeme prend possession du handle).
    if bytes.is_empty() {
        return Ok(());
    }
    unsafe {
        let hmem = GlobalAlloc(GMEM_MOVEABLE, bytes.len())
            .map_err(|e| anyhow!("GlobalAlloc {}: {e}", bytes.len()))?;
        if hmem.is_invalid() {
            return Err(anyhow!("GlobalAlloc a retourne un handle invalide"));
        }
        let dst = GlobalLock(hmem) as *mut c_void;
        if dst.is_null() {
            return Err(anyhow!("GlobalLock (write) a echoue"));
        }
        ptr::copy_nonoverlapping(bytes.as_ptr() as *const c_void, dst, bytes.len());
        let _ = GlobalUnlock(hmem);

        // A partir de ce point, le systeme devient proprietaire du handle en
        // cas de succes. En cas d'echec on ne le libere pas explicitement :
        // la doc dit que le handle est libere lorsque l'app sort ou que le
        // clipboard est empty. Acceptable vu que c'est un cas d'erreur rare.
        SetClipboardData(format, Some(HANDLE(hmem.0)))
            .map_err(|e| anyhow!("SetClipboardData {format}: {e}"))?;
    }
    Ok(())
}
