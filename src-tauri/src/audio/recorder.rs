// Recorder WASAPI streaming vers WAV 16 kHz mono Int16 + callback chunks.
//
// Reference VoiceInk : VoiceInk/CoreAudioRecorder.swift
// - AUHAL + ExtAudioFile en mac, remplace par cpal + hound ici
// - Pipeline : callback device -> mono mixdown -> resample vers 16k -> Int16 clamp
//   -> ecriture WAV + emission chunk pour streaming providers
//
// Contrainte cpal : Stream est `!Send` sur certaines plateformes (WASAPI inclus).
// Le thread worker cree donc le Stream lui-meme, le garde sur sa pile, et lit les
// echantillons via un channel mpsc alimente par le callback du stream.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use cpal::traits::{DeviceTrait, StreamTrait};
use cpal::SampleFormat;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use tracing::{debug, error, info, warn};

use super::device::{default_input_device, find_input_device_by_name};
use super::meters::compute_rms_peak;
use super::resampler::{float_to_int16, interleaved_to_mono, MonoResampler};
use super::{CHUNK_FRAMES, TARGET_CHANNELS, TARGET_SAMPLE_RATE};

/// Niveaux audio en dB publies vers l'UI.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct AudioMeter {
    pub rms_db: f32,
    pub peak_db: f32,
}

impl Default for AudioMeter {
    fn default() -> Self {
        Self {
            rms_db: -160.0,
            peak_db: -160.0,
        }
    }
}

/// Chunk audio Int16 mono 16 kHz expose au consommateur (streaming providers).
pub type AudioChunk = Vec<i16>;

/// Configuration d'un enregistrement.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecorderConfig {
    /// Nom du peripherique. `None` = peripherique par defaut.
    pub device_name: Option<String>,
    /// Chemin de sortie du WAV (cree/ecrase au demarrage).
    pub output_path: PathBuf,
}

/// Callback pour reception des chunks audio en temps reel (streaming providers).
pub type ChunkCallback = Arc<dyn Fn(AudioChunk) + Send + Sync>;

/// Handle sur un enregistrement actif.
pub struct RecorderHandle {
    stop_flag: Arc<AtomicBool>,
    meter: Arc<Mutex<AudioMeter>>,
    thread: Option<std::thread::JoinHandle<Result<()>>>,
    output_path: PathBuf,
}

impl RecorderHandle {
    pub fn current_meter(&self) -> AudioMeter {
        *self.meter.lock()
    }

    /// Arrete l'enregistrement et attend la finalisation du WAV.
    pub fn stop(mut self) -> Result<PathBuf> {
        self.stop_flag.store(true, Ordering::SeqCst);
        if let Some(handle) = self.thread.take() {
            match handle.join() {
                Ok(Ok(())) => {}
                Ok(Err(e)) => return Err(e),
                Err(_) => return Err(anyhow!("le thread d'enregistrement a panic")),
            }
        }
        Ok(self.output_path)
    }
}

pub struct AudioRecorder;

impl AudioRecorder {
    /// Demarre une capture audio. Retourne apres que le thread worker a initialise
    /// le stream, pour pouvoir remonter les erreurs de setup au caller.
    pub fn start(
        config: RecorderConfig,
        on_chunk: Option<ChunkCallback>,
    ) -> Result<RecorderHandle> {
        let stop_flag = Arc::new(AtomicBool::new(false));
        let meter = Arc::new(Mutex::new(AudioMeter::default()));

        let (ready_tx, ready_rx) = mpsc::channel::<Result<()>>();

        let worker_stop = stop_flag.clone();
        let worker_meter = meter.clone();
        let output_path = config.output_path.clone();
        let config_clone = config.clone();

        let thread = std::thread::Builder::new()
            .name("parla-audio-worker".into())
            .spawn(move || -> Result<()> {
                run_worker(config_clone, on_chunk, worker_stop, worker_meter, ready_tx)
            })?;

        // Attends que le worker signale que le stream est pret (ou echec).
        match ready_rx.recv_timeout(Duration::from_secs(5)) {
            Ok(Ok(())) => Ok(RecorderHandle {
                stop_flag,
                meter,
                thread: Some(thread),
                output_path,
            }),
            Ok(Err(e)) => {
                // Le worker s'est arrete en erreur. On attend son join pour remonter l'erreur finale.
                let _ = thread.join();
                Err(e)
            }
            Err(_) => {
                stop_flag.store(true, Ordering::SeqCst);
                let _ = thread.join();
                Err(anyhow!("timeout d'initialisation du recorder audio"))
            }
        }
    }
}

