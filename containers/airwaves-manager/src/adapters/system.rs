use std::sync::Mutex;
use sysinfo::System;

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

impl SystemPort for SystemAdapter {
    fn get_info(&self) -> Result<SystemInfo, AppError> {
        let hostname = System::host_name().unwrap_or_else(|| "airwaves".to_string());
        let os = format!(
            "{} {}",
            System::name().unwrap_or_else(|| "Linux".to_string()),
            System::os_version().unwrap_or_default()
        );
        let arch = System::cpu_arch().unwrap_or_else(|| "unknown".to_string());
        let kernel = System::kernel_version().unwrap_or_else(|| "unknown".to_string());
        let uptime = System::uptime();

        // Read Airwaves version
        let version = std::fs::read_to_string("/etc/airwaves-release")
            .ok()
            .and_then(|content| {
                content
                    .lines()
                    .find(|l| l.starts_with("AIRWAVES_VERSION="))
                    .map(|l| l.trim_start_matches("AIRWAVES_VERSION=").to_string())
            })
            .unwrap_or_else(|| "unknown".to_string());

        Ok(SystemInfo {
            hostname,
            os,
            architecture: arch,
            kernel,
            uptime,
            airwaves_version: version,
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

        let (disk_total, disk_used) = sys.disks().iter().fold((0u64, 0u64), |(total, used), d| {
            (total + d.total_space(), used + (d.total_space() - d.available_space()))
        });
        let disk_percent = if disk_total > 0 {
            (disk_used as f32 / disk_total as f32) * 100.0
        } else {
            0.0
        };

        // Temperature from thermal zones
        let temperature = sys.components().iter()
            .find(|c| c.label().contains("CPU") || c.label().contains("cpu") || c.label().contains("SoC"))
            .map(|c| c.temperature());

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
