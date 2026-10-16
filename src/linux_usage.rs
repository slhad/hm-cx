use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Default)]
pub(crate) struct UsageCache {
    cpu: Option<CpuTimes>,
}

#[derive(Debug, Clone, Copy)]
struct CpuTimes {
    total: u64,
    idle: u64,
}

pub(crate) fn sensor_key(sensor_id: &str) -> Option<&'static str> {
    match sensor_id {
        "/amdcpu/0/load/0" => Some("cpu_total"),
        "/ram/load/0" => Some("ram_percent"),
        "/ram/data/0" => Some("ram_used_gb"),
        "/ram/data/1" => Some("ram_available_gb"),
        "/gpu-amd/0/load/0" => Some("gpu_percent"),
        "/gpu-amd/0/load/1" => Some("gpu_memory_percent"),
        "/gpu-amd/0/smalldata/0" => Some("gpu_memory_used_mb"),
        "/gpu-amd/0/smalldata/1" => Some("gpu_memory_free_mb"),
        "/gpu-amd/0/smalldata/2" => Some("gpu_memory_total_mb"),
        _ => None,
    }
}

pub(crate) fn source_available(key: &str) -> bool {
    match key {
        "cpu_total" => std::path::Path::new("/proc/stat").exists(),
        "ram_percent" | "ram_used_gb" | "ram_available_gb" => {
            std::path::Path::new("/proc/meminfo").exists()
        }
        "gpu_percent"
        | "gpu_memory_percent"
        | "gpu_memory_used_mb"
        | "gpu_memory_free_mb"
        | "gpu_memory_total_mb" => amd_gpu_device().is_some(),
        _ => false,
    }
}

fn amd_gpu_device() -> Option<PathBuf> {
    let mut devices = fs::read_dir("/sys/class/drm")
        .ok()?
        .filter_map(Result::ok)
        .filter(|entry| {
            entry.file_name().to_str().is_some_and(|name| {
                name.strip_prefix("card").is_some_and(|suffix| {
                    !suffix.is_empty() && suffix.chars().all(|c| c.is_ascii_digit())
                })
            })
        })
        .map(|entry| entry.path().join("device"))
        .filter(|device| {
            fs::read_to_string(device.join("vendor")).is_ok_and(|vendor| vendor.trim() == "0x1002")
                && device.join("gpu_busy_percent").exists()
        });
    let device = devices.next()?;
    // Do not silently assign another AMD card to the OHM GPU when multiple cards exist.
    devices.next().is_none().then_some(device)
}

fn parse_cpu_times(contents: &str) -> Option<CpuTimes> {
    let mut fields = contents.lines().next()?.split_whitespace();
    if fields.next()? != "cpu" {
        return None;
    }
    // Guest times are already included in user/nice, so sum only the first eight fields.
    let values = fields
        .take(8)
        .map(str::parse::<u64>)
        .collect::<Result<Vec<_>, _>>()
        .ok()?;
    if values.len() < 4 {
        return None;
    }
    Some(CpuTimes {
        total: values
            .iter()
            .try_fold(0u64, |sum, value| sum.checked_add(*value))?,
        idle: values[3].checked_add(values.get(4).copied().unwrap_or(0))?,
    })
}

fn cpu_percent(previous: CpuTimes, current: CpuTimes) -> Option<f64> {
    let total = current.total.checked_sub(previous.total)?;
    let idle = current.idle.checked_sub(previous.idle)?;
    if total == 0 || idle > total {
        return None;
    }
    Some(100.0 * (total - idle) as f64 / total as f64)
}

fn memory_values(contents: &str) -> Option<(f64, f64)> {
    let fields: HashMap<_, _> = contents
        .lines()
        .filter_map(|line| {
            let (key, rest) = line.split_once(':')?;
            let mut parts = rest.split_whitespace();
            let value = parts.next()?.parse::<u64>().ok()?;
            (parts.next()? == "kB").then_some((key, value))
        })
        .collect();
    let total = *fields.get("MemTotal")?;
    let available = *fields.get("MemAvailable")?;
    if total == 0 || available > total {
        return None;
    }
    Some((total as f64 * 1024.0, available as f64 * 1024.0))
}

pub(crate) fn sample(cache: &mut UsageCache) -> HashMap<String, f64> {
    let mut values = HashMap::new();
    let cpu = fs::read_to_string("/proc/stat")
        .ok()
        .and_then(|text| parse_cpu_times(&text));
    if let (Some(previous), Some(current)) = (cache.cpu, cpu) {
        if let Some(percent) = cpu_percent(previous, current) {
            values.insert("cpu_total".into(), percent);
        }
    }
    cache.cpu = cpu;
    if let Some((total, available)) = fs::read_to_string("/proc/meminfo")
        .ok()
        .and_then(|text| memory_values(&text))
    {
        let used = total - available;
        // OHM Data/SmallData formatting uses decimal GB/MB.
        values.insert("ram_used_gb".into(), used / 1e9);
        values.insert("ram_available_gb".into(), available / 1e9);
        values.insert("ram_percent".into(), 100.0 * used / total);
    }
    if let Some(device) = amd_gpu_device() {
        let read = |name: &str| {
            fs::read_to_string(device.join(name))
                .ok()?
                .trim()
                .parse::<u64>()
                .ok()
        };
        if let Some(busy) = read("gpu_busy_percent").filter(|v| *v <= 100) {
            values.insert("gpu_percent".into(), busy as f64);
        }
        if let (Some(used), Some(total)) = (read("mem_info_vram_used"), read("mem_info_vram_total"))
        {
            if total > 0 && used <= total {
                values.insert("gpu_memory_used_mb".into(), used as f64 / 1e6);
                values.insert("gpu_memory_free_mb".into(), (total - used) as f64 / 1e6);
                values.insert("gpu_memory_total_mb".into(), total as f64 / 1e6);
                values.insert(
                    "gpu_memory_percent".into(),
                    100.0 * used as f64 / total as f64,
                );
            }
        }
    }
    values
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cpu_load_uses_deltas_and_does_not_double_count_guests() {
        let first = parse_cpu_times("cpu 100 0 50 800 50 0 0 0 70 0").unwrap();
        let second = parse_cpu_times("cpu 130 0 70 840 60 0 0 0 90 0").unwrap();
        assert_eq!(cpu_percent(first, second), Some(50.0));
        assert_eq!(cpu_percent(second, second), None);
        assert_eq!(cpu_percent(second, first), None);
    }

    #[test]
    fn memory_load_counts_reclaimable_cache_as_available() {
        let (total, available) =
            memory_values("MemTotal: 1000000 kB\nMemFree: 100000 kB\nMemAvailable: 400000 kB\n")
                .unwrap();
        assert_eq!((total - available) / total, 0.6);
        assert_eq!(total, 1_024_000_000.0);
        assert_eq!(memory_values("MemTotal: 0 kB\nMemAvailable: 0 kB"), None);
        assert_eq!(memory_values("MemTotal: 10 kB\nMemAvailable: 20 kB"), None);
        assert_eq!(memory_values("MemTotal: 10 kB\n"), None);
    }

    #[test]
    fn virtual_memory_is_not_mapped_to_gpu_vram() {
        assert_eq!(sensor_key("/vram/load/1"), None);
        assert_eq!(
            sensor_key("/gpu-amd/0/smalldata/0"),
            Some("gpu_memory_used_mb")
        );
    }
}
