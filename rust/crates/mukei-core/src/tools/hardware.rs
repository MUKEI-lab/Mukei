//! TRD §5.4 — cached hardware snapshot tool.

use std::collections::BTreeMap;
use std::fs;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use async_trait::async_trait;
use once_cell::sync::Lazy;
use parking_lot::RwLock;
use serde_json::Value;

use crate::error::{MukeiError, Result};
use crate::tools::Tool;

#[derive(Default)]
pub struct HardwareTool;

#[derive(Clone, Debug)]
struct CachedHardwareInfo {
    turn_generation: u64,
    payload: String,
}

static TURN_GENERATION: AtomicU64 = AtomicU64::new(1);
static CACHE: Lazy<Arc<RwLock<Option<CachedHardwareInfo>>>> =
    Lazy::new(|| Arc::new(RwLock::new(None)));

impl HardwareTool {
    pub fn begin_turn() -> u64 {
        let next = TURN_GENERATION.fetch_add(1, Ordering::SeqCst) + 1;
        *CACHE.write() = None;
        next
    }
}

#[async_trait]
impl Tool for HardwareTool {
    fn name(&self) -> &'static str {
        "get_hardware_info"
    }

    async fn run(&self, _arguments: Value) -> Result<String> {
        let turn = TURN_GENERATION.load(Ordering::SeqCst);
        if let Some(cached) = CACHE.read().clone() {
            if cached.turn_generation == turn {
                return Ok(cached.payload);
            }
        }

        let join = crate::runtime::spawn_blocking_tool(build_payload);
        let payload = join
            .await
            .map_err(|e| MukeiError::BlockingJoinFailed(e.to_string()))??;

        *CACHE.write() = Some(CachedHardwareInfo {
            turn_generation: turn,
            payload: payload.clone(),
        });
        Ok(payload)
    }
}

fn build_payload() -> Result<String> {
    let mut map = BTreeMap::<&str, String>::new();
    map.insert("os", std::env::consts::OS.to_string());
    map.insert("arch", std::env::consts::ARCH.to_string());
    map.insert(
        "logical_cpus",
        std::thread::available_parallelism()
            .map(|n| n.get().to_string())
            .unwrap_or_else(|_| "unknown".to_string()),
    );

    if let Ok(cpuinfo) = fs::read_to_string("/proc/cpuinfo") {
        let model = cpuinfo
            .lines()
            .find_map(|line| line.split_once(':').map(|(k, v)| (k.trim(), v.trim())))
            .filter(|(k, _)| *k == "model name" || *k == "Hardware" || *k == "Processor")
            .map(|(_, v)| v.to_string())
            .unwrap_or_else(|| "unknown".to_string());
        map.insert("cpu_model", model);
    }

    if let Ok(meminfo) = fs::read_to_string("/proc/meminfo") {
        if let Some(line) = meminfo.lines().find(|line| line.starts_with("MemTotal:")) {
            map.insert("mem_total", line.replace("MemTotal:", "").trim().to_string());
        }
    }

    let json = serde_json::to_string_pretty(&map)
        .map_err(|e| MukeiError::Internal(e.to_string()))?;
    Ok(format!(
        "<external_data source=\"get_hardware_info\" trust=\"local_device\">\n{}\n</external_data>",
        json
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn returns_external_data_wrapper() {
        let tool = HardwareTool;
        let value = tool.run(serde_json::json!({})).await.unwrap();
        assert!(value.contains("<external_data"));
    }
}
