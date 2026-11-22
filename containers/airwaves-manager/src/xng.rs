//! xng: one decoder process for every SDR and mode, fed to Airframes.
//!
//! The catalog app runs `xng station /etc/xng/station.toml`. Before each
//! install or reconfigure the manager writes that file from the attached SDRs
//! and the app's settings, so the user never edits TOML:
//!
//! - `XNG_STATION_ID`: base station ident. xng appends the mode
//!   (`-ACARS`, `-VDL2`, ...) and feeds each mode to its own Airframes ingest.
//! - `XNG_MODES`: optional per-SDR overrides, `<serial|id>=<mode>[@<gain>]`
//!   separated by commas or newlines. `off` leaves an SDR alone. SDRs not
//!   listed get a mode suited to their hardware (see [`default_mode`]).
//! - `XNG_FEED_AIRFRAMES`: `true` (default) or `false`.
//! - Airband voice: set one SDR to `airband` in `XNG_MODES` and list the
//!   channels in `XNG_AIRBAND_CHANNELS` (`120.500=SFO Tower, 121.800`). When
//!   the device is signed in to Airframes (`atc.json`, written by the
//!   manager's device sign-in) and `XNG_ATC_FEED` isn't `false`, the channels
//!   are fed to the Airframes ATC network. `XNG_TRANSCRIBE=true` adds
//!   on-device transcripts (amd64/arm64 images only).
//!
//! The station position comes from the device settings (config.json
//! `station.latitude/longitude`) when set.

use serde::Serialize;
use std::collections::HashMap;
use std::path::PathBuf;

use crate::domain::{AirwavesConfig, CatalogApp, SdrDevice, SdrType};
use crate::error::AppError;

pub const APP_ID: &str = "xng";
pub const STATION_ID_KEY: &str = "XNG_STATION_ID";
pub const MODES_KEY: &str = "XNG_MODES";
pub const FEED_KEY: &str = "XNG_FEED_AIRFRAMES";
pub const AIRBAND_CHANNELS_KEY: &str = "XNG_AIRBAND_CHANNELS";
pub const ATC_FEED_KEY: &str = "XNG_ATC_FEED";
pub const ATC_NAME_KEY: &str = "XNG_ATC_STATION_NAME";
pub const TRANSCRIBE_KEY: &str = "XNG_TRANSCRIBE";
/// ATC credentials inside the container (the app dir is mounted at /etc/xng).
pub const CONTAINER_ATC_CREDENTIALS: &str = "/etc/xng/atc.json";

/// Modes the station file may name: the ones xng runs from a plan without
/// hand-picked channels, plus `airband` (channels from XNG_AIRBAND_CHANNELS).
pub const MODES: &[&str] = &[
    "airband", "acars", "vdl2", "hfdl", "adsb", "uat", "ais", "iridium", "std-c", "sonde",
];

/// Manager-side directory holding per-app config (`<dir>/<app id>/...`).
pub fn apps_dir() -> PathBuf {
    std::env::var("AIRWAVES_APPS_DIR")
        .ok()
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/etc/airwaves/apps"))
}

/// The same directory as the Docker host sees it, for bind mounts. Equal to
/// [`apps_dir`] on devices (/etc/airwaves is mounted at the same path); the
/// dev compose file overrides it.
pub fn host_apps_dir() -> PathBuf {
    std::env::var("AIRWAVES_HOST_APPS_DIR")
        .ok()
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(apps_dir)
}

#[derive(Debug, Clone, PartialEq)]
pub struct Session {
    pub sdr: String,
    pub mode: String,
    pub gain: Option<f64>,
}

#[derive(Debug, Clone, PartialEq)]
struct Override {
    mode: String,
    gain: Option<f64>,
}

/// The SoapySDR / native driver name for a device, or None when xng can't
/// open it.
pub fn driver(device_type: &SdrType) -> Option<&'static str> {
    match device_type {
        SdrType::RtlSdr => Some("rtlsdr"),
        SdrType::Airspy => Some("airspy"),
        SdrType::AirspyHf => Some("airspyhf"),
        SdrType::HackRf => Some("hackrf"),
        SdrType::SdrPlay => Some("sdrplay"),
        SdrType::FuncubeDongle | SdrType::Unknown => None,
    }
}

