//! Read-only hardware summary for the device header: CPU model, installed
//! RAM, kernel release and the NVIDIA GPU model. Everything comes from
//! world-readable `/proc` files; a missing source leaves the field empty and
//! the UI hides it.

use std::path::Path;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SystemSummary {
    pub cpu_model: String,
    /// e.g. `16 потоков`.
    pub cpu_detail: String,
    /// e.g. `32 ГБ`.
    pub memory_total: String,
    pub kernel_release: String,
    pub gpu_model: String,
}

impl SystemSummary {
    pub fn read() -> Self {
        let cpuinfo = std::fs::read_to_string("/proc/cpuinfo").unwrap_or_default();
        let meminfo = std::fs::read_to_string("/proc/meminfo").unwrap_or_default();
        let (cpu_model, threads) = parse_cpuinfo(&cpuinfo);
        Self {
            cpu_model,
            cpu_detail: if threads > 0 {
                format!("{threads} потоков")
            } else {
                String::new()
            },
            memory_total: parse_mem_total_kib(&meminfo)
                .map(format_memory)
                .unwrap_or_default(),
            kernel_release: std::fs::read_to_string("/proc/sys/kernel/osrelease")
                .map(|s| s.trim().to_string())
                .unwrap_or_default(),
            gpu_model: nvidia_model(Path::new("/proc/driver/nvidia/gpus")).unwrap_or_default(),
        }
    }
}

/// CPU marketing name and logical processor count.
pub fn parse_cpuinfo(text: &str) -> (String, usize) {
    let mut model = String::new();
    let mut threads = 0;
    for line in text.lines() {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        match key.trim() {
            "processor" => threads += 1,
            "model name" if model.is_empty() => model = tidy_cpu_name(value.trim()),
            _ => {}
        }
    }
    (model, threads)
}

fn tidy_cpu_name(name: &str) -> String {
    let name = name
        .replace("(R)", "")
        .replace("(TM)", "")
        .replace("(tm)", "");
    // "AMD Ryzen 7 7735HS with Radeon Graphics" → keep the CPU part.
    let name = name.split(" with ").next().unwrap_or(&name);
    let name = name.split(" w/ ").next().unwrap_or(name);
    name.split_whitespace()
        .filter(|word| !word.eq_ignore_ascii_case("processor"))
        .filter(|word| !word.ends_with("-Core"))
        .collect::<Vec<_>>()
        .join(" ")
}

/// "NVIDIA GeForce RTX 4060 Laptop GPU" → "GeForce RTX 4060": the vendor and
/// form factor are implied on a laptop control panel.
pub fn tidy_gpu_name(name: &str) -> String {
    let name = name.strip_prefix("NVIDIA ").unwrap_or(name);
    let name = name.strip_suffix(" Laptop GPU").unwrap_or(name);
    name.trim().to_string()
}

pub fn parse_mem_total_kib(text: &str) -> Option<u64> {
    text.lines()
        .find_map(|line| line.strip_prefix("MemTotal:"))
        .and_then(|rest| rest.split_whitespace().next())
        .and_then(|value| value.parse().ok())
}

/// Installed RAM rounded to the module size users recognise (MemTotal is a
/// little below the physical amount because of firmware reservations).
pub fn format_memory(kib: u64) -> String {
    let gib = kib as f64 / (1024.0 * 1024.0);
    let rounded = if gib > 6.0 {
        (gib / 2.0).ceil() * 2.0
    } else {
        gib.ceil()
    };
    format!("{rounded:.0} ГБ")
}

fn nvidia_model(dir: &Path) -> Option<String> {
    let entries = std::fs::read_dir(dir).ok()?;
    for entry in entries.flatten() {
        let info = std::fs::read_to_string(entry.path().join("information")).ok()?;
        if let Some(model) = info
            .lines()
            .find_map(|line| line.strip_prefix("Model:"))
            .map(|model| tidy_gpu_name(model.trim()))
            .filter(|model| !model.is_empty())
        {
            return Some(model);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cpuinfo_yields_model_and_threads() {
        let text = "processor\t: 0\nmodel name\t: AMD Ryzen 7 7735HS with Radeon Graphics\n\nprocessor\t: 1\nmodel name\t: AMD Ryzen 7 7735HS with Radeon Graphics\n";
        assert_eq!(parse_cpuinfo(text), ("AMD Ryzen 7 7735HS".to_string(), 2));
        let (intel, _) = parse_cpuinfo("model name : 13th Gen Intel(R) Core(TM) i9-13980HX\n");
        assert_eq!(intel, "13th Gen Intel Core i9-13980HX");
    }

    #[test]
    fn memory_rounds_to_installed_size() {
        assert_eq!(
            parse_mem_total_kib("MemTotal:       32568904 kB\n"),
            Some(32_568_904)
        );
        assert_eq!(format_memory(32_568_904), "32 ГБ");
        assert_eq!(format_memory(15_700_000), "16 ГБ");
    }

    #[test]
    fn gpu_name_drops_vendor_and_form_factor() {
        assert_eq!(
            tidy_gpu_name("NVIDIA GeForce RTX 4060 Laptop GPU"),
            "GeForce RTX 4060"
        );
        assert_eq!(tidy_gpu_name("Quadro T1000"), "Quadro T1000");
    }

    #[test]
    fn missing_nvidia_driver_is_empty() {
        assert_eq!(nvidia_model(Path::new("/nonexistent/orbis")), None);
    }
}
