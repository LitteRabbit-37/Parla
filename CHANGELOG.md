# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

Bug-fix release: the clipboard crash reported in issue #13, plus the VoiceInk
2.13 catch-up items that were left out of 0.6.0 (Unicode word boundaries,
trigger-word priority, refreshed enhancement prompts, cloud model ids).

### Fixed
- Crash `STATUS_HEAP_CORRUPTION` (0xc0000374) after a dictation when an image was on the clipboard (issue #13). The clipboard backup enumerated every format and treated each handle as global memory; Windows synthesizes `CF_BITMAP` from `CF_DIB` whenever an image is copied, so an `HBITMAP` ended up in `GlobalSize`, which is undefined behaviour and kills the process on some machines. The backup and the restore now skip every non-`HGLOBAL` format: `CF_BITMAP`, `CF_PALETTE`, `CF_METAFILEPICT`, `CF_ENHMETAFILE`, `CF_OWNERDISPLAY`, the `CF_DSP*` variants, the private range (0x200-0x2FF) and the GDI object range (0x300-0x3FF). Images are still preserved through `CF_DIB` / `CF_DIBV5` / PNG and Windows re-synthesizes `CF_BITMAP` on restore. Present since 0.1.0.
- Word replacements no longer match inside words containing non-ASCII letters (VoiceInk commit 491f581): a rule `ERN -> EAN` used to turn "vergrößern" into "vergrößEAN" because the boundary class was `[a-zA-Z0-9]`. Word characters are now Unicode letters, marks and digits, with the non-spaced scripts (Han, Hiragana, Katakana, Hangul, Thai) exempted so a Latin term flush against CJK text still matches.
- The clipboard is restored even when the Ctrl+V simulation fails (VoiceInk `CursorPaster`, commit 8ce493d); previously a failed paste left the transcript in the clipboard and dropped the user's previous content.
- Trigger words are resolved like VoiceInk `ModeTriggerWordDetectionService`: candidates from all prompts are sorted by trigger length first, then prompt order, so "hey claude" on one prompt beats "hey" on another. Trigger words are also normalized on save (trimmed, empty entries dropped, case-insensitive duplicates removed).

### Changed
- Enhancement prompts rewritten to VoiceInk 2.13 (`Core/Enhancement/AIPrompts.swift`, `PromptTemplates.swift`, commits 5706e83 and eda5ccb): the system template is now the structured `<SYSTEM_INSTRUCTIONS>` block (task, rules, context rules, task instructions, worked examples, output requirements) with explicit self-correction handling, spoken number / currency / date normalization, paragraph and list rules and a prompt-injection guard. The predefined Default and Assistant prompts are refreshed automatically on launch (they are read-only in Parla, like VoiceInk's predefined prompts), user-created prompts are untouched. The optional Chat, Email and Rewrite templates use the VoiceInk 2.13 texts; Rewrite is a full instruction block like in VoiceInk.
- Cloud transcription model ids aligned on VoiceInk 2.13: AssemblyAI `universal-3-5-pro` (batch + realtime, `mode=balanced`, up to 1000 key terms) and `universal-2` (batch only, 200 key terms), following LLMkit 95b29c2 which no longer sends a prompt; Soniox `stt-async-v5` / `stt-rt-v5`; Cartesia `ink-2` (English only). Previously selected ids keep working: the old AssemblyAI ids are mapped to their successor, the Soniox v4 ids are passed through. Gemini transcription keeps the 2.5 models for now: VoiceInk's `gemini-3.5-transcribe` uses a different API (Files upload + `interactions`), which will come with Gemini streaming.
- ElevenLabs streaming sends `no_verbatim=true` (VoiceInk fix for #808, disfluencies removed like in batch) and the custom vocabulary as `keyterms` (trimmed, 20 characters max, 50 terms), matching LLMkit's `ElevenLabsStreamingClient`.


## [0.6.0] - 2026-09-16

Feature release catching up with VoiceInk 2.x on shortcuts and the recorder
pill: additional global shortcuts including "paste last transcription"
(issue #11), a tray menu that shows the configured shortcuts, live text in
the pill while you speak, and refreshed LLM model lists (issue #12).

### Added
- Additional global shortcuts (issue #11), mirroring VoiceInk 2.x `Features/Shortcuts` (`ShortcutAction.globalUtilityActions`, `RecordingShortcutManager.handleGlobalShortcut`, `LastTranscriptionService`): "Paste last transcription (original)", "Paste last transcription (enhanced)", "Retry last transcription", "Open history" and a customizable "Cancel recording" shortcut, all unset by default like VoiceInk. Plus a Parla-specific "Copy last transcription" shortcut: VoiceInk only exposes copying as a menu accelerator (Shift+Cmd+C, active while the menu is open), Parla makes it a real global shortcut. Shortcuts are captured with the existing combo recorder, validated like VoiceInk `ShortcutValidator` (no bare text key without a modifier except F-keys, Alt+digit reserved for Power Mode profile selection, no two actions on the same keys) and the matched key events are swallowed so the target app never receives them. Paste actions wait 150 ms before Ctrl+V like VoiceInk and target the window that had focus when the shortcut was pressed. Retry re-transcribes the last recording's audio with the current source (enhancement included), copies the result to the clipboard and adds a new history entry; it refuses while a recording is in progress. Stored under the existing `hotkey` settings key (`actions` sub-object, older configs load with everything unset).
- Custom cancel-recording shortcut (VoiceInk `RecorderPanelShortcutManager`): when set, it replaces the double-Escape while the recorder is visible; the reset button restores double-Escape. On the first Escape of a double-Escape cancel, the pill now shows "Press Esc again to cancel" once (VoiceInk `showEscapeConfirmationHintIfNeeded`, key `escape_cancel_hint_shown`, reset with the cancel shortcut).
- Tray menu rebuilt on the VoiceInk 2.x `MenuBarView` layout: "Open Parla", "Toggle recording" (shows the primary shortcut), a "Power Mode: <active>" submenu listing enabled profiles with a check mark plus "Manage Power Mode" / "Manage AI models", an "Audio input" submenu to pick the microphone (system default or a specific device, persisted and used by hotkey recordings), "Retry last transcription", "Copy last transcription" and "History" with their configured shortcuts displayed in the accelerator column, a "Launch at login" check item, then Settings / Check for updates / Quit. Before the onboarding is completed the menu only offers "Complete onboarding" and "Quit". Labels follow the UI language (en/fr/es). "Toggle recording" now runs the same full cycle as the hotkey (pill, Power Mode, screen context, mute) instead of the bare manual recorder, and a recording started from the tray is stopped by the next press of the recording shortcut.
- Live text in the recorder pill while you speak (VoiceInk `MiniRecorderView.hasLiveTranscript`, `LiveTranscriptView`, commit 42f7ec8): with a streaming cloud provider (ElevenLabs, Deepgram, Mistral, Soniox, Speechmatics, AssemblyAI, Cartesia, xAI) the pill widens from 184 to 300 px and shows the running transcript above the control bar, scrolling as text arrives. New "Live text display" toggle in Settings (`show_live_transcript`, on by default). Previously the text was only revealed after the recording stopped. Local Whisper / Parakeet stay batch-only (VoiceInk's local streaming relies on FluidAudio, which has no Windows equivalent yet).
- Persisted microphone selection (`selected_input_device`), shared by the Audio Input panel and the tray submenu, used whenever a recording starts without an explicit device (hotkey, tray). Falls back to the system default when the device is not connected.
- Recorder pill controls aligned on VoiceInk 2.x `RecorderComponents.swift`: the left button is now a record / stop button (grey when ready, red while recording, inactive during processing, `RecorderRecordButton`) running the same full cycle as the hotkey; the prompt selector that VoiceInk removed from its pill (commit d59dfde) is gone too, the prompt is carried by the Power Mode profile. The right button keeps the Power Mode popover, now opened on hover as well as on click and closed 250 ms after leaving both the button and the popover (`RecorderModeButton.syncPopoverVisibility`), with clickable rows that apply the profile (`ModePopover`).
- The pill popover is rendered in its own always-on-top, non-activating window ("recorder-popover", pre-created hidden with the pill), like VoiceInk's NSPopover. The pill window is no longer resized to make room for the popover, which removes the visual jumps seen when opening it.
- Cloud transcription timeout setting on the AI Models page (VoiceInk `CloudTranscriptionSettings`, commit 1bab779): 10 s to 30 min, default 30 s. The batch HTTP client used to be hard-coded to 120 s.

### Changed
- LLM enhancement model lists aligned on VoiceInk 2.13 `AIService.availableModels` (issue #12): OpenAI gpt-5.6-luna (default), gpt-5.6-terra, gpt-5.6-sol, gpt-5.5, gpt-5.4 family and gpt-4.1 family; Gemini gemini-3.7-flash (default), 3.6-flash, 3.5-flash-lite, 3.5-flash, 3.1-pro-preview, 3.1-flash-lite, 2.5-flash-lite; Anthropic claude-sonnet-5 (default) and claude-haiku-4-5; Mistral small (default), medium, large; Cerebras gpt-oss-120b, gemma-4-31b, zai-glm-4.7; Groq gpt-oss-120b / 20b. Reasoning parameters follow VoiceInk `ReasoningConfig.swift` (no reasoning for GPT-5.x, "low" thinking for Gemini 3.7 Flash and 3.1 Pro, "minimal" for the other Gemini 3.x, `reasoning_format: hidden` for Cerebras gpt-oss, `include_reasoning: false` for Groq gpt-oss). A previously selected model that is no longer listed stays selectable as "(current)" in the dropdown.
- Sound feedback volumes lowered to VoiceInk 2.x values (start / stop 0.3, cancel 0.2, commit 81157a5) instead of 0.4 / 0.4 / 0.3.
- The recorder pill no longer shows the final streaming text during the "Transcribing" step; like VoiceInk it shows the processing badge once the recording stops.


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