/// What an SDR does when the user hasn't chosen: HF receivers decode HFDL;
/// RTL-SDRs take ACARS, VDL2, ADS-B and AIS in turn; other wideband SDRs
/// start on VDL2.
pub fn default_mode(device_type: &SdrType, rtl_index: usize) -> &'static str {
    match device_type {
        SdrType::AirspyHf => "hfdl",
        SdrType::RtlSdr => ["acars", "vdl2", "adsb", "ais"][rtl_index % 4],
        _ => "vdl2",
    }
}

fn parse_overrides(raw: &str) -> Result<HashMap<String, Override>, String> {
    let mut out = HashMap::new();
    for item in raw
        .split([',', '\n'])
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        let (key, value) = item
            .split_once('=')
            .ok_or_else(|| format!("'{item}' should look like <serial>=<mode>"))?;
        let (mode, gain) = match value.split_once('@') {
            Some((m, g)) => {
                let g = g
                    .trim()
                    .parse::<f64>()
                    .map_err(|_| format!("'{item}': gain must be a number of dB"))?;
                (m.trim(), Some(g))
            }
            None => (value.trim(), None),
        };
        let mode = mode.to_ascii_lowercase();
        if mode != "off" && !MODES.contains(&mode.as_str()) {
            return Err(format!(
                "'{item}': unknown mode '{mode}' (use one of {} or off)",
                MODES.join(", ")
            ));
        }
        out.insert(key.trim().to_string(), Override { mode, gain });
    }
    Ok(out)
}

/// One session per usable SDR, in device order.
pub fn sessions<'a>(
    devices: impl IntoIterator<Item = &'a SdrDevice>,
    modes: &str,
) -> Result<Vec<Session>, String> {
    let overrides = parse_overrides(modes)?;
    let mut rtl_index = 0;
    let mut out = Vec::new();
    for d in devices {
        let Some(drv) = driver(&d.device_type) else {
            continue;
        };
        let serial = d
            .configured_serial
            .as_deref()
            .or(d.serial.as_deref())
            .map(str::trim)
            .filter(|s| !s.is_empty());
        let chosen = serial
            .and_then(|s| overrides.get(s))
            .or_else(|| overrides.get(&d.id));
        let is_rtl = matches!(d.device_type, SdrType::RtlSdr);
        let (mode, gain) = match chosen {
            Some(o) => (o.mode.clone(), o.gain),
            None => (default_mode(&d.device_type, rtl_index).to_string(), None),
        };
        if is_rtl {
            rtl_index += 1;
        }
        if mode == "off" {
            continue;
        }
        let sdr = match serial {
            Some(s) => format!("driver={drv},serial={s}"),
            None => format!("driver={drv}"),
        };
        out.push(Session { sdr, mode, gain });
    }
    Ok(out)
}

#[derive(Serialize)]
#[serde(rename_all = "kebab-case")]
struct StationFile<'a> {
    station_id: &'a str,
    outputs: Outputs,
    session: Vec<SessionFile<'a>>,
}

#[derive(Serialize)]
#[serde(rename_all = "kebab-case")]
struct Outputs {
    feed_airframes: bool,
    http: &'static str,
    metrics: &'static str,
    airframes: AirframesOutput,
    #[serde(skip_serializing_if = "Option::is_none")]
    airband: Option<AirbandOutputsFile>,
}

#[derive(Serialize)]
#[serde(rename_all = "kebab-case")]
struct AirframesOutput {
    enabled: bool,
    auto_suffix: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "kebab-case")]
struct SessionFile<'a> {
    sdr: &'a str,
    mode: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    gain: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    receiver_pos: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    airband: Option<SessionAirbandFile>,
}

/// Airband settings for the station file.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Airband {
    /// (frequency, label)
    pub channels: Vec<(String, Option<String>)>,
    /// Feed the Airframes ATC network under this station name.
    pub atc: Option<String>,
    pub transcribe: bool,
}

