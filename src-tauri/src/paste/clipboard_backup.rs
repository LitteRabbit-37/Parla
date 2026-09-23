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
// Formats supportes : ceux dont le handle Win32 est un HGLOBAL (buffer
// memoire partageable) : CF_TEXT, CF_UNICODETEXT, CF_OEMTEXT, CF_HDROP,
// CF_DIB, CF_DIBV5, CF_RIFF, CF_WAVE, CF_LOCALE, formats enregistres comme
// "HTML Format", "Rich Text Format", "PNG", etc.
//
// Formats NON supportes : ceux dont GetClipboardData renvoie un handle qui
// n'est PAS un HGLOBAL. Les passer a GlobalSize / GlobalLock est un
// comportement indefini qui, selon la valeur du handle et l'etat du tas,
// tue le processus avec STATUS_HEAP_CORRUPTION (0xc0000374, issue #13 :
// une image dans le presse-papiers fait apparaitre CF_BITMAP, synthetise
// par Windows a partir de CF_DIB, et son HBITMAP passait dans GlobalSize).
//   - CF_BITMAP (HBITMAP), CF_PALETTE (HPALETTE), CF_METAFILEPICT,
//     CF_ENHMETAFILE (HENHMETAFILE), CF_OWNERDISPLAY,
//   - les variantes CF_DSP* (bitmap / metafile / enhmetafile),
//   - CF_PRIVATEFIRST..CF_PRIVATELAST (contenu prive, non libere par le
//     systeme) et CF_GDIOBJFIRST..CF_GDIOBJLAST (objets GDI).
// Les images restent sauvegardees via CF_DIB / CF_DIBV5 / PNG, et Windows
// re-synthetise CF_BITMAP lui-meme a la restauration. VoiceInk cote macOS
// copie les pasteboardItems de la meme facon (data brute par type).

use std::ffi::c_void;
use std::ptr;

use anyhow::{anyhow, Result};
use tracing::{debug, warn};
use windows::Win32::Foundation::{HANDLE, HGLOBAL, HWND};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, EnumClipboardFormats, GetClipboardData, OpenClipboard,
    SetClipboardData,
};
use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock, GMEM_MOVEABLE};

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
        let mut bytes: Vec<u8> = text
            .encode_utf16()
            .flat_map(|u| u.to_le_bytes())
            .collect();
        bytes.push(0);
        bytes.push(0);
        Self {
            entries: vec![ClipboardEntry { format: 13, bytes }],
        }
    }
}

/// Formats dont le handle n'est pas un HGLOBAL byte-copyable (skippes au
/// backup ET a la restauration).
///
/// Ne JAMAIS passer ces handles a GlobalSize / GlobalLock / GlobalFree :
/// ce sont des objets GDI ou des handles prives (issue #13).
fn is_skipped_format(format: u32) -> bool {
    const CF_BITMAP: u32 = 2;
    const CF_METAFILEPICT: u32 = 3;
    const CF_PALETTE: u32 = 9;
    const CF_ENHMETAFILE: u32 = 14;
    const CF_OWNERDISPLAY: u32 = 0x80;
    const CF_DSPBITMAP: u32 = 0x82;
    const CF_DSPMETAFILEPICT: u32 = 0x83;
    const CF_DSPENHMETAFILE: u32 = 0x8E;
    const CF_PRIVATEFIRST: u32 = 0x200;
    const CF_PRIVATELAST: u32 = 0x2FF;
    const CF_GDIOBJFIRST: u32 = 0x300;
    const CF_GDIOBJLAST: u32 = 0x3FF;
    matches!(
        format,
        CF_BITMAP
            | CF_METAFILEPICT
            | CF_PALETTE
            | CF_ENHMETAFILE
            | CF_OWNERDISPLAY
            | CF_DSPBITMAP
            | CF_DSPMETAFILEPICT
            | CF_DSPENHMETAFILE
    ) || (CF_PRIVATEFIRST..=CF_PRIVATELAST).contains(&format)
        || (CF_GDIOBJFIRST..=CF_GDIOBJLAST).contains(&format)
}

/// Ouvre le clipboard, enumere tous les formats, copie les bytes de chacun.
pub fn backup_all() -> Result<Backup> {
    unsafe {
        OpenClipboard(Some(HWND(ptr::null_mut())))
            .map_err(|e| anyhow!("OpenClipboard: {e}"))?;
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
                debug!(format = fmt, "format clipboard skip (handle non byte-copyable)");
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
        let handle = GetClipboardData(format).map_err(|e| anyhow!("GetClipboardData {format}: {e}"))?;
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
            // Un backup ne devrait jamais contenir ces formats, mais un
            // GlobalAlloc pose sous CF_BITMAP serait tout aussi invalide.
            if is_skipped_format(entry.format) {
                continue;
            }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skips_every_non_hglobal_format() {
        // Issue #13 : CF_BITMAP synthetise depuis CF_DIB, plus les autres
        // handles GDI / prives.
        for f in [2u32, 3, 9, 14, 0x80, 0x82, 0x83, 0x8E, 0x200, 0x2FF, 0x300, 0x3FF] {
            assert!(is_skipped_format(f), "format {f:#x} doit etre skippe");
        }
    }

    #[test]
    fn keeps_hglobal_formats() {
        // CF_TEXT, CF_UNICODETEXT, CF_OEMTEXT, CF_DIB, CF_DIBV5, CF_HDROP,
        // CF_LOCALE, CF_DSPTEXT et les formats enregistres (>= 0xC000).
        for f in [1u32, 13, 7, 8, 17, 15, 16, 0x81, 0xC000, 0xC0A3, 0xFFFF] {
            assert!(!is_skipped_format(f), "format {f:#x} doit etre conserve");
        }
    }

    #[test]
    fn text_only_backup_is_utf16_with_terminator() {
        let b = Backup::text_only("hi".into());
        assert_eq!(b.entries.len(), 1);
        assert_eq!(b.entries[0].format, 13);
        assert_eq!(b.entries[0].bytes, vec![b'h', 0, b'i', 0, 0, 0]);
    }
}
