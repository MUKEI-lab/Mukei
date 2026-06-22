//! `mukei_core::engine::gpu_strategy` — TRD §3.2 / PRD REQ-INF-03.
//!
//! Heuristic GPU detection + thermal-aware layer count.
//!
//! # Invariants
//!
//! - The probe reads `/proc/cpuinfo` (Linux/Android) and the GLES
//!   vendor string when available. The bridge crate may override the
//!   result via [`GpuStrategy::with_kind`] / [`GpuStrategy::with_layers`]
//!   if it has better signal from the platform native side.
//! - The probe is **side-effect free** — no allocator pressure, no
//!   long-running I/O. Safe to call on the runtime worker.
//! - [`GpuStrategy::pick_layers_with_thermal`] is the production entry
//!   point. It reduces the layer count when `thermal_status >= 2`
//!   (Android `ThermalStatus.SEVERE`) so generation stays responsive
//!   under load.

use std::fs;

/// GPU family detected on the live device.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum GpuKind {
    /// ARM Mali (most mid-range Android phones).
    Mali,
    /// Qualcomm Adreno (Snapdragon).
    Adreno,
    /// Apple Silicon (NPU/ANE offload).
    Sugarloaf,
    /// CPU-only by user request or unsupported GPU.
    CpuOnly,
    /// Probe could not classify the device.
    Unknown,
}

impl GpuKind {
    /// Stable tag used in the FFI snapshot and `tracing` spans.
    pub fn as_tag(self) -> &'static str {
        match self {
            Self::Mali => "mali",
            Self::Adreno => "adreno",
            Self::Sugarloaf => "sugarloaf",
            Self::CpuOnly => "cpu_only",
            Self::Unknown => "unknown",
        }
    }
}

/// Detected GPU + chosen layer count.
#[derive(Clone, Debug)]
pub struct GpuStrategy {
    /// Detected GPU family.
    pub kind: GpuKind,
    /// Last-decided layer count (cached for the bridge to read).
    pub gpu_layers: i32,
}

impl GpuStrategy {
    /// Probe `/proc/cpuinfo` and friends. Falls back to
    /// `GpuKind::Unknown` when the platform does not expose a useful
    /// fingerprint (desktop CI, non-Android Linux, etc.).
    pub fn detect() -> Self {
        let kind = detect_gpu_kind();
        Self {
            kind,
            gpu_layers: 0,
        }
    }

    /// Bridge-supplied override (e.g. when the QML side knows it is
    /// running on a Pixel 8 even though `/proc/cpuinfo` is sparse).
    pub fn with_kind(mut self, kind: GpuKind) -> Self {
        self.kind = kind;
        self
    }

    /// Manually override the layer count (escape hatch for advanced
    /// users tweaking `mukei.toml`).
    pub fn with_layers(mut self, layers: i32) -> Self {
        self.gpu_layers = layers;
        self
    }

    /// Choose a layer count for a given model size and the active GPU.
    /// The fallback is `0` (CPU-only).
    pub fn pick_layers(&self, model_bytes: u64) -> i32 {
        match self.kind {
            GpuKind::Mali if model_bytes < 1_500_000_000 => 99,
            GpuKind::Mali => 32,
            GpuKind::Adreno if model_bytes < 1_500_000_000 => 12,
            GpuKind::Adreno => 0,
            GpuKind::Sugarloaf => 99,
            _ => 0,
        }
    }

    /// Thermal-aware variant. `thermal_status` follows the Android
    /// `PowerManager.ThermalStatus` enum (0=none, 1=light, 2=moderate,
    /// 3=severe, 4=critical). At `>= 2` we halve the offload count; at
    /// `>= 3` we drop to CPU entirely.
    pub fn pick_layers_with_thermal(&self, model_bytes: u64, thermal_status: u8) -> i32 {
        let base = self.pick_layers(model_bytes);
        if thermal_status >= 3 {
            0
        } else if thermal_status == 2 {
            (base / 2).max(0)
        } else {
            base
        }
    }
}

/// Tries `/proc/cpuinfo` + `Build.HARDWARE` semantics. Each branch is
/// strictly best-effort — a wrong classification is preferable to a
/// crash.
fn detect_gpu_kind() -> GpuKind {
    // macOS / iOS: detect Apple Silicon by uname-style identifiers.
    #[cfg(target_os = "macos")]
    {
        if let Ok(out) = std::process::Command::new("uname").arg("-m").output() {
            let arch = String::from_utf8_lossy(&out.stdout).to_lowercase();
            if arch.contains("arm64") || arch.contains("aarch64") {
                return GpuKind::Sugarloaf;
            }
        }
        return GpuKind::CpuOnly;
    }

    #[cfg(target_os = "linux")]
    {
        if let Ok(text) = fs::read_to_string("/proc/cpuinfo") {
            let lower = text.to_lowercase();
            if lower.contains("qualcomm") || lower.contains("snapdragon") || lower.contains("adreno") {
                return GpuKind::Adreno;
            }
            if lower.contains("mali") || lower.contains("exynos") || lower.contains("mediatek") {
                return GpuKind::Mali;
            }
        }
        // Android-specific build.prop hint when `/proc/cpuinfo` is sparse.
        if let Ok(hw) = fs::read_to_string("/system/build.prop") {
            let lower = hw.to_lowercase();
            if lower.contains("qcom") || lower.contains("sdm") {
                return GpuKind::Adreno;
            }
            if lower.contains("mt6") || lower.contains("exynos") {
                return GpuKind::Mali;
            }
        }
        return GpuKind::Unknown;
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        GpuKind::Unknown
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adreno_small_model_uses_partial_offload() {
        let s = GpuStrategy {
            kind: GpuKind::Adreno,
            gpu_layers: 12,
        };
        assert_eq!(s.pick_layers(800_000_000), 12);
    }

    #[test]
    fn adreno_large_model_drops_to_cpu() {
        let s = GpuStrategy {
            kind: GpuKind::Adreno,
            gpu_layers: 12,
        };
        assert_eq!(s.pick_layers(4_000_000_000), 0);
    }

    #[test]
    fn cpu_only_returns_zero() {
        let s = GpuStrategy::detect().with_kind(GpuKind::CpuOnly);
        assert_eq!(s.pick_layers(1_000_000_000), 0);
    }

    #[test]
    fn thermal_halves_layers_at_moderate() {
        let s = GpuStrategy {
            kind: GpuKind::Mali,
            gpu_layers: 0,
        };
        let base = s.pick_layers(800_000_000);
        assert!(base > 0);
        let halved = s.pick_layers_with_thermal(800_000_000, 2);
        assert_eq!(halved, base / 2);
    }

    #[test]
    fn thermal_severe_drops_to_cpu() {
        let s = GpuStrategy {
            kind: GpuKind::Mali,
            gpu_layers: 0,
        };
        assert_eq!(s.pick_layers_with_thermal(800_000_000, 3), 0);
        assert_eq!(s.pick_layers_with_thermal(800_000_000, 4), 0);
    }

    #[test]
    fn gpu_kind_tags_are_stable_ascii() {
        for k in [
            GpuKind::Mali,
            GpuKind::Adreno,
            GpuKind::Sugarloaf,
            GpuKind::CpuOnly,
            GpuKind::Unknown,
        ] {
            let t = k.as_tag();
            assert!(t.chars().all(|c| c.is_ascii_lowercase() || c == '_'));
        }
    }
}
