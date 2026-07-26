// Capture de la fenetre actuellement active.
//
// Reference VoiceInk : ScreenCaptureService.swift utilise ScreenCaptureKit
// (SCShareableContent + SCScreenshotManager) avec oversampling x2 pour
// ameliorer la precision de l'OCR Vision.
//
// Sur Windows, on passe par la crate `xcap` qui abstrait BitBlt/Desktop
// Duplication. On selectionne la fenetre correspondant au PID du
// foreground window pour ne pas capturer Parla lui-meme.

use anyhow::{anyhow, Result};
use image::codecs::png::PngEncoder;
use image::{ImageEncoder, RgbaImage};
use xcap::Window;

use crate::power_mode::active_window::ActiveWindow;

pub struct CaptureResult {
    pub png: Vec<u8>,
    pub app_name: String,
    pub window_title: String,
}

pub fn capture_foreground(active: &ActiveWindow) -> Result<CaptureResult> {
    let windows = Window::all().map_err(|e| anyhow!("xcap Window::all: {e}"))?;
    if windows.is_empty() {
        return Err(anyhow!("aucune fenetre detectee"));
    }
    // Trouve la fenetre correspondante : priorite au match PID, sinon titre,
    // sinon la premiere non minimisee.
    let target = windows
        .iter()
        .find(|w| {
            !w.is_minimized().unwrap_or(true) && w.pid().map(|p| p == active.pid).unwrap_or(false)
        })
        .or_else(|| {
            windows.iter().find(|w| {
                !w.is_minimized().unwrap_or(true) && w.title().unwrap_or_default() == active.title
            })
        })
        .or_else(|| windows.iter().find(|w| !w.is_minimized().unwrap_or(true)))
        .ok_or_else(|| anyhow!("aucune fenetre capturable"))?;

    let rgba: RgbaImage = target
        .capture_image()
        .map_err(|e| anyhow!("xcap capture: {e}"))?;

    let (w, h) = (rgba.width(), rgba.height());
    if w == 0 || h == 0 {
        return Err(anyhow!("capture vide"));
    }

    let app_name = target.app_name().unwrap_or_default();
    let window_title = target.title().unwrap_or_default();

    let mut png = Vec::with_capacity((w * h) as usize);
    PngEncoder::new(&mut png)
        .write_image(&rgba, w, h, image::ColorType::Rgba8.into())
        .map_err(|e| anyhow!("png encode: {e}"))?;

    Ok(CaptureResult {
        png,
        app_name,
        window_title,
    })
}
