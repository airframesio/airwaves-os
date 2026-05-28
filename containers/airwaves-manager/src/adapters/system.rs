use std::sync::Mutex;
use sysinfo::{Components, Disks, System};

use crate::domain::*;
use crate::error::AppError;
use crate::ports::SystemPort;

pub struct SystemAdapter {
    sys: Mutex<System>,
}

impl SystemAdapter {
    pub fn new() -> Self {
        Self {
            sys: Mutex::new(System::new_all()),
        }
    }
}

/// Prefix for reading host files. When the manager runs inside a container,
/// the host root is bind-mounted (e.g. at /host) and `HOST_ROOT` points to it.
/// Empty string means "read paths directly" (native / dev mode).
fn host_root() -> String {
    std::env::var("HOST_ROOT")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_default()
}

/// Read a host file, applying the HOST_ROOT prefix.
fn read_host_file(rel_path: &str) -> Option<String> {
    std::fs::read_to_string(format!("{}{}", host_root(), rel_path)).ok()
}

/// Parse a `KEY=value` line out of an os-release / shell-style file,
/// stripping surrounding quotes.
fn parse_kv(content: &str, key: &str) -> Option<String> {
    let prefix = format!("{key}=");
    content.lines().find_map(|line| {
        line.trim()
            .strip_prefix(&prefix)
            .map(|v| v.trim().trim_matches('"').to_string())
    })
}

/// Detect the hardware model from the host filesystem (ARM device-tree or x86 DMI).
fn detect_model(root: &str) -> String {
    // ARM / SBC boards expose a device-tree model (NUL-terminated).
    for p in ["/proc/device-tree/model", "/sys/firmware/devicetree/base/model"] {
        if let Ok(s) = std::fs::read_to_string(format!("{root}{p}")) {
            let s = s.trim_matches(|c: char| c == '\0' || c.is_whitespace());
            if !s.is_empty() {
                return s.to_string();
            }
        }
    }

    // x86 / generic: combine DMI vendor + product name.
    let read_dmi = |field: &str| {
        std::fs::read_to_string(format!("{root}/sys/class/dmi/id/{field}"))
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty() && s != "To Be Filled By O.E.M.")
    };
    match (read_dmi("sys_vendor"), read_dmi("product_name")) {
        (Some(v), Some(p)) => format!("{v} {p}"),
        (None, Some(p)) => p,
        (Some(v), None) => v,
        _ => "Unknown".to_string(),
    }
}

/// statvfs a path and return (total_bytes, used_bytes) for its filesystem.
fn statvfs_disk(path: &str) -> Option<(u64, u64)> {
    let c_path = std::ffi::CString::new(path).ok()?;
    // SAFETY: c_path is a valid NUL-terminated string; stat is zeroed before use.
    unsafe {
        let mut stat: libc::statvfs = std::mem::zeroed();
        if libc::statvfs(c_path.as_ptr(), &mut stat) != 0 {
            return None;
        }
        let block = stat.f_frsize as u64;
        let total = stat.f_blocks as u64 * block;
        let avail = stat.f_bavail as u64 * block;
        Some((total, total.saturating_sub(avail)))
    }
}

impl SystemPort for SystemAdapter {
    fn get_info(&self) -> Result<SystemInfo, AppError> {
        let root = host_root();

        // Hostname: prefer the host's /etc/hostname over the container's.
        let hostname = read_host_file("/etc/hostname")
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .or_else(System::host_name)
            .unwrap_or_else(|| "airwaves".to_string());

        // OS: parse the host's /etc/os-release for the real distro, falling
        // back to sysinfo (which would report the container's base image).
        let os_release = read_host_file("/etc/os-release");
        let os = os_release
            .as_deref()
            .and_then(|c| parse_kv(c, "PRETTY_NAME"))
            .unwrap_or_else(|| {
                format!(
                    "{} {}",
                    System::name().unwrap_or_else(|| "Linux".to_string()),
                    System::os_version().unwrap_or_default()
                )
                .trim()
                .to_string()
            });
        let os_codename = os_release
            .as_deref()
            .and_then(|c| parse_kv(c, "VERSION_CODENAME"))
            .unwrap_or_default();

        // cpu_arch returns a String directly in sysinfo 0.33
        let architecture = {
            let a = System::cpu_arch();
            if a.is_empty() {
                "unknown".to_string()
            } else {
                a
            }
        };
        let kernel = System::kernel_version().unwrap_or_else(|| "unknown".to_string());
        let uptime = System::uptime();
        let model = detect_model(&root);

        // CPU details from the (host-visible) /proc.
        let (cpu_model, cpu_cores) = {
            let sys = self.sys.lock().map_err(|e| AppError::Internal(e.to_string()))?;
            let cpus = sys.cpus();
            let model = cpus
                .first()
                .map(|c| c.brand().trim().to_string())
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| "Unknown".to_string());
            (model, cpus.len())
        };

        // Airwaves release metadata, written into the host image at build time.
        let release = read_host_file("/etc/airwaves-release");
        let field = |key: &str| {
            release
                .as_deref()
                .and_then(|c| parse_kv(c, key))
                .filter(|s| !s.is_empty())
        };

        Ok(SystemInfo {
            hostname,
            os,
            os_codename,
            architecture,
            kernel,
            uptime,
            model,
            cpu_model,
            cpu_cores,
            airwaves_version: field("AIRWAVES_VERSION").unwrap_or_else(|| "unknown".to_string()),
            airwaves_codename: field("AIRWAVES_CODENAME").unwrap_or_default(),
            airwaves_build_date: field("AIRWAVES_BUILD_DATE").unwrap_or_default(),
            airwaves_board: field("AIRWAVES_BUILD_BOARD").unwrap_or_default(),
        })
    }

    fn get_stats(&self) -> Result<SystemStats, AppError> {
        let mut sys = self.sys.lock().map_err(|e| AppError::Internal(e.to_string()))?;
        sys.refresh_all();

        let cpu_usage = sys.global_cpu_usage();
        let memory_total = sys.total_memory();
        let memory_used = sys.used_memory();
        let memory_percent = if memory_total > 0 {
            (memory_used as f32 / memory_total as f32) * 100.0
        } else {
            0.0
        };

        // Report the host root filesystem, not the container's overlay. The
        // host root is statvfs'd via HOST_ROOT (or "/" when running natively).
        let disk_root = std::env::var("HOST_ROOT")
            .ok()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "/".to_string());
        let (disk_total, disk_used) = statvfs_disk(&disk_root).unwrap_or_else(|| {
            // Fallback: aggregate across all visible disks.
            let disks = Disks::new_with_refreshed_list();
            disks.iter().fold((0u64, 0u64), |(total, used), d| {
                (
                    total + d.total_space(),
                    used + (d.total_space() - d.available_space()),
                )
            })
        });
        let disk_percent = if disk_total > 0 {
            (disk_used as f32 / disk_total as f32) * 100.0
        } else {
            0.0
        };

        // Temperature from thermal zones
        let components = Components::new_with_refreshed_list();
        let temperature = components
            .iter()
            .find(|c| {
                let label = c.label();
                label.contains("CPU") || label.contains("cpu") || label.contains("SoC")
            })
            .and_then(|c| c.temperature());

        let load_avg = System::load_average();

        Ok(SystemStats {
            cpu_usage,
            memory_total,
            memory_used,
            memory_percent,
            disk_total,
            disk_used,
            disk_percent,
            temperature,
            load_average: [load_avg.one, load_avg.five, load_avg.fifteen],
        })
    }
}
