// Detection GPU NVIDIA via NVML (driver) + detection dynamique du runtime CUDA.
// But : savoir au demarrage si on peut activer les backends GPU (whisper.cpp cuBLAS,
// llama.cpp CUDA, ONNX Runtime CUDA EP).

use crate::GpuInfo;
use tracing::debug;

#[cfg(feature = "gpu-detect")]
pub fn detect() -> GpuInfo {
    match nvml_wrapper::Nvml::init() {
        Ok(nvml) => {
            let device_name = nvml.device_by_index(0).and_then(|d| d.name()).ok();
            let driver_version = nvml.sys_driver_version().ok();
            let cuda_version = nvml
                .sys_cuda_driver_version()
                .ok()
                .map(|v| format!("{}.{}", v / 1000, (v % 1000) / 10));

            GpuInfo {
                has_nvidia: true,
                device_name,
                driver_version,
                cuda_version,
            }
        }
        Err(e) => {
            debug!("NVML init failed: {e}");
            GpuInfo {
                has_nvidia: false,
                device_name: None,
                driver_version: None,
                cuda_version: None,
            }
        }
    }
}

#[cfg(not(feature = "gpu-detect"))]
pub fn detect() -> GpuInfo {
    GpuInfo {
        has_nvidia: false,
        device_name: None,
        driver_version: None,
        cuda_version: None,
    }
}