fn run_worker(
    config: RecorderConfig,
    on_chunk: Option<ChunkCallback>,
    stop_flag: Arc<AtomicBool>,
    meter: Arc<Mutex<AudioMeter>>,
    ready_tx: mpsc::Sender<Result<()>>,
) -> Result<()> {
    // 1. Resolution peripherique.
    let device = match &config.device_name {
        Some(name) => find_input_device_by_name(name)
            .ok_or_else(|| anyhow!("peripherique audio introuvable: {name}")),
        None => {
            default_input_device().ok_or_else(|| anyhow!("aucun peripherique audio par defaut"))
        }
    };
    let device = match device {
        Ok(d) => d,
        Err(e) => {
            let _ = ready_tx.send(Err(anyhow!("{e}")));
            return Err(e);
        }
    };
    let device_name = device.name().unwrap_or_else(|_| "?".into());

    let supported = match device.default_input_config() {
        Ok(c) => c,
        Err(e) => {
            let e = anyhow!("default_input_config: {e}");
            let _ = ready_tx.send(Err(anyhow!("{e}")));
            return Err(e);
        }
    };
    let input_sample_rate = supported.sample_rate().0;
    let input_channels = supported.channels();
    let sample_format = supported.sample_format();

    info!(
        device = %device_name,
        sample_rate = input_sample_rate,
        channels = input_channels,
        format = ?sample_format,
        "Demarrage de l'enregistrement audio"
    );

    // 2. WAV writer 16 kHz mono Int16 (ref VoiceInk outputFormat L380-390).
    if let Some(parent) = config.output_path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    let wav_spec = hound::WavSpec {
        channels: TARGET_CHANNELS,
        sample_rate: TARGET_SAMPLE_RATE,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = match hound::WavWriter::create(&config.output_path, wav_spec) {
        Ok(w) => w,
        Err(e) => {
            let e = anyhow!("creation WAV {}: {e}", config.output_path.display());
            let _ = ready_tx.send(Err(anyhow!("{e}")));
            return Err(e);
        }
    };

    // 3. Channel callback audio -> worker loop.
    let (sample_tx, sample_rx) = mpsc::channel::<Vec<f32>>();

    let stream = match build_input_stream(&device, sample_format, sample_tx) {
        Ok(s) => s,
        Err(e) => {
            let _ = ready_tx.send(Err(anyhow!("{e}")));
            return Err(e);
        }
    };

    if let Err(e) = stream.play() {
        let e = anyhow!("stream play: {e}");
        let _ = ready_tx.send(Err(anyhow!("{e}")));
        return Err(e);
    }

    // Setup OK, on notifie le caller.
    let _ = ready_tx.send(Ok(()));

    // 4. Pipeline de traitement.
    let mut resampler = MonoResampler::new(input_sample_rate)?;
    let mut mono_buf: Vec<f32> = Vec::with_capacity(4096);
    let mut resampled: Vec<f32> = Vec::with_capacity(4096);
    let mut int16_buf: Vec<i16> = Vec::with_capacity(4096);
    let mut chunk_accum: Vec<i16> = Vec::with_capacity(CHUNK_FRAMES * 4);
    let mut last_meter_update = std::time::Instant::now();

    loop {
        let chunk = match sample_rx.recv_timeout(Duration::from_millis(100)) {
            Ok(c) => c,
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if stop_flag.load(Ordering::SeqCst) {
                    break;
                }
                continue;
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        };

        if stop_flag.load(Ordering::SeqCst) {
            // On traite quand meme ce buffer puis on sort.
        }

        // Metering sur le buffer Float32 brut (avant resampling).
        let (rms_db, peak_db) = compute_rms_peak(&chunk);
        if last_meter_update.elapsed() >= Duration::from_millis(33) {
            *meter.lock() = AudioMeter { rms_db, peak_db };
            last_meter_update = std::time::Instant::now();
        } else {
            let mut m = meter.lock();
            m.peak_db = m.peak_db.max(peak_db);
        }

        mono_buf.clear();
        interleaved_to_mono(&chunk, input_channels, &mut mono_buf);

        resampled.clear();
        if let Err(e) = resampler.process(&mono_buf, &mut resampled) {
            warn!("Resampler err: {e}");
            continue;
        }

        int16_buf.clear();
        float_to_int16(&resampled, &mut int16_buf);

        for &s in &int16_buf {
            writer.write_sample(s)?;
        }

        if let Some(cb) = on_chunk.as_ref() {
            chunk_accum.extend_from_slice(&int16_buf);
            while chunk_accum.len() >= CHUNK_FRAMES {
                let out_chunk = chunk_accum.drain(..CHUNK_FRAMES).collect::<Vec<_>>();
                cb(out_chunk);
            }
        }

        if stop_flag.load(Ordering::SeqCst) {
            break;
        }
    }

    // 5. Finalisation : flush resampler, ecrit le reste, ferme le WAV.
    drop(stream); // stop du stream wasapi avant flush

    resampled.clear();
    if let Err(e) = resampler.flush(&mut resampled) {
        warn!("Resampler flush: {e}");
    }
    int16_buf.clear();
    float_to_int16(&resampled, &mut int16_buf);
    for &s in &int16_buf {
        writer.write_sample(s)?;
    }
    if let Some(cb) = on_chunk.as_ref() {
        chunk_accum.extend_from_slice(&int16_buf);
        if !chunk_accum.is_empty() {
            let final_chunk: Vec<i16> = std::mem::take(&mut chunk_accum);
            cb(final_chunk);
        }
    }
    writer.finalize()?;

    *meter.lock() = AudioMeter::default();
    debug!("Worker audio termine");
    Ok(())
}

fn build_input_stream(
    device: &cpal::Device,
    format: SampleFormat,
    tx: mpsc::Sender<Vec<f32>>,
) -> Result<cpal::Stream> {
    let config = device
        .default_input_config()
        .context("default_input_config failed")?;
    let stream_config: cpal::StreamConfig = config.clone().into();

    let err_fn = |e| error!("Erreur stream audio: {e}");

    let tx_f32 = tx.clone();
    let tx_i16 = tx.clone();
    let tx_u16 = tx;
    let stream = match format {
        SampleFormat::F32 => device.build_input_stream(
            &stream_config,
            move |data: &[f32], _| {
                let _ = tx_f32.send(data.to_vec());
            },
            err_fn,
            None,
        )?,
        SampleFormat::I16 => device.build_input_stream(
            &stream_config,
            move |data: &[i16], _| {
                let buf: Vec<f32> = data.iter().map(|&s| s as f32 / 32768.0).collect();
                let _ = tx_i16.send(buf);
            },
            err_fn,
            None,
        )?,
        SampleFormat::U16 => device.build_input_stream(
            &stream_config,
            move |data: &[u16], _| {
                let buf: Vec<f32> = data
                    .iter()
                    .map(|&s| (s as f32 - 32768.0) / 32768.0)
                    .collect();
                let _ = tx_u16.send(buf);
            },
            err_fn,
            None,
        )?,
        other => return Err(anyhow!("format audio non supporte: {other:?}")),
    };
    Ok(stream)
}
