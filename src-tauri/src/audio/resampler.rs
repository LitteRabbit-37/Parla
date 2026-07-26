// Resampler Float32 streaming vers 16 kHz mono via rubato.
//
// Reference VoiceInk : VoiceInk/CoreAudioRecorder.swift:631-713 (convertAndWriteToFile)
// VoiceInk utilise une interpolation lineaire maison. Sur Windows on prefere rubato
// (SincFixedIn) qui est de meilleure qualite et supporte les ratios non entiers.
// Pour la voix cela reste suffisamment rapide en temps reel.

use anyhow::{anyhow, Result};
use rubato::{
    Resampler, SincFixedIn, SincInterpolationParameters, SincInterpolationType, WindowFunction,
};

use super::TARGET_SAMPLE_RATE;

/// Taille des chunks pousses dans le resampler (en frames d'entree).
/// rubato::SincFixedIn impose une taille fixe. 480 frames = 10 ms a 48 kHz,
/// un compromis courant entre latence et qualite.
const INPUT_CHUNK: usize = 480;

pub struct MonoResampler {
    inner: Option<SincFixedIn<f32>>,
    input_sample_rate: u32,
    carry: Vec<f32>, // accumulation inter-chunks
}

impl MonoResampler {
    pub fn new(input_sample_rate: u32) -> Result<Self> {
        if input_sample_rate == 0 {
            return Err(anyhow!("sample rate d'entree invalide"));
        }
        let ratio = TARGET_SAMPLE_RATE as f64 / input_sample_rate as f64;
        // Si le peripherique est deja a 16 kHz, on court-circuite le resampler.
        let inner = if input_sample_rate == TARGET_SAMPLE_RATE {
            None
        } else {
            let params = SincInterpolationParameters {
                sinc_len: 128,
                f_cutoff: 0.95,
                interpolation: SincInterpolationType::Linear,
                oversampling_factor: 128,
                window: WindowFunction::BlackmanHarris2,
            };
            Some(SincFixedIn::<f32>::new(ratio, 2.0, params, INPUT_CHUNK, 1)?)
        };
        Ok(Self {
            inner,
            input_sample_rate,
            carry: Vec::with_capacity(INPUT_CHUNK * 2),
        })
    }

    /// Consomme un buffer mono Float32 au sample rate d'entree et pousse
    /// les echantillons resamples dans `out`. Le reste qui ne remplit pas
    /// un chunk est conserve pour le prochain appel.
    pub fn process(&mut self, input: &[f32], out: &mut Vec<f32>) -> Result<()> {
        let Some(inner) = self.inner.as_mut() else {
            // Pas de resampling a faire.
            out.extend_from_slice(input);
            return Ok(());
        };

        self.carry.extend_from_slice(input);

        while self.carry.len() >= INPUT_CHUNK {
            let chunk: Vec<f32> = self.carry.drain(..INPUT_CHUNK).collect();
            let frames = inner.process(&[chunk], None)?;
            if let Some(first) = frames.into_iter().next() {
                out.extend(first);
            }
        }
        Ok(())
    }

    /// Flush final : traite les derniers echantillons en paddant avec du silence
    /// pour atteindre un chunk complet. A appeler a la fin de l'enregistrement.
    pub fn flush(&mut self, out: &mut Vec<f32>) -> Result<()> {
        let Some(inner) = self.inner.as_mut() else {
            out.extend(self.carry.drain(..));
            return Ok(());
        };
        if self.carry.is_empty() {
            return Ok(());
        }
        let missing = INPUT_CHUNK.saturating_sub(self.carry.len());
        let mut chunk = std::mem::take(&mut self.carry);
        chunk.resize(INPUT_CHUNK, 0.0);
        let frames = inner.process(&[chunk], None)?;
        if let Some(first) = frames.into_iter().next() {
            // On coupe la queue correspondant au silence de padding.
            let extra = ((missing as f64) * (TARGET_SAMPLE_RATE as f64)
                / (self.input_sample_rate as f64))
                .round() as usize;
            let keep = first.len().saturating_sub(extra);
            out.extend_from_slice(&first[..keep]);
        }
        Ok(())
    }
}

/// Mixdown multi-canal interleaved vers mono par moyenne simple.
pub fn interleaved_to_mono(input: &[f32], channels: u16, out: &mut Vec<f32>) {
    if channels <= 1 {
        out.extend_from_slice(input);
        return;
    }
    let ch = channels as usize;
    let frames = input.len() / ch;
    out.reserve(frames);
    for frame in input.chunks_exact(ch) {
        let sum: f32 = frame.iter().sum();
        out.push(sum / ch as f32);
    }
}

/// Convertit Float32 [-1, 1] vers Int16 avec clipping (VoiceInk L703-707).
pub fn float_to_int16(input: &[f32], out: &mut Vec<i16>) {
    out.reserve(input.len());
    for &s in input {
        let scaled = (s * 32767.0).clamp(-32768.0, 32767.0);
        out.push(scaled as i16);
    }
}
