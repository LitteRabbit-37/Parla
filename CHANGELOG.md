# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.3.0] - 2026-05-10

Major catch-up with VoiceInk: three new cloud STT providers, a real
dictation language picker, per-model speed/accuracy ratings, and a
per-model performance dashboard.

### Added
- xAI Grok speech-to-text cloud provider. Batch via `POST https://api.x.ai/v1/stt` (multipart with the `file` part last, per xAI docs) and real-time streaming via WebSocket `wss://api.x.ai/v1/stt` with raw 16-bit LE PCM frames at 16 kHz, `endpointing=800` (matches LLMkit `XAIStreamingClient`). API key stored under the keychain user `xAIAPIKey`. New `grok-stt` entry in the cloud catalog with the 25 languages declared by VoiceInk `XAIProvider.swift` plus auto-detect.
- Cartesia Ink Whisper streaming-only cloud provider. WebSocket `wss://api.cartesia.ai/stt/websocket` with `model`, `language`, `encoding=pcm_s16le`, `sample_rate=16000` and `cartesia_version=2026-03-01` query params, authenticated via the `X-API-Key` header (no Bearer). No auto-detect: when the user picks "Auto-detect", Parla falls back to `en` (matches LLMkit `CartesiaStreamingClient`). `finalize` flush + `done` close. New `ink-whisper` catalog entry with 100 languages from VoiceInk `CartesiaProvider.swift`. API key stored under `cartesiaAPIKey`.
- AssemblyAI Universal cloud provider with batch and real-time streaming. Two catalog entries: `universal-3-pro` (highest accuracy with auto-detect, fallback to `universal-2`) and `universal-streaming` (faster Universal-2). Batch follows the upload + create + poll flow (`POST /v2/upload` -> `POST /v2/transcript` -> `GET /v2/transcript/{id}` until `status=completed`, with `speech_models` mapping, `language_code` or `language_detection`, and `keyterms_prompt` for the user dictionary). Streaming via WebSocket `wss://streaming.assemblyai.com/v3/ws` with model-specific query params (Universal-3 Pro vs Universal-Streaming-English/Multilingual selected from the active dictation language), 1.6 kB chunk buffering before sending (AssemblyAI minimum), `Begin` handshake, `Turn` event accumulation that only commits formatted turns or new turn orders (mirrors LLMkit `lastCommittedTurnOrder`), and `Terminate` flush on commit. Custom vocabulary normalized identically to LLMkit (trim, max 50 chars, max 6 words, dedup case-insensitive, capped at 100 entries). API key stored under `assemblyAIAPIKey`. Languages: 6 + auto-detect from VoiceInk `AssemblyAIProvider.swift` realtime list.
- Dictation language selector on the AI Models page, filtered by the languages supported by the active model (Whisper, Parakeet or cloud). Replaces the previously inert `whisper_language` store key with a real UI: each catalog entry now declares its supported ISO codes (per-provider lists ported from VoiceInk `LanguageDictionary.swift` and `Cloud/*Provider.swift`). "Auto-detect" surfaces explicitly when the active model supports it; the pipeline coalesces it back to `None` before calling whisper or any cloud provider. Persisted under the legacy key `whisper_language` (Power Mode profiles already snapshot/restore it).
- Speed and accuracy ratings on every model card (Whisper, Parakeet and cloud), shown as five colored dots (green / yellow / orange / red) plus a numeric score. Values mirror VoiceInk `TranscriptionModelRegistry.swift` and the per-provider speed/accuracy declared in each `Cloud/*Provider.swift`.
- Per-model performance dashboard on the Dashboard page. Shows session count, average audio duration, average processing time and a real-time speed factor (`avg audio / avg processing`) for each transcription model used in the selected period (last 7 days / last 30 days / this year / all time). A second grid shows the average enhancement duration per LLM. Mirrors VoiceInk `Views/Metrics/ModelPerformancePanel.swift`. Aggregation runs as a single SQL `GROUP BY` over the existing `transcriptions` table, no extra `SessionMetric` store and no migration needed.
- Two installer variants produced by the CI release workflow: CPU (canonical, auto-update capable) and CUDA (NVIDIA GPU acceleration via cuda-whisper + cuda-llama + cuda-onnx features).
- `cuda` Cargo meta-feature enabling all three CUDA sub-features at once.
- Auto-updater via GitHub Releases with `tauri-plugin-updater`.
- CI release workflow signing artifacts with `TAURI_SIGNING_PRIVATE_KEY`.
- Internationalization (i18n) infrastructure with English, French and Spanish locales.
- Language selector in the Settings panel.
- Prompt detection from trigger words in transcripts (parity with VoiceInk `PromptDetectionService`).
- Multi-format clipboard backup/restore on Windows (images, files, HTML, RTF).
- `transcription/engine.rs` module mirroring VoiceInk `VoiceInkEngine.swift`.
- HTTP timeout helpers for all batch cloud providers + WebSocket handshake timeout for streaming providers.
- URL validator for user-configurable endpoints (Custom OpenAI-compat, Ollama).
- `source:changed` event replacing UI polling in the AI Models panel.
- Unit tests for hotkeys state machine, cloud catalog, enhancement helpers, prompt detection, word replacement, xAI / Cartesia / AssemblyAI streaming protocol handlers and AssemblyAI keyterms normalization (88 tests total).

### Changed
- Restrictive CSP on the webview (`default-src 'self'` baseline with targeted allowances).
- `parking_lot::Mutex` unified across the backend (previously one site used `std::sync::Mutex`).
- `TranscriptionSource` type extracted to `src/lib/tauri.ts` (was duplicated in two panels).
- README rewritten in English with VoiceInk-inspired structure.

### Removed
- Unused `thiserror` dependency.

### Fixed
- Toggle mode hotkey never ending a recording via press (now sets hands-free on release, matching VoiceInk).
- Cloud transcription providers hanging indefinitely on network failure (120s timeout + 15s connect timeout).
- Word replacement now applies longest rules first, so a shorter rule no longer cannibalizes a more specific one (e.g. "good" no longer breaks "good morning"). Variants inside a single CSV rule are sorted longest-first as well. Boundary regex switched from `\b` to lookarounds `(?<![a-zA-Z0-9])...(?![a-zA-Z0-9])` so punctuation acts as a word boundary and `_` is no longer treated as a word character. Mirrors VoiceInk commit 620a843.

## [0.1.0] - 2026-04-17

Initial internal release. First installer.
