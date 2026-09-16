<div align="center">
  <img src="assets/parla-icon.png" width="180" height="180" />
</div>
<br>
<div align="center">
  <h1>Parla</h1>
  <p>Voice to text app for Windows, inspired by VoiceInk for macOS. Transcribe what you say, almost instantly, locally or through the cloud.</p>
</div>
<br>
<div align="center">
  <table>
    <th><a href="https://github.com/LitteRabbit-37/Parla/issues/new/choose">Help&nbsp;&amp;&nbsp;Feedback</a></th>
    <td><a href="https://github.com/LitteRabbit-37/Parla/releases">Releases</a></td>
  </table>
</div>
<div align="center">
  <img alt="License" src="https://img.shields.io/badge/License-GPL%20v3-blue.svg">
  <img alt="Platform" src="https://img.shields.io/badge/platform-Windows%2010%2B-brightgreen">
  <img src="https://img.shields.io/github/downloads/LitteRabbit-37/Parla/total?color=%23CAAA3A">
  <img alt="GitHub Release" src="https://img.shields.io/github/v/release/LitteRabbit-37/Parla">
  <img alt="GitHub Repo stars" src="https://img.shields.io/github/stars/LitteRabbit-37/Parla?style=social">
</div>

<br>
<br>

> **Note: Parla is a Windows port / re-implementation inspired by [VoiceInk](https://github.com/Beingpax/VoiceInk) by Pax. All Swift / macOS-specific code is re-written from scratch in Rust + TypeScript. VoiceInk remains the behavioral reference - every feature decision in Parla is cross-checked with its VoiceInk counterpart.**

![Parla](assets/parla-app.png)

## Overview

### What is it?

**Parla** is a native Windows app that transcribes your voice to text and pastes it at your cursor, with a full pipeline that goes far beyond raw transcription:

- **Accurate transcription** via local Whisper (whisper.cpp), NVIDIA Parakeet (ONNX Runtime), or cloud providers.
- **Privacy-first by default** - you can run the whole pipeline (transcription + LLM enhancement) 100% offline.
- **Power Mode** - detects the foreground window and browser URL, then applies a pre-configured profile (which model, which LLM prompt, which dictionary) automatically.
- **Context aware** - the foreground window is captured + OCRed, and the extracted text is injected into the LLM enhancement prompt so the output matches what you were working on.
- **Global shortcuts** with configurable toggle, push-to-talk or hybrid mode, a double-Escape (or custom shortcut) to cancel a recording, plus additional shortcuts to copy, paste or retry the last transcription and open the history. The tray menu shows them next to each action.
- **Personal dictionary** of word replacements applied on every transcription (technical terms, product names, custom acronyms).
- **Prompt detection** via trigger words in your transcript - say "mail ..." and the Email prompt kicks in automatically.
- **Multilingual UI** - French, English, Spanish.
- **NVIDIA CUDA acceleration** for Whisper, Parakeet and local llama.cpp, with transparent CPU fallback.
- **Auto-updater** via GitHub Releases, so the app keeps itself in sync with new releases.

### Features

Compared to existing Windows dictation tools, Parla targets feature parity with VoiceInk on macOS:

- **Accurate transcription** with local (Whisper / Parakeet) or cloud (Groq, Deepgram, ElevenLabs, Mistral, Soniox, Speechmatics, Gemini, Custom OpenAI-compat) models.
- **Streaming transcription** for providers that support it (ElevenLabs Scribe v2, Deepgram nova-3, Mistral voxtral RT, Soniox stt-rt-v4, Speechmatics, AssemblyAI, Cartesia, xAI), with the text shown live in the recorder pill while you speak.
- **LLM enhancement** with an embedded llama.cpp (GGUF models with CUDA), Ollama local, plus all major cloud providers (Anthropic, OpenAI, Gemini, Mistral, Groq, Cerebras, OpenRouter) and local CLI templates (pi / claude / codex) or a custom PowerShell command.
- **Smart modes** (prompts) configurable per-profile: Default cleanup, Assistant, Email, Chat, Rewrite, or any custom prompt you define.
- **Multi-format clipboard backup** - if you had an image, a file, or rich text in your clipboard, it is restored byte-for-byte after paste.
- **Non-focus-stealing overlay** recorder pill that floats above all windows without capturing focus (Win32 `WS_EX_NOACTIVATE` + `WS_EX_TOOLWINDOW`).
- **History** stored in SQLite with CSV export and configurable retention.
- **Windows Credential Manager** for every API key - nothing is ever written to disk in plain text.

### Supported transcription models

| Source | Models | GPU acceleration |
|---|---|---|
| whisper.cpp (local) | tiny, base, small, medium, large-v2, large-v3, large-v3-turbo, large-v3-turbo-q5_0, custom `.bin` import | CUDA / cuBLAS |
| Parakeet (local) | parakeet-tdt-0.6b-v2 (EN), parakeet-tdt-0.6b-v3 (25+ langues) | CUDA EP, DirectML EP, CPU |
| VAD Silero | v5.1.2 (via `whisper-vad`) | CPU |
| Groq | whisper-large-v3-turbo | - |
| ElevenLabs | scribe_v1, scribe_v2 realtime | - |
| Deepgram | nova-3, nova-3-medical | - |
| Mistral | voxtral-mini-latest, voxtral-mini-transcribe-realtime-2602 | - |
| Soniox | stt-async-v4, stt-rt-v4 | - |
| Speechmatics | speechmatics-enhanced | - |
| Google Gemini | 2.5 Pro, 2.5 Flash, 3 Flash, 3.1 Pro | - |
| Custom | Any OpenAI-compatible endpoint | - |

### Supported LLM enhancement providers

| Provider | API key | Endpoint |
|---|---|---|
| llama.cpp embedded | No | In-process (GGUF models, CUDA optional) |
| Ollama local | No | `http://localhost:11434` (configurable) |
| Anthropic | Yes | `https://api.anthropic.com/v1/messages` |
| OpenAI | Yes | `https://api.openai.com/v1/chat/completions` |
| Google Gemini | Yes | `https://generativelanguage.googleapis.com/v1beta` |
| Mistral | Yes | `https://api.mistral.ai/v1/chat/completions` |
| Groq | Yes | `https://api.groq.com/openai/v1` |
| Cerebras | Yes | `https://api.cerebras.ai/v1` |
| OpenRouter | Yes | `https://openrouter.ai/api/v1` |
| Local CLI (PowerShell) | No | Local process (pi / claude / codex templates or custom) |
| Custom OpenAI-compat | Optional | User-configured URL (HTTPS enforced) |

### Prerequisites

- Windows 10 (22H2) or later
- A microphone
- (Optional) NVIDIA GPU with CUDA 12.x drivers for GPU-accelerated local transcription and LLM enhancement

## Get started

### Download

Head to the [releases page](https://github.com/LitteRabbit-37/Parla/releases) and grab `Parla_x.y.z_x64-setup.exe`. Works on every Windows 10 / 11 machine. Runs Whisper, Parakeet and llama.cpp on your CPU out of the box.

Auto-updater is built in: once installed, Parla checks `latest.json` on every start and offers to install new releases in place.

### GPU acceleration (NVIDIA CUDA)

The pre-built installer ships CPU-only because compiling whisper.cpp + llama.cpp + ggml-cuda does not fit in our public CI runners' budget. Users with an NVIDIA GPU who want CUDA acceleration (typically 3 to 10 times faster on large models) build Parla themselves from source - see [BUILDING.md](BUILDING.md#cuda-build) for the recipe. You will need the CUDA Toolkit 12.x installed plus Visual Studio 2022 with the C++ workload. The build takes 15 to 30 minutes on a reasonably recent desktop.

Once installed, Parla runs in the system tray. The default global shortcut for recording is configurable from the `Recorder` panel in the app.

### Build from source

See [BUILDING.md](BUILDING.md) for the detailed instructions. Quick version:

```bash
git clone https://github.com/LitteRabbit-37/Parla.git
cd Parla
npm install
npm run tauri dev
```

For a release build:

```bash
npm run tauri build
```

## Tech stack

- [**Tauri v2**](https://tauri.app/) desktop framework (Rust backend + WebView2 frontend)
- [**React 19**](https://react.dev/) + [**TypeScript**](https://www.typescriptlang.org/) + [**Vite**](https://vitejs.dev/) + [**Tailwind v4**](https://tailwindcss.com/) + [**shadcn/ui**](https://ui.shadcn.com/) for the UI
- [**whisper.cpp**](https://github.com/ggerganov/whisper.cpp) via [`whisper-rs`](https://github.com/tazz4843/whisper-rs) for local Whisper transcription
- [**parakeet-rs**](https://github.com/altunenes/parakeet-rs) (ONNX Runtime + TDT decoder) for local Parakeet transcription
- [**llama.cpp**](https://github.com/ggerganov/llama.cpp) via [`llama-cpp-2`](https://github.com/utilityai/llama-cpp-rs) for local LLM enhancement
- [**cpal**](https://github.com/RustAudio/cpal) for WASAPI audio capture
- [**xcap**](https://github.com/nashaofu/xcap) for screen / window capture + Windows `Media.Ocr` for OCR
- [**uiautomation**](https://github.com/leexgone/uiautomation-rs) for Chromium / Firefox / Edge URL extraction
- [**rusqlite**](https://github.com/rusqlite/rusqlite) (bundled) for the history database
- [**keyring**](https://github.com/hwchen/keyring-rs) with Windows Credential Manager backend for API keys
- [**react-i18next**](https://react.i18next.com/) for the UI translations (French, English, Spanish)

### Architecture

```
src-tauri/src/
  audio/           - WASAPI recorder + VAD Silero
  transcription/   - engine (VoiceInk parity), pipeline, Whisper, Parakeet
    cloud/         - batch + streaming providers
  enhancement/     - LLM orchestrator + providers + prompt detection
  power_mode/      - foreground window detection + URL extraction + profile matcher
  screen_context/  - window capture + OCR
  history/         - SQLite + retention + CSV export
  paste/           - Ctrl+V via SendInput + multi-format clipboard backup
  hotkeys/         - low-level keyboard hook + state machine
src/
  components/      - UI panels + mini recorder overlay
  lib/tauri.ts     - IPC bindings
  i18n/            - locale files (fr, en, es)
```

## Known limitations

### Recording does not work inside apps launched as administrator

Parla uses a global low-level keyboard hook to detect its recording shortcut (Right Alt by default). Windows UIPI (User Interface Privilege Isolation) prevents a non-elevated process from receiving keyboard events destined for a window of higher integrity level. In practice, this means that when a terminal, an editor, or any other application has been started with **Run as administrator**, the Parla hotkey is silently swallowed while that elevated window has keyboard focus, and no recording starts.

The proper fix is to ship Parla with a manifest flag `uiAccess=true`, which Windows honors on **code-signed** binaries installed under `C:\Program Files`. We do not currently have an Authenticode code-signing certificate, so this is not available yet.

Workarounds until we sign the releases:

- Use a non-elevated terminal whenever possible. Windows Terminal, PowerShell and Command Prompt only need administrator rights for a very small set of commands.
- If you genuinely need an elevated terminal, run Parla as administrator as well (right-click `parla.exe` > Run as administrator). Both processes then share the same integrity level and the hotkey is delivered. Note that an administrator Parla cannot paste into non-elevated applications - UIPI enforces the rule symmetrically.
- Give focus to a non-elevated window before triggering the hotkey, then switch back.

This limitation is a property of Windows, not a bug in Parla. It will be removed in a future release when signed installers are available.

## Documentation

- [BUILDING.md](BUILDING.md) - Detailed build instructions
- [CONTRIBUTING.md](CONTRIBUTING.md) - How to contribute
- [CHANGELOG.md](CHANGELOG.md) - Release notes

## Requirements

- Windows 10 22H2 or later (Windows 11 recommended for best `Media.Ocr` language coverage)
- At least 4 GB of RAM for the base Whisper model, more for the larger ones
- (Optional) NVIDIA GPU with CUDA 12.x drivers for GPU acceleration

## Contributing

Bug reports, feature requests, and pull requests are all welcome. See [CONTRIBUTING.md](CONTRIBUTING.md) for details.

You can also help by:
- Reporting bugs or regressions via [issues](https://github.com/LitteRabbit-37/Parla/issues)
- Suggesting features or enhancements
- Improving the documentation or translations (FR / EN / ES)

## License

This project is licensed under the **GNU General Public License v3.0**. See the [LICENSE](LICENSE) file for details.

Parla is an independent project inspired by [VoiceInk](https://github.com/Beingpax/VoiceInk) (GPL-3.0) but does not share any Swift / macOS source code. The Windows implementation is a full re-write in Rust + TypeScript.

## Acknowledgments

### Core technology
- [whisper.cpp](https://github.com/ggerganov/whisper.cpp) - High-performance inference of OpenAI's Whisper model
- [parakeet-rs](https://github.com/altunenes/parakeet-rs) - Rust wrapper over ONNX Runtime for NVIDIA Parakeet TDT
- [llama.cpp](https://github.com/ggerganov/llama.cpp) - Inference engine for the embedded LLM
- [Tauri](https://tauri.app/) - The desktop framework that ties everything together

### Inspiration
- [VoiceInk](https://github.com/Beingpax/VoiceInk) by [Pax](https://github.com/Beingpax) - the macOS reference this project aims to match feature-for-feature
- [FluidAudio](https://github.com/FluidInference/FluidAudio) - the macOS Parakeet implementation VoiceInk relies on

### Essential dependencies
- [whisper-rs](https://github.com/tazz4843/whisper-rs) - Rust bindings over whisper.cpp
- [llama-cpp-rs](https://github.com/utilityai/llama-cpp-rs) - Rust bindings over llama.cpp
- [cpal](https://github.com/RustAudio/cpal) - Cross-platform audio I/O
- [keyring-rs](https://github.com/hwchen/keyring-rs) - OS-level credential storage
- [rusqlite](https://github.com/rusqlite/rusqlite) - SQLite bindings for Rust
- [tokio-tungstenite](https://github.com/snapview/tokio-tungstenite) - async WebSocket for streaming providers

## Support

If you find Parla useful and want to support the development, you can buy me a coffee:

<a href="https://www.buymeacoffee.com/litterabbit" target="_blank"><img src="https://cdn.buymeacoffee.com/buttons/v2/default-yellow.png" alt="Buy Me A Coffee" height="40" /></a>

## Thanks

<!--START_SECTION:buy-me-a-coffee-->
<!--END_SECTION:buy-me-a-coffe-->

Many thanks to [Pax](https://github.com/Beingpax) for the original VoiceInk and to every contributor of the upstream projects (whisper.cpp, llama.cpp, parakeet-rs, Tauri, shadcn/ui).

Feel free to submit issues, pull requests, or feedback.
