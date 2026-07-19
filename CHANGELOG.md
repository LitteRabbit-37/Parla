# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.5.0] - 2026-07-19

Feature release: keyboard shortcuts to switch Power Mode profiles while
recording (issue #8), plus a fix so the Parakeet GPU build actually runs
inference on the GPU instead of silently falling back to CPU (issue #9).

### Added
- Power Mode profile selection shortcuts (issue #8), mirroring VoiceInk `MiniRecorderShortcutManager`. While the mini-recorder is visible, `Alt+1`..`Alt+9` and `Alt+0` select the 1st..10th enabled profile (in stored order) and apply it live. The low-level keyboard hook only arms these shortcuts while recording and only for as many profiles as actually exist, and it swallows the keystroke so the digit is never typed into the focused app. It captures `Alt` alone: `AltGr+digit` (AZERTY, where Ctrl is down) and any Ctrl/Shift/Win combo pass through, and a digit that already matches the user's configured record hotkey is not hijacked. Manual selection preserves the original pre-recording baseline, so the end-of-dictation restore still returns to the settings from before recording, not to the previously applied profile. `Alt+N` hint badges are shown next to each enabled profile in the Power Mode settings list and in the mini-recorder profile popover (en/fr/es).

### Fixed
- Parakeet GPU builds now actually run inference on the GPU (issue #9). The engine loaded Parakeet with `from_pretrained(dir, None)`, which parakeet-rs resolves to `ExecutionProvider::Cpu` (the enum default), so building with `cuda-onnx` or `directml-onnx` only exposed the provider variant and flipped the cosmetic UI label while inference kept running on CPU. The execution provider is now passed explicitly, gated by feature (Cuda for `cuda-onnx`, DirectML for `directml-onnx`, CPU otherwise), and each GPU provider falls back to CPU automatically if initialization fails.

## [0.4.1] - 2026-07-14

UX release: the AI Models screen is reworked to match VoiceInk so
choosing a cloud provider is no longer confusing (issue #6).

### Changed
- AI Models page redesigned to match VoiceInk `ModelManagementView`, fixing the confusing cloud-provider flow (issue #6). The three source tiles (Local / Parakeet / Cloud) are replaced by a "Default Model" header showing the active model, a Recommended / Local / Cloud pill filter (purely visual), and a unified list of model cards. Each card exposes a three-state action button mirroring VoiceInk `CloudModelCardView`: "Configure" (cloud API key entered inline) becomes "Set as Default" once the key is verified, then "Default Model" once active. Verifying a cloud key now only stores the key; the provider is applied only when the user explicitly clicks "Set as Default", so nothing silently falls back to Local and the active model stays named in the header and highlighted in the list. Whisper and Parakeet models share the Local filter and the `.bin` import moves to a card at the end of that list. No backend change: Whisper activation composes the existing `set_selected_whisper_model` + `set_transcription_kind` commands. The old `CloudProvidersPanel`, `ModelsPanel`, `ParakeetPanel` and `TranscriptionSourcePanel` are removed.

## [0.4.0] - 2026-06-27

Feature release: optional audio cues so the recording state is audible
without watching the screen (issue #5).

### Added
- Sound feedback cues on recording start, paste and cancel (issue #5), mirroring VoiceInk `SoundManager.swift`. Three short cues reused from VoiceInk (`recstart` / `recstop` / `esc`) are embedded in the binary and played via `rodio` on the cpal output device at reduced volume (0.4 / 0.4 / 0.3). A new "Sound feedback" toggle in the Recording settings card (`sound_feedback_enabled`, on by default) gates them, persisted like the other recording settings and translated in en/fr/es. The start cue plays before microphone capture begins so it never leaks into the recording; the stop cue fires at the paste site in the pipeline (like VoiceInk it doubles as a "text inserted" confirmation, and is therefore skipped for empty transcriptions); the esc cue plays on cancel. To keep the start cue audible when "Mute system audio during recording" is enabled, the system mute now engages 300 ms after start (deferred and cancellable, so a tap-then-stop shorter than that never mutes).

## [0.3.2] - 2026-06-08

Bug-fix release: Dashboard error on "Last 7 days" / "Last 30 days" filters
(issue #4) and autostart now leaves Parla in the tray instead of opening
the main window.

### Fixed
- Dashboard "Model performance" now works for the "Last 7 days" and "Last 30 days" filters. The frontend was sending `last_7_days` / `last_30_days` while the Rust `MetricsPeriod` enum, serialized with `rename_all = "snake_case"`, expects `last7_days` / `last30_days` (heck's snake_case does not insert an underscore between a letter and an adjacent digit). Frontend variant strings aligned on the serde output. `this_year` and `all_time` were unaffected.
- Autostart at Windows logon now starts Parla in tray-only mode (keyboard hook active, main window hidden) instead of popping the main window on every boot. The autostart plugin registers the Run entry with a `--minimized` flag, the main window is created hidden in `tauri.conf.json`, and the setup hook shows it on demand when the flag is absent (manual launch). A one-shot migration on first 0.3.2 boot re-writes the Run registry entry for users who had autostart enabled in 0.3.0/0.3.1, so they get the new behavior at the next Windows logon without having to toggle autostart manually.

## [0.3.1] - 2026-05-13

Bug-fix release for the custom recording shortcut (issue #3).

### Fixed
- Custom shortcut recorder now ignores standalone modifier keypresses (Ctrl, Shift, Alt). Pressing Ctrl alone used to be captured as "Ctrl + Ctrl" because WebView2 emits the generic VK codes (0x10/0x11/0x12) which were missing from the recorder's modifier filter, so the modifier passed through as if it were a final key. With those VKs added, a combination like Ctrl + F can now be recorded correctly.
- Custom shortcut recorder now rejects standalone text-producing keys (letters, digits, Space, ...) and shows an inline error. A bare key like F would otherwise trigger the recorder on every keystroke in any text field. Standalone keys are still allowed for non-textual keys (F1-F24, Pause, PrintScreen, Insert, Delete, Home, End, PageUp/Down).
- Recorded custom combination is now editable: the displayed combo in Settings is a clickable button that re-opens the recorder. Previously, once a combination was captured, the only way to change it was to switch back to a modifier-only option or reset all hotkey settings.

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
