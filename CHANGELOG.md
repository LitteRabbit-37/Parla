# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- Dictation language selector on the AI Models page, filtered by the languages supported by the active model (Whisper, Parakeet or cloud). Replaces the previously inert `whisper_language` store key with a real UI: each catalog entry now declares its supported ISO codes (per-provider lists ported from VoiceInk `LanguageDictionary.swift` and `Cloud/*Provider.swift`). "Auto-detect" surfaces explicitly when the active model supports it; the pipeline coalesces it back to `None` before calling whisper or any cloud provider. Persisted under the legacy key `whisper_language` (Power Mode profiles already snapshot/restore it).
- Speed and accuracy ratings on every model card (Whisper, Parakeet and cloud), shown as five colored dots (green / yellow / orange / red) plus a numeric score. Values mirror VoiceInk `TranscriptionModelRegistry.swift` and the per-provider speed/accuracy declared in each `Cloud/*Provider.swift`
- Two installer variants produced by the CI release workflow: CPU (canonical, auto-update capable) and CUDA (NVIDIA GPU acceleration via cuda-whisper + cuda-llama + cuda-onnx features)
- `cuda` Cargo meta-feature enabling all three CUDA sub-features at once
- Auto-updater via GitHub Releases with `tauri-plugin-updater`
- CI release workflow signing artifacts with `TAURI_SIGNING_PRIVATE_KEY`
- Internationalization (i18n) infrastructure with English, French and Spanish locales
- Language selector in the Settings panel
- Prompt detection from trigger words in transcripts (parity with VoiceInk `PromptDetectionService`)
- Multi-format clipboard backup/restore on Windows (images, files, HTML, RTF)
- `transcription/engine.rs` module mirroring VoiceInk `VoiceInkEngine.swift`
- HTTP timeout helpers for all batch cloud providers + WebSocket handshake timeout for streaming providers
- URL validator for user-configurable endpoints (Custom OpenAI-compat, Ollama)
- `source:changed` event replacing UI polling in the AI Models panel
- Unit tests for hotkeys state machine, cloud catalog, enhancement helpers and prompt detection (69 tests total)

### Changed
- Restrictive CSP on the webview (`default-src 'self'` baseline with targeted allowances)
- `parking_lot::Mutex` unified across the backend (previously one site used `std::sync::Mutex`)
- `TranscriptionSource` type extracted to `src/lib/tauri.ts` (was duplicated in two panels)
- README rewritten in English with VoiceInk-inspired structure

### Removed
- Unused `thiserror` dependency

### Fixed
- Toggle mode hotkey never ending a recording via press (now sets hands-free on release, matching VoiceInk)
- Cloud transcription providers hanging indefinitely on network failure (120s timeout + 15s connect timeout)
- Word replacement now applies longest rules first, so a shorter rule no longer cannibalizes a more specific one (e.g. "good" no longer breaks "good morning"). Variants inside a single CSV rule are sorted longest-first as well. Boundary regex switched from `\b` to lookarounds `(?<![a-zA-Z0-9])...(?![a-zA-Z0-9])` so punctuation acts as a word boundary and `_` is no longer treated as a word character. Mirrors VoiceInk commit 620a843.

## [0.1.0] - 2026-04-17

Initial internal release. First installer.