/// Widest spread of airband channels one radio is set up for. xng captures
/// 2.4 MHz around the middle of the channels; 2 MHz keeps every channel
/// clear of the band edges on RTL-SDR and Airspy alike.
pub const AIRBAND_MAX_SPAN_MHZ: f64 = 2.0;

/// Parse `XNG_AIRBAND_CHANNELS`: `freq[=label]` separated by commas or
/// newlines. Frequencies are MHz between 108 and 400, all within
/// [`AIRBAND_MAX_SPAN_MHZ`] of each other.
pub fn parse_airband_channels(raw: &str) -> Result<Vec<(String, Option<String>)>, String> {
    let mut out: Vec<(String, Option<String>)> = Vec::new();
    for item in raw
        .split([',', '\n'])
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        let (freq, label) = match item.split_once('=') {
            Some((f, l)) => (f.trim(), Some(l.trim()).filter(|l| !l.is_empty())),
            None => (item, None),
        };
        let mhz: f64 = freq
            .parse()
            .map_err(|_| format!("'{item}': the frequency should be in MHz, like 120.500"))?;
        if !(108.0..=400.0).contains(&mhz) {
            return Err(format!(
                "'{item}': {mhz} MHz is outside the airband (108 to 400 MHz)"
            ));
        }
        let freq = format!("{mhz:.3}");
        if out.iter().any(|(f, _)| *f == freq) {
            return Err(format!("{freq} MHz is listed twice"));
        }
        if let Some(l) = label {
            if l.len() > 60 || l.chars().any(char::is_control) {
                return Err(format!(
                    "'{item}': labels are one line of at most 60 characters"
                ));
            }
        }
        out.push((freq, label.map(str::to_string)));
    }
    let mhz = |(f, _): &(String, Option<String>)| f.parse::<f64>().unwrap_or(0.0);
    if let (Some(lo), Some(hi)) = (
        out.iter().map(mhz).reduce(f64::min),
        out.iter().map(mhz).reduce(f64::max),
    ) {
        if hi - lo > AIRBAND_MAX_SPAN_MHZ + 1e-9 {
            return Err(format!(
                "the channels span {lo:.3} to {hi:.3} MHz; one radio can hear channels within {AIRBAND_MAX_SPAN_MHZ} MHz of each other"
            ));
        }
    }
    Ok(out)
}

#[derive(Serialize)]
#[serde(rename_all = "kebab-case")]
struct AirbandOutputsFile {
    #[serde(skip_serializing_if = "Option::is_none")]
    atc: Option<AtcFile>,
    #[serde(skip_serializing_if = "Option::is_none")]
    transcribe: Option<TranscribeFile>,
}

#[derive(Serialize)]
#[serde(rename_all = "kebab-case")]
struct AtcFile {
    credentials: &'static str,
    station_name: String,
    location_precision_m: u32,
}

#[derive(Serialize)]
struct TranscribeFile {}

#[derive(Serialize)]
#[serde(rename_all = "kebab-case")]
struct SessionAirbandFile {
    channel: Vec<ChannelFile>,
}

#[derive(Serialize)]
#[serde(rename_all = "kebab-case")]
struct ChannelFile {
    freq: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    label: Option<String>,
}

/// Render `station.toml`.
pub fn render_station(
    station_id: &str,
    feed_airframes: bool,
    receiver_pos: Option<(f64, f64)>,
    sessions: &[Session],
    airband: &Airband,
) -> String {
    let pos = receiver_pos.map(|(lat, lon)| format!("{lat:.4},{lon:.4}"));
    let has_airband = sessions.iter().any(|s| s.mode == "airband");
    let airband_outputs =
        (has_airband && (airband.atc.is_some() || airband.transcribe)).then(|| {
            AirbandOutputsFile {
                atc: airband.atc.as_ref().map(|name| AtcFile {
                    credentials: CONTAINER_ATC_CREDENTIALS,
                    station_name: name.clone(),
                    location_precision_m: 500,
                }),
                transcribe: airband.transcribe.then_some(TranscribeFile {}),
            }
        });
    let file = StationFile {
        station_id,
        outputs: Outputs {
            feed_airframes,
            http: "0.0.0.0:8080",
            metrics: "0.0.0.0:9090",
            airframes: AirframesOutput {
                enabled: feed_airframes,
                auto_suffix: true,
            },
            airband: airband_outputs,
        },
        session: sessions
            .iter()
            .map(|s| SessionFile {
                sdr: &s.sdr,
                mode: &s.mode,
                gain: s.gain,
                receiver_pos: pos.clone(),
                airband: (s.mode == "airband").then(|| SessionAirbandFile {
                    channel: airband
                        .channels
                        .iter()
                        .map(|(freq, label)| ChannelFile {
                            freq: freq.clone(),
                            label: label.clone(),
                        })
                        .collect(),
                }),
            })
            .collect(),
    };
    let body = toml::to_string(&file).expect("station file serializes");
    format!(
        "# Written by airwaves-manager from the xng app settings; edits are\n\
         # replaced on the next install or reconfigure.\n{body}"
    )
}

