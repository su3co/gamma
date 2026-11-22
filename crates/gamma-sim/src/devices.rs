//! Devices a simulation can run on, and throughput used for time estimates.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Device {
    Cpu,
    /// One or several NVIDIA GPUs of this machine.
    Cuda {
        gpus: Vec<usize>,
    },
}

impl Device {
    pub fn parse(s: &str) -> anyhow::Result<Device> {
        let s = s.trim();
        if s == "cpu" {
            return Ok(Device::Cpu);
        }
        if let Some(rest) = s.strip_prefix("cuda") {
            let rest = rest.trim_start_matches(':');
            if rest.is_empty() {
                return Ok(Device::Cuda { gpus: vec![0] });
            }
            if rest == "all" {
                let n = cuda_count();
                if n == 0 {
                    anyhow::bail!("no CUDA GPUs on this machine (or gamma was built without CUDA)");
                }
                return Ok(Device::Cuda {
                    gpus: (0..n).collect(),
                });
            }
            let v: Result<Vec<usize>, _> =
                rest.split(',').map(|x| x.trim().parse::<usize>()).collect();
            return Ok(Device::Cuda {
                gpus: v.map_err(|_| anyhow::anyhow!("bad GPU list in '{s}'"))?,
            });
        }
        anyhow::bail!("unknown device '{s}': use cpu, cuda, cuda:0, cuda:0,1 or cuda:all")
    }

    pub fn label(&self) -> String {
        match self {
            Device::Cpu => "cpu".into(),
            Device::Cuda { gpus } => format!(
                "cuda:{}",
                gpus.iter()
                    .map(|g| g.to_string())
                    .collect::<Vec<_>>()
                    .join(",")
            ),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct DeviceInfo {
    /// `cpu`, `cuda:0`, ...
    pub id: String,
    pub name: String,
    /// Memory available to a run, bytes (system memory for the CPU).
    pub memory: Option<u64>,
    pub threads: Option<usize>,
    /// Typical throughput, cell updates per second (default table or measured).
    pub throughput: f64,
}

pub fn cuda_count() -> usize {
    #[cfg(feature = "cuda")]
    {
        lightsim_cuda::device_count().unwrap_or(0)
    }
    #[cfg(not(feature = "cuda"))]
    {
        0
    }
}

#[cfg(feature = "cuda")]
fn cuda_info(i: usize) -> (String, Option<u64>) {
    match cuda_core::CudaContext::new(i) {
        Ok(ctx) => (ctx.name().unwrap_or_else(|_| format!("GPU {i}")), None),
        Err(_) => (format!("GPU {i}"), None),
    }
}

/// Default single-precision throughput (cell updates per second) for a
/// device name, from lightsim's benchmarks of a vacuum box with absorbing
/// boundaries; dielectric structures and monitors cost 10-40 % more.
pub fn default_throughput(name: &str, threads: usize) -> f64 {
    let n = name.to_ascii_lowercase();
    if n.contains("rtx pro 6000") {
        18e9
    } else if n.contains("gb10") {
        1.8e9
    } else if n.contains("h100") || n.contains("h200") || n.contains("b200") {
        20e9
    } else if n.contains("a100") {
        12e9
    } else if n.contains("rtx") || n.contains("geforce") || n.contains("nvidia") {
        6e9
    } else {
        // CPU: about 14 Mcells/s per core on recent cores
        14e6 * threads.max(1) as f64
    }
}

pub fn total_memory() -> Option<u64> {
    #[cfg(target_os = "linux")]
    {
        let s = std::fs::read_to_string("/proc/meminfo").ok()?;
        let line = s.lines().find(|l| l.starts_with("MemTotal:"))?;
        let kb: u64 = line.split_whitespace().nth(1)?.parse().ok()?;
        Some(kb * 1024)
    }
    #[cfg(target_os = "macos")]
    {
        let out = std::process::Command::new("sysctl")
            .args(["-n", "hw.memsize"])
            .output()
            .ok()?;
        String::from_utf8_lossy(&out.stdout).trim().parse().ok()
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        None
    }
}

pub fn cpu_name() -> String {
    #[cfg(target_os = "macos")]
    {
        if let Ok(out) = std::process::Command::new("sysctl")
            .args(["-n", "machdep.cpu.brand_string"])
            .output()
        {
            let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if !s.is_empty() {
                return s;
            }
        }
    }
    #[cfg(target_os = "linux")]
    {
        if let Ok(s) = std::fs::read_to_string("/proc/cpuinfo") {
            if let Some(l) = s.lines().find(|l| l.starts_with("model name")) {
                if let Some((_, v)) = l.split_once(':') {
                    return v.trim().to_string();
                }
            }
        }
    }
    format!("{} CPU", std::env::consts::ARCH)
}

/// Devices of this machine.
pub fn local_devices() -> Vec<DeviceInfo> {
    let threads = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1);
    #[allow(unused_mut)]
    let mut out = vec![DeviceInfo {
        id: "cpu".into(),
        name: cpu_name(),
        memory: total_memory(),
        threads: Some(threads),
        throughput: default_throughput("cpu", threads),
    }];
    #[cfg(feature = "cuda")]
    for i in 0..cuda_count() {
        let (name, mem) = cuda_info(i);
        let tp = default_throughput(&name, 0);
        out.push(DeviceInfo {
            id: format!("cuda:{i}"),
            name,
            memory: mem,
            threads: None,
            throughput: tp,
        });
    }
    out
}

pub fn precision() -> &'static str {
    if lightsim_core::REAL_IS_F64 {
        "f64"
    } else {
        "f32"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parse() {
        assert_eq!(Device::parse("cpu").unwrap(), Device::Cpu);
        assert_eq!(
            Device::parse("cuda:0,1").unwrap(),
            Device::Cuda { gpus: vec![0, 1] }
        );
        assert_eq!(Device::parse("cuda").unwrap().label(), "cuda:0");
        assert!(Device::parse("tpu").is_err());
        assert!(!local_devices().is_empty());
    }
}
