//! `mukei_core::engine::gpu_strategy` — TRD §3.2.
//!
//! Mali vs Adreno layer splitting. The heuristic:
//!  - Read `Build.HARDWARE` and `/proc/cpuinfo` to fingerprint the GPU.
//!  - Mali (ARM): full offload; Adreno (Snapdragon): partial offload
//!    (the first 12 layers to GPU, the rest to CPU) because Adreno hits
//!    a thermal wall on 3+ GB models.
//!  - Sugarloaf (Apple): NPU offload via ANE.

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum GpuKind {
    Mali,
    Adreno,
    Sugarloaf,
    CpuOnly,
    Unknown,
}

pub struct GpuStrategy {
    pub kind: GpuKind,
    pub gpu_layers: i32,
}

impl GpuStrategy {
    pub fn detect() -> Self {
        // Real implementation probes `/proc/cpuinfo` and the GLES
        // vendor string. The bridge crate overrides it.
        Self { kind: GpuKind::Unknown, gpu_layers: 0 }
    }

    /// Choose a layer count for a given model size and the active
    /// GPU. The fallback is `0` (CPU-only).
    pub fn pick_layers(&self, model_bytes: u64) -> i32 {
        match self.kind {
            GpuKind::Mali      if model_bytes < 1_500_000_000 => 99,
            GpuKind::Mali                                   => 32,
            GpuKind::Adreno    if model_bytes < 1_500_000_000 => 12,
            GpuKind::Adreno                                 => 0,
            GpuKind::Sugarloaf                             => 99,
            _                                              => 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adreno_small_model_uses_partial_offload() {
        let s = GpuStrategy { kind: GpuKind::Adreno, gpu_layers: 12 };
        assert_eq!(s.pick_layers(800_000_000), 12);
    }

    #[test]
    fn adreno_large_model_drops_to_cpu() {
        let s = GpuStrategy { kind: GpuKind::Adreno, gpu_layers: 12 };
        assert_eq!(s.pick_layers(4_000_000_000), 0);
    }

    #[test]
    fn cpu_only_returns_zero() {
        let s = GpuStrategy::detect();
        assert_eq!(s.kind, GpuKind::Unknown);
        assert_eq!(s.pick_layers(1_000_000_000), 0);
    }
}