/// Station idents are short uppercase tokens (letters, digits, '-', '_').
pub fn normalize_station_id(raw: &str) -> Result<String, String> {
    let id = raw.trim().to_ascii_uppercase();
    if id.is_empty() {
        return Err(
            "Set a station ID (for example XX-KSEA) so Airframes can credit your feed.".into(),
        );
    }
    if id.len() > 40
        || !id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err(format!(
            "Station ID '{raw}' may only use letters, digits, '-' and '_' (40 characters at most)."
        ));
    }
    Ok(id)
}

fn receiver_pos(config: &AirwavesConfig) -> Option<(f64, f64)> {
    let (lat, lon) = (config.station.latitude, config.station.longitude);
    let valid = lat.is_finite()
        && lon.is_finite()
        && (-90.0..=90.0).contains(&lat)
        && (-180.0..=180.0).contains(&lon)
        && !(lat == 0.0 && lon == 0.0);
    valid.then_some((lat, lon))
}

/// Write the station file for `app` (the xng catalog entry with the user's
/// env merged in) and give the container the whole USB bus, since its
/// sessions can open any attached SDR.
pub async fn prepare(
    app: &mut CatalogApp,
    config: &AirwavesConfig,
    devices: &[SdrDevice],
) -> Result<(), AppError> {
    let env = &app.env;
    let flag = |key: &str, default: bool| {
        env.get(key)
            .map(|v| v.trim().to_ascii_lowercase())
            .filter(|v| !v.is_empty())
            .map(|v| !matches!(v.as_str(), "false" | "0" | "no" | "off"))
            .unwrap_or(default)
    };
    let station_id =
        normalize_station_id(env.get(STATION_ID_KEY).map(String::as_str).unwrap_or(""))
            .map_err(AppError::BadRequest)?;
    let feed = flag(FEED_KEY, true);
    // Leave SDRs that other apps were given to those apps.
    let taken: Vec<_> = crate::sdr::assignments_from_config(config)
        .into_iter()
        .filter(|r| r.app_id.as_deref() != Some(APP_ID))
        .collect();
    let free: Vec<&SdrDevice> = devices
        .iter()
        .filter(|d| {
            !taken.iter().any(|r| {
                r.device_id.as_deref() == Some(d.id.as_str())
                    || match (r.serial.as_deref(), d.serial.as_deref()) {
                        (Some(a), Some(b)) => {
                            crate::sdr::serial_key(a) == crate::sdr::serial_key(b)
                        }
                        _ => false,
                    }
            })
        })
        .collect();
    let sessions = sessions(free, env.get(MODES_KEY).map(String::as_str).unwrap_or(""))
        .map_err(AppError::BadRequest)?;
    if sessions.is_empty() {
        return Err(AppError::BadRequest(
            "xng found no free SDR. Plug one in, free one from another app, or check XNG_MODES for SDRs set to off."
                .into(),
        ));
    }

    let mut airband = Airband::default();
    let airband_sessions = sessions.iter().filter(|s| s.mode == "airband").count();
    if airband_sessions > 1 {
        return Err(AppError::BadRequest(
            "Only one SDR can run airband for now; set the others to another mode.".into(),
        ));
    }
    if airband_sessions == 1 {
        airband.channels = parse_airband_channels(
            env.get(AIRBAND_CHANNELS_KEY)
                .map(String::as_str)
                .unwrap_or(""),
        )
        .map_err(AppError::BadRequest)?;
        if airband.channels.is_empty() {
            return Err(AppError::BadRequest(
                "An SDR is set to airband: list its channels in XNG_AIRBAND_CHANNELS, e.g. 120.500=Tower, 121.800=Ground.".into(),
            ));
        }
        if flag(ATC_FEED_KEY, true) {
            if crate::airframes::signed_in().await {
                let name = env
                    .get(ATC_NAME_KEY)
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .unwrap_or_else(|| config.device.name.clone());
                airband.atc = Some(if name.trim().is_empty() {
                    station_id.clone()
                } else {
                    name
                });
            } else {
                tracing::info!(
                    "xng airband: not signed in to Airframes, so the ATC feed stays off"
                );
            }
        }
        airband.transcribe = flag(TRANSCRIBE_KEY, false);
    }

    let toml = render_station(&station_id, feed, receiver_pos(config), &sessions, &airband);
    let dir = apps_dir().join(APP_ID);
    tokio::fs::create_dir_all(&dir)
        .await
        .map_err(|e| AppError::Internal(format!("create {}: {e}", dir.display())))?;
    let path = dir.join("station.toml");
    tokio::fs::write(&path, toml)
        .await
        .map_err(|e| AppError::Internal(format!("write {}: {e}", path.display())))?;

    app.env.insert(STATION_ID_KEY.to_string(), station_id);
    app.env.insert(
        crate::sdr::SDR_USB_ACCESS_ENV_KEY.to_string(),
        "full-bus".to_string(),
    );
    tracing::info!(
        "xng station file written: {} session(s): {}",
        sessions.len(),
        sessions
            .iter()
            .map(|s| format!("{} on {}", s.mode, s.sdr))
            .collect::<Vec<_>>()
            .join(", ")
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dev(id: &str, t: SdrType, serial: Option<&str>) -> SdrDevice {
        SdrDevice {
            id: id.to_string(),
            name: id.to_string(),
            device_type: t,
            vendor_id: 0,
            product_id: 0,
            serial: serial.map(str::to_string),
            status: "available".to_string(),
            assigned_to: None,
            configured_name: None,
            configured_serial: None,
        }
    }

    #[test]
    fn defaults_follow_the_hardware() {
        let devices = vec![
            dev("a", SdrType::RtlSdr, Some("00000001")),
            dev("b", SdrType::AirspyHf, Some("abc")),
            dev("c", SdrType::RtlSdr, Some("00000002")),
            dev("d", SdrType::FuncubeDongle, None),
        ];
        let s = sessions(&devices, "").unwrap();
        assert_eq!(
            s.iter()
                .map(|s| (s.sdr.as_str(), s.mode.as_str()))
                .collect::<Vec<_>>(),
            vec![
                ("driver=rtlsdr,serial=00000001", "acars"),
                ("driver=airspyhf,serial=abc", "hfdl"),
                ("driver=rtlsdr,serial=00000002", "vdl2"),
            ]
        );
    }

    #[test]
    fn overrides_pick_mode_gain_or_off() {
        let devices = vec![
            dev("a", SdrType::RtlSdr, Some("00000001")),
            dev("b", SdrType::RtlSdr, Some("00000002")),
            dev("usb-1-3", SdrType::RtlSdr, None),
        ];
        let s = sessions(&devices, "00000001=adsb@42.1\n00000002=off, usb-1-3=AIS").unwrap();
        assert_eq!(
            s,
            vec![
                Session {
                    sdr: "driver=rtlsdr,serial=00000001".into(),
                    mode: "adsb".into(),
                    gain: Some(42.1)
                },
                Session {
                    sdr: "driver=rtlsdr".into(),
                    mode: "ais".into(),
                    gain: None
                },
            ]
        );
    }

    #[test]
    fn bad_overrides_are_explained() {
        let d = vec![dev("a", SdrType::RtlSdr, Some("1"))];
        assert!(sessions(&d, "1=fm").unwrap_err().contains("unknown mode"));
        assert!(sessions(&d, "1=acars@loud").unwrap_err().contains("gain"));
        assert!(sessions(&d, "acars")
            .unwrap_err()
            .contains("<serial>=<mode>"));
    }

    #[test]
    fn station_file_is_valid_toml_with_auto_suffix() {
        let s = vec![Session {
            sdr: "driver=rtlsdr,serial=1".into(),
            mode: "acars".into(),
            gain: Some(40.0),
        }];
        let out = render_station(
            "XX-KSEA",
            true,
            Some((47.6205, -122.3493)),
            &s,
            &Airband::default(),
        );
        let v: toml::Value = toml::from_str(&out).unwrap();
        assert_eq!(v["station-id"].as_str(), Some("XX-KSEA"));
        assert_eq!(v["outputs"]["feed-airframes"].as_bool(), Some(true));
        assert_eq!(
            v["outputs"]["airframes"]["auto-suffix"].as_bool(),
            Some(true)
        );
        assert_eq!(v["session"][0]["mode"].as_str(), Some("acars"));
        assert_eq!(v["session"][0]["gain"].as_float(), Some(40.0));
        assert_eq!(
            v["session"][0]["receiver-pos"].as_str(),
            Some("47.6205,-122.3493")
        );
    }

    #[test]
    fn station_ids_are_checked() {
        assert_eq!(normalize_station_id(" xx-ksea ").unwrap(), "XX-KSEA");
        assert!(normalize_station_id("").is_err());
        assert!(normalize_station_id("XX KSEA").is_err());
        assert!(normalize_station_id("XX\"]\n[x").is_err());
    }

    #[test]
    fn airband_channels_parse_and_reject_junk() {
        let c = parse_airband_channels("120.5=SFO Tower,\n121.8").unwrap();
        assert_eq!(
            c,
            vec![
                ("120.500".to_string(), Some("SFO Tower".to_string())),
                ("121.800".to_string(), None)
            ]
        );
        assert!(parse_airband_channels("88.5")
            .unwrap_err()
            .contains("outside"));
        assert!(parse_airband_channels("tower").unwrap_err().contains("MHz"));
        assert!(parse_airband_channels("118.0,120.0").is_ok());
        assert!(parse_airband_channels("118.0,120.5")
            .unwrap_err()
            .contains("within 2 MHz"));
        assert!(parse_airband_channels("120.5,120.500")
            .unwrap_err()
            .contains("twice"));
    }

    #[test]
    fn airband_session_renders_channels_and_atc() {
        let s = vec![
            Session {
                sdr: "driver=rtlsdr,serial=1".into(),
                mode: "acars".into(),
                gain: None,
            },
            Session {
                sdr: "driver=rtlsdr,serial=2".into(),
                mode: "airband".into(),
                gain: None,
            },
        ];
        let ab = Airband {
            channels: vec![
                ("120.500".into(), Some("SFO \"Tower\"".into())),
                ("121.800".into(), None),
            ],
            atc: Some("Ridge".into()),
            transcribe: true,
        };
        let out = render_station("XX-KSFO", true, None, &s, &ab);
        let v: toml::Value = toml::from_str(&out).unwrap();
        assert!(v["session"][0].get("airband").is_none());
        let ch = &v["session"][1]["airband"]["channel"];
        assert_eq!(ch[0]["freq"].as_str(), Some("120.500"));
        assert_eq!(ch[0]["label"].as_str(), Some("SFO \"Tower\""));
        assert!(ch[1].get("label").is_none());
        let atc = &v["outputs"]["airband"]["atc"];
        assert_eq!(atc["credentials"].as_str(), Some(CONTAINER_ATC_CREDENTIALS));
        assert_eq!(atc["station-name"].as_str(), Some("Ridge"));
        assert!(v["outputs"]["airband"].get("transcribe").is_some());
    }

    #[test]
    fn no_airband_outputs_without_an_airband_session() {
        let s = vec![Session {
            sdr: "driver=rtlsdr".into(),
            mode: "acars".into(),
            gain: None,
        }];
        let ab = Airband {
            atc: Some("x".into()),
            ..Default::default()
        };
        let v: toml::Value = toml::from_str(&render_station("XX", true, None, &s, &ab)).unwrap();
        assert!(v["outputs"].get("airband").is_none());
    }
}
