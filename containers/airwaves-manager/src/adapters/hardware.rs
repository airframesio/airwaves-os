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

pub struct HardwareAdapter {
    simulate: bool,
}

impl HardwareAdapter {
    pub fn new() -> Self {
        let simulate = std::env::var("SIMULATE_HARDWARE")
            .map(|v| v == "true" || v == "1")
            .unwrap_or(false);
        if simulate {
            tracing::info!("Hardware simulation enabled (SIMULATE_HARDWARE=true)");
        }
        Self { simulate }
    }

    fn simulated_sdr_devices() -> Vec<SdrDevice> {
        vec![
            SdrDevice {
                id: "0bda:2838-00000101-bus001-dev004".to_string(),
                name: "RTL-SDR Blog V4".to_string(),
                device_type: SdrType::RtlSdr,
                vendor_id: 0x0bda,
                product_id: 0x2838,
                serial: Some("00000101".to_string()),
                status: "available".to_string(),
                assigned_to: None,
                configured_name: None,
                configured_serial: None,
            },
            SdrDevice {
                id: "0bda:2838-00000102-bus001-dev005".to_string(),
                name: "RTL-SDR Blog V3".to_string(),
                device_type: SdrType::RtlSdr,
                vendor_id: 0x0bda,
                product_id: 0x2838,
                serial: Some("00000102".to_string()),
                status: "available".to_string(),
                assigned_to: None,
                configured_name: None,
                configured_serial: None,
            },
            SdrDevice {
                id: "1d50:60a1-AIRSPY-MINI-bus002-dev003".to_string(),
                name: "Airspy Mini".to_string(),
                device_type: SdrType::Airspy,
                vendor_id: 0x1d50,
                product_id: 0x60a1,
                serial: Some("AIRSPY-MINI".to_string()),
                status: "available".to_string(),
                assigned_to: None,
                configured_name: None,
                configured_serial: None,
            },
        ]
    }

    fn simulated_usb_devices() -> Vec<UsbDevice> {
        vec![
            UsbDevice {
                vendor_id: 0x0bda,
                product_id: 0x2838,
                vendor_name: Some("Realtek Semiconductor Corp.".to_string()),
                product_name: Some("RTL2838 DVB-T".to_string()),
                serial: Some("00000101".to_string()),
                bus: 1,
                address: 4,
            },
            UsbDevice {
                vendor_id: 0x0bda,
                product_id: 0x2838,
                vendor_name: Some("Realtek Semiconductor Corp.".to_string()),
                product_name: Some("RTL2838 DVB-T".to_string()),
                serial: Some("00000102".to_string()),
                bus: 1,
                address: 5,
            },
            UsbDevice {
                vendor_id: 0x1d50,
                product_id: 0x60a1,
                vendor_name: Some("OpenMoko, Inc.".to_string()),
                product_name: Some("Airspy Mini".to_string()),
                serial: Some("AIRSPY-MINI".to_string()),
                bus: 2,
                address: 3,
            },
        ]
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
        if self.simulate {
            return Ok(Self::simulated_usb_devices());
        }

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
        if self.simulate {
            return Ok(Self::simulated_sdr_devices());
        }

        let usb_devices = self.list_usb_devices()?;

        let sdr_devices: Vec<SdrDevice> = usb_devices
            .iter()
            .filter_map(|usb| {
                Self::classify_sdr(usb.vendor_id, usb.product_id).map(|(name, sdr_type)| {
                    let id = format!(
                        "{:04x}:{:04x}-{}-bus{:03}-dev{:03}",
                        usb.vendor_id,
                        usb.product_id,
                        usb.serial.as_deref().unwrap_or("unknown"),
                        usb.bus,
                        usb.address
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
                        configured_name: None,
                        configured_serial: None,
                    }
                })
            })
            .collect();

        Ok(sdr_devices)
    }
}
