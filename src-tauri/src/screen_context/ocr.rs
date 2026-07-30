// OCR via Windows.Media.Ocr (API systeme Windows 10+).
//
// Reference VoiceInk : Vision.framework VNRecognizeTextRequest avec
// recognitionLevel=.accurate et usesLanguageCorrection=true, auto-detect
// de langue. Sur Windows on utilise OcrEngine::TryCreateFromUserProfileLanguages
// qui choisit automatiquement la langue prefere de l'utilisateur (ou
// anglais en fallback).

use anyhow::{anyhow, Result};
use windows::Graphics::Imaging::BitmapDecoder;
use windows::Media::Ocr::OcrEngine;
use windows::Storage::Streams::{DataWriter, InMemoryRandomAccessStream};

/// Execute l'OCR sur un buffer PNG. Renvoie le texte reconnu ligne par ligne,
/// joint par `\n` (equivalent VoiceInk).
pub fn recognize_png(png: &[u8]) -> Result<String> {
    if png.is_empty() {
        return Err(anyhow!("png vide"));
    }

    // Cree un moteur OCR avec les langues du profil utilisateur.
    let engine = OcrEngine::TryCreateFromUserProfileLanguages()
        .map_err(|e| anyhow!("OcrEngine create: {e}"))?;

    // Stream in-memory pour faire transiter les bytes PNG vers BitmapDecoder.
    let stream = InMemoryRandomAccessStream::new()
        .map_err(|e| anyhow!("InMemoryRandomAccessStream: {e}"))?;

    let writer =
        DataWriter::CreateDataWriter(&stream).map_err(|e| anyhow!("DataWriter create: {e}"))?;
    writer
        .WriteBytes(png)
        .map_err(|e| anyhow!("DataWriter WriteBytes: {e}"))?;
    writer
        .StoreAsync()
        .map_err(|e| anyhow!("DataWriter StoreAsync: {e}"))?
        .get()
        .map_err(|e| anyhow!("DataWriter Store get: {e}"))?;
    writer
        .FlushAsync()
        .map_err(|e| anyhow!("DataWriter FlushAsync: {e}"))?
        .get()
        .map_err(|e| anyhow!("DataWriter Flush get: {e}"))?;
    writer
        .DetachStream()
        .map_err(|e| anyhow!("DataWriter DetachStream: {e}"))?;

    // Remet la position au debut pour le decoder.
    stream.Seek(0).map_err(|e| anyhow!("stream Seek: {e}"))?;

    let decoder = BitmapDecoder::CreateAsync(&stream)
        .map_err(|e| anyhow!("BitmapDecoder CreateAsync: {e}"))?
        .get()
        .map_err(|e| anyhow!("BitmapDecoder get: {e}"))?;
    let bitmap = decoder
        .GetSoftwareBitmapAsync()
        .map_err(|e| anyhow!("GetSoftwareBitmapAsync: {e}"))?
        .get()
        .map_err(|e| anyhow!("GetSoftwareBitmap get: {e}"))?;

    let result = engine
        .RecognizeAsync(&bitmap)
        .map_err(|e| anyhow!("OcrEngine RecognizeAsync: {e}"))?
        .get()
        .map_err(|e| anyhow!("Recognize get: {e}"))?;

    // On recupere ligne par ligne pour preserver la structure du texte
    // (VoiceInk joint les observations par "\n").
    let lines = result
        .Lines()
        .map_err(|e| anyhow!("Ocr result lines: {e}"))?;
    let size = lines.Size().map_err(|e| anyhow!("Lines size: {e}"))?;
    let mut out = String::new();
    for i in 0..size {
        let line = lines
            .GetAt(i)
            .map_err(|e| anyhow!("Lines GetAt {i}: {e}"))?;
        let text = line.Text().map_err(|e| anyhow!("Line text: {e}"))?;
        let s = text.to_string_lossy();
        let trimmed = s.trim();
        if !trimmed.is_empty() {
            if !out.is_empty() {
                out.push('\n');
            }
            out.push_str(trimmed);
        }
    }
    Ok(out)
}
