use crate::domain::*;
use crate::error::AppError;
use crate::ports::HardwarePort;

/// Known SDR device identifiers (vendor_id, product_id, name, type)
const KNOWN_SDR_DEVICES: &[(u16, u16, &str, &str)] = &[
    // RTL-SDR
    (0x0bda, 0x2838, "RTL-SDR", "rtl_sdr"),
    (0x0bda, 0x2832, "RTL2832U", "rtl_sdr"),
    // Airspy
    (0x1d50, 0x60a1, "Airspy R2/Mini", "airspy"),
    (0x03eb, 0x800c, "Airspy HF+", "airspy_hf"),
    // HackRF
    (0x1d50, 0x6089, "HackRF One", "hackrf"),
    // SDRplay
    (0x1df7, 0x2500, "SDRplay RSP1", "sdr_play"),
    (0x1df7, 0x3000, "SDRplay RSP1A", "sdr_play"),
    (0x1df7, 0x3010, "SDRplay RSP2", "sdr_play"),
    (0x1df7, 0x3020, "SDRplay RSPduo", "sdr_play"),
    // Funcube
    (0x04d8, 0xfb31, "Funcube Dongle Pro", "funcube_dongle"),
    (0x04d8, 0xfb56, "Funcube Dongle Pro+", "funcube_dongle"),
];

pub struct HardwareAdapter;

impl HardwareAdapter {
    pub fn new() -> Self {
        Self
    }

    fn classify_sdr(vendor_id: u16, product_id: u16) -> Option<(&'static str, SdrType)> {
        KNOWN_SDR_DEVICES
            .iter()
            .find(|(vid, pid, _, _)| *vid == vendor_id && *pid == product_id)
            .map(|(_, _, name, type_str)| {
                let sdr_type = match *type_str {
                    "rtl_sdr" => SdrType::RtlSdr,
                    "airspy" => SdrType::Airspy,
                    "airspy_hf" => SdrType::AirspyHf,
                    "hackrf" => SdrType::HackRf,
                    "sdr_play" => SdrType::SdrPlay,
                    "funcube_dongle" => SdrType::FuncubeDongle,
                    _ => SdrType::Unknown,
                };
                (*name, sdr_type)
            })
    }
}

impl HardwarePort for HardwareAdapter {
    fn list_usb_devices(&self) -> Result<Vec<UsbDevice>, AppError> {
        // Read from /sys/bus/usb/devices/ for basic enumeration
        // This works inside containers with /sys mounted
        let mut devices = Vec::new();

        let sys_usb = std::path::Path::new("/sys/bus/usb/devices");
        if !sys_usb.exists() {
            return Ok(devices);
        }

        if let Ok(entries) = std::fs::read_dir(sys_usb) {
            for entry in entries.flatten() {
                let path = entry.path();
                let vendor_path = path.join("idVendor");
                let product_path = path.join("idProduct");

                if vendor_path.exists() && product_path.exists() {
                    let vendor_str = std::fs::read_to_string(&vendor_path).unwrap_or_default();
                    let product_str = std::fs::read_to_string(&product_path).unwrap_or_default();

                    let vendor_id = u16::from_str_radix(vendor_str.trim(), 16).unwrap_or(0);
                    let product_id = u16::from_str_radix(product_str.trim(), 16).unwrap_or(0);

                    if vendor_id == 0 {
                        continue;
                    }

                    let vendor_name = std::fs::read_to_string(path.join("manufacturer"))
                        .ok()
                        .map(|s| s.trim().to_string());
                    let product_name = std::fs::read_to_string(path.join("product"))
                        .ok()
                        .map(|s| s.trim().to_string());
                    let serial = std::fs::read_to_string(path.join("serial"))
                        .ok()
                        .map(|s| s.trim().to_string());
                    let busnum = std::fs::read_to_string(path.join("busnum"))
                        .ok()
                        .and_then(|s| s.trim().parse().ok())
                        .unwrap_or(0);
                    let devnum = std::fs::read_to_string(path.join("devnum"))
                        .ok()
                        .and_then(|s| s.trim().parse().ok())
                        .unwrap_or(0);

                    devices.push(UsbDevice {
                        vendor_id,
                        product_id,
                        vendor_name,
                        product_name,
                        serial,
                        bus: busnum,
                        address: devnum,
                    });
                }
            }
        }

        Ok(devices)
    }

    fn list_sdr_devices(&self) -> Result<Vec<SdrDevice>, AppError> {
        let usb_devices = self.list_usb_devices()?;

        let sdr_devices: Vec<SdrDevice> = usb_devices
            .iter()
            .filter_map(|usb| {
                Self::classify_sdr(usb.vendor_id, usb.product_id).map(|(name, sdr_type)| {
                    let id = format!(
                        "{:04x}:{:04x}-{}",
                        usb.vendor_id,
                        usb.product_id,
                        usb.serial.as_deref().unwrap_or("unknown")
                    );

                    SdrDevice {
                        id,
                        name: usb.product_name.clone().unwrap_or_else(|| name.to_string()),
                        device_type: sdr_type,
                        vendor_id: usb.vendor_id,
                        product_id: usb.product_id,
                        serial: usb.serial.clone(),
                        status: "available".to_string(),
                        assigned_to: None,
                    }
                })
            })
            .collect();

        Ok(sdr_devices)
    }
}
