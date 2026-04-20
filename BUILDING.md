# Building Parla

This document describes how to build Parla from source on Windows.

## Prerequisites

### Required

- **Windows 10 22H2 or Windows 11**
- **[Rust](https://rustup.rs/)** stable toolchain (tested with 1.95+)
- **[Node.js 20+](https://nodejs.org/)** and `npm`
- **Microsoft Visual Studio 2022 Build Tools** with the "Desktop development with C++" workload (or Visual Studio Community/Professional/Enterprise)
- **WebView2 Runtime** (preinstalled on Windows 11, installed automatically by the Parla installer)

### Optional (for GPU acceleration)

- **NVIDIA CUDA Toolkit 12.x** with the "Visual Studio Integration" sub-component enabled.
- An NVIDIA GPU with recent drivers.

CUDA is OFF by default so a plain `npm run tauri build` produces a CPU-only binary with no NVIDIA dependency. Enable GPU acceleration with `--features cuda` - full recipe in the [CUDA build](#cuda-build) section below.

## Clone the repo

```bash
git clone https://github.com/LitteRabbit-37/Parla.git
cd Parla
```

## Install dependencies

```bash
npm install
```

## Development mode

Run the app in hot-reload dev mode (Vite dev server + Tauri dev):

```bash
npm run tauri dev
```

The first launch builds the Rust backend which can take a few minutes (whisper.cpp and llama.cpp are compiled from source via their `-sys` crates).

## Release build

```bash
npm run tauri build
```

This produces:

- `src-tauri/target/release/parla.exe` - the standalone binary
- `src-tauri/target/release/bundle/nsis/Parla_x.y.z_x64-setup.exe` - NSIS installer (the only bundle target we ship; it prompts for data removal on uninstall and lets the user opt out of the desktop shortcut)

## ARM64 build

To build Parla natively for Windows on ARM (Snapdragon X, etc.):

### Prerequisites for ARM64

- **Clang/LLVM tools**: In the Visual Studio Installer, under \"Desktop development with C++\", ensure \"C++ Clang Compiler for Windows\" and \"MSBuild support for LLVM toolset\" are checked. The ggml library (used by whisper.cpp and llama.cpp) does not support MSVC on ARM64 — it requires Clang.
- **Ninja**: Install via `choco install ninja`, or ensure it is available from the Visual Studio installation.

### Build

```powershell
rustup target add aarch64-pc-windows-msvc
$env:CC = "clang-cl"
$env:CXX = "clang-cl"
$env:CMAKE_GENERATOR = "Ninja"
npm run tauri build -- --target aarch64-pc-windows-msvc -- --no-default-features
```

This produces:

- `src-tauri/target/aarch64-pc-windows-msvc/release/parla.exe` - the standalone binary
- `src-tauri/target/aarch64-pc-windows-msvc/release/bundle/nsis/Parla_x.y.z_arm64-setup.exe` - NSIS installer

`--no-default-features` disables `gpu-detect` (the `nvml-wrapper` crate) because NVIDIA does not ship `nvml.dll` for Windows ARM64. All other functionality (Whisper, Parakeet, llama.cpp, audio capture, etc.) works natively on ARM64.

The CI workflow `.github/workflows/release.yml` builds the ARM64 variant automatically on GitHub's `windows-11-arm` runner.

## Cargo features

Parla exposes several Cargo features in `src-tauri/Cargo.toml` that control backend capabilities:

| Feature | Default | Description |
|---|---|---|
| `gpu-detect` | yes | Detect NVIDIA GPU via NVML at startup (log only) |
| `cuda` | no | Meta-feature enabling `cuda-whisper` + `cuda-llama` + `cuda-onnx` all at once |
| `cuda-whisper` | no | Compile `whisper.cpp` with CUDA support |
| `cuda-llama` | no | Compile `llama.cpp` with CUDA support |
| `cuda-onnx` | no | Enable the ONNX Runtime CUDA Execution Provider for Parakeet |
| `directml-onnx` | no | Enable the ONNX Runtime DirectML EP (AMD / Intel GPU) |

The CI workflow `.github/workflows/release.yml` builds CPU variants for both x64 and ARM64. The GitHub public `windows-latest` runner (4 cores, 16 GB RAM, 14 GB disk) cannot fit the CUDA build of whisper.cpp + llama.cpp + ggml-cuda within the 6 hour job limit - nvcc and MSBuild oversubscribe the machine and swap-thrash until the job dies. If you need CUDA, build it yourself from source with the recipe below.

## CUDA build

Produces `Parla_x.y.z_x64-setup.exe` (same filename as CPU but linked against CUDA 12 runtime DLLs).

### Prerequisites for CUDA

- An NVIDIA GPU with up-to-date drivers.
- **NVIDIA CUDA Toolkit 12.x** (12.6 or 12.9 both work ; 13.x also compiles but requires very recent NVIDIA drivers at install time). Installer : https://developer.nvidia.com/cuda-downloads. During install, tick the "Visual Studio Integration" sub-component - without it CMake will not find the CUDA toolset.
- **Visual Studio 2022** with the "Desktop development with C++" workload (Build Tools, Community, Professional or Enterprise all work).
- Confirm `nvcc --version` runs from a fresh terminal.

### Build

```bash
git clone https://github.com/LitteRabbit-37/Parla.git
cd Parla
npm install
npm run tauri build -- --features cuda
```

Expected wall time : **10 to 30 minutes** depending on your CPU (`llama-cpp-sys-2` and `whisper-rs-sys` spawn `nvcc` on every `.cu` file in ggml-cuda). Your GPU is not used during compilation - nvcc runs on CPU and generates PTX / SASS. The GPU only gets exercised at runtime when Parla transcribes.

### Result

```
src-tauri\target\release\bundle\nsis\Parla_0.1.0_x64-setup.exe
```

Install it and the Parla binary you run links against the CUDA runtime at startup. If you launch this installer on a machine without an NVIDIA GPU (or without the CUDA runtime DLLs in PATH) the process will fail to start because the loader cannot resolve `cudart64_12.dll`. The CPU variant has no such dependency.

### Notes

- The CUDA variant currently does not receive the auto-updater. Tauri's updater points to a single `latest.json` which maps to the CPU installer. A future release could add a second endpoint for CUDA builds ; open an issue if that matters to you.
- If your CUDA build fails at CMake with "No CUDA toolset found", the Visual Studio Integration sub-package is missing. Re-run the CUDA installer and tick "Visual Studio Integration" under CUDA / Development.
- To skip whisper-rs specifically (e.g. to build a Parakeet-only CUDA variant), use `--features cuda-llama,cuda-onnx` instead of the meta-feature `cuda`.

## Tests

```bash
cd src-tauri
cargo test --lib
```

The Rust test suite covers hotkeys state machine, power mode matching, text filters, URL validator, prompt detection, cloud catalog and enhancement helpers.

## Auto-updater signing keys

If you plan to distribute builds via GitHub Releases with the Tauri auto-updater, you need a signing key pair:

```bash
npm run tauri signer generate
```

This prints a public key and a private key. Set the public key in `src-tauri/tauri.conf.json` under `plugins.updater.pubkey`, and store the private key in your repo secrets as `TAURI_SIGNING_PRIVATE_KEY` (with `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` if you set a passphrase). The `.github/workflows/release.yml` workflow picks these up automatically.

## Troubleshooting

- **rustc STATUS_ACCESS_VIOLATION on tokenizers**: already mitigated via `[profile.release.package.tokenizers] opt-level = 1` in `Cargo.toml`. If you still hit it, update your rustc version.
- **ggml symbol collision** between `whisper-rs-sys` and `llama-cpp-sys-2`: handled by `.cargo/config.toml` with `/FORCE:MULTIPLE` on MSVC link. Nothing to do.
- **`VAD download 404`**: the default VAD URL is `https://huggingface.co/ggml-org/whisper-vad` - if HuggingFace is unreachable, skip VAD from the `VAD` panel and retry later.
- **`CUDA` not detected** even though you have a GPU: ensure `nvml.dll` is reachable (usually in `C:\Windows\System32\nvml.dll`) and the NVIDIA driver is recent enough.

## Project layout

See the [README.md](README.md) "Architecture" section for a full module tour.
