// Module audio - capture WASAPI via cpal, metering, resampling, ecriture WAV.
//
// Reference VoiceInk : VoiceInk/CoreAudioRecorder.swift
// - Format cible : 16 kHz mono PCM Int16 packed
// - Output WAV + callback chunk simultanes pour streaming providers
// - Resampling + mixdown mono en temps reel
// - Metering RMS/peak en dB pour l'UI (60 Hz cote VoiceInk, 30 Hz ici est suffisant)

pub mod device;
pub mod feedback;
pub mod meters;
pub mod mute;
pub mod recorder;
pub mod resampler;

pub use device::{list_input_devices, AudioDeviceInfo};
pub use recorder::{AudioMeter, AudioRecorder, RecorderConfig, RecorderHandle};

/// Sample rate cible impose par whisper.cpp et la majorite des modeles de transcription.
pub const TARGET_SAMPLE_RATE: u32 = 16_000;

/// Nombre de canaux de sortie (mono pour Whisper).
pub const TARGET_CHANNELS: u16 = 1;

/// Taille des chunks emis vers le consommateur streaming (20 ms = 320 frames a 16 kHz).
pub const CHUNK_FRAMES: usize = 320;
