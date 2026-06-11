use axum::extract::State;
use axum::Json;
use prost::Message;
use serde::{Deserialize, Serialize};

use crate::ports::ConfigPort;
use crate::{AppError, AppState};

/// Aircraft position from readsb's aircraft.json
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Aircraft {
    pub hex: String,
    #[serde(default)]
    pub flight: Option<String>,
    #[serde(default)]
    pub lat: Option<f64>,
    #[serde(default)]
    pub lon: Option<f64>,
    #[serde(default)]
    pub alt_baro: Option<serde_json::Value>,
    #[serde(default)]
    pub gs: Option<f64>,
    #[serde(default)]
    pub track: Option<f64>,
    #[serde(default)]
    pub squawk: Option<String>,
    #[serde(default)]
    pub seen: Option<f64>,
    #[serde(default)]
    pub rssi: Option<f64>,
}

#[derive(Debug, Serialize, Deserialize)]
struct ReadsbResponse {
    #[serde(default)]
    aircraft: Vec<Aircraft>,
    #[serde(default)]
    messages: Option<u64>,
    #[serde(default)]
    now: Option<f64>,
}

/// Minimal readsb protobuf model for /data/aircraft.pb.
///
/// The full schema is larger; tracking only needs the fields that overlap with
/// aircraft.json. Unknown fields are ignored by prost, so this stays compatible
/// with newer readsb-protobuf images.
#[derive(Clone, PartialEq, Message)]
struct ReadsbAircraftsUpdate {
    #[prost(uint64, tag = "1")]
    now: u64,
    #[prost(uint64, tag = "2")]
    messages: u64,
    #[prost(message, repeated, tag = "15")]
    aircraft: Vec<ReadsbAircraftMeta>,
}

#[derive(Clone, PartialEq, Message)]
struct ReadsbAircraftMeta {
    #[prost(uint32, tag = "1")]
    addr: u32,
    #[prost(string, tag = "2")]
    flight: String,
    #[prost(uint32, tag = "3")]
    squawk: u32,
    #[prost(int32, tag = "5")]
    alt_baro: i32,
    #[prost(double, tag = "8")]
    lat: f64,
    #[prost(double, tag = "9")]
    lon: f64,
    #[prost(uint64, tag = "10")]
    messages: u64,
    #[prost(uint64, tag = "11")]
    seen: u64,
    #[prost(float, tag = "12")]
    rssi: f32,
    #[prost(uint32, tag = "23")]
    gs: u32,
    #[prost(int32, tag = "27")]
    track: i32,
}

/// Vehicle unified format for the frontend map
#[derive(Debug, Serialize)]
pub struct Vehicle {
    pub id: String,
    pub callsign: String,
    pub vehicle_type: String,
    pub lat: f64,
    pub lng: f64,
    pub altitude: f64,
    pub speed: f64,
    pub heading: f64,
    pub source: String,
}

#[derive(Debug, Serialize)]
pub struct TrackingResponse {
    pub vehicles: Vec<Vehicle>,
    pub station: StationLocation,
    pub sources: Vec<TrackingSource>,
}

#[derive(Debug, Serialize)]
pub struct StationLocation {
    pub lat: f64,
    pub lng: f64,
}

#[derive(Debug, Serialize)]
pub struct TrackingSource {
    pub name: String,
    pub source_type: String,
    pub vehicle_count: usize,
    pub available: bool,
}

/// Fetch real-time vehicle positions from running decoder containers.
/// Tries to reach readsb (aircraft) and ais-catcher (ships) via Docker network.
pub async fn get_vehicles(
    State(state): State<AppState>,
) -> Result<Json<TrackingResponse>, AppError> {
    let config = state.config.read_config().await?;
    let station = StationLocation {
        lat: config.station.latitude,
        lng: config.station.longitude,
    };

    let mut vehicles = Vec::new();
    let mut sources = Vec::new();

    // Try fetching from readsb (aircraft.json)
    match fetch_readsb_aircraft().await {
        Ok(aircraft) => {
            let count = aircraft.len();
            for ac in aircraft {
                if let (Some(lat), Some(lon)) = (ac.lat, ac.lon) {
                    let alt = match &ac.alt_baro {
                        Some(serde_json::Value::Number(n)) => n.as_f64().unwrap_or(0.0),
                        _ => 0.0,
                    };
                    vehicles.push(Vehicle {
                        id: ac.hex.clone(),
                        callsign: ac
                            .flight
                            .unwrap_or_else(|| ac.hex.clone())
                            .trim()
                            .to_string(),
                        vehicle_type: "aircraft".to_string(),
                        lat,
                        lng: lon,
                        altitude: alt,
                        speed: ac.gs.unwrap_or(0.0),
                        heading: ac.track.unwrap_or(0.0),
                        source: "readsb".to_string(),
                    });
                }
            }
            sources.push(TrackingSource {
                name: "readsb".to_string(),
                source_type: "adsb".to_string(),
                vehicle_count: count,
                available: true,
            });
        }
        Err(_) => {
            sources.push(TrackingSource {
                name: "readsb".to_string(),
                source_type: "adsb".to_string(),
                vehicle_count: 0,
                available: false,
            });
        }
    }

    // Try fetching from ais-catcher
    match fetch_ais_vessels().await {
        Ok(ships) => {
            let count = ships.len();
            vehicles.extend(ships);
            sources.push(TrackingSource {
                name: "ais-catcher".to_string(),
                source_type: "ais".to_string(),
                vehicle_count: count,
                available: true,
            });
        }
        Err(_) => {
            sources.push(TrackingSource {
                name: "ais-catcher".to_string(),
                source_type: "ais".to_string(),
                vehicle_count: 0,
                available: false,
            });
        }
    }

    Ok(Json(TrackingResponse {
        vehicles,
        station,
        sources,
    }))
}

/// Fetch aircraft from readsb container's HTTP endpoint
async fn fetch_readsb_aircraft() -> Result<Vec<Aircraft>, AppError> {
    match fetch_readsb_aircraft_json().await {
        Ok(aircraft) => Ok(aircraft),
        Err(json_err) => match fetch_readsb_aircraft_protobuf().await {
            Ok(aircraft) => Ok(aircraft),
            Err(pb_err) => Err(AppError::Internal(format!(
                "readsb aircraft unavailable: json={json_err}; protobuf={pb_err}"
            ))),
        },
    }
}

async fn fetch_readsb_aircraft_json() -> Result<Vec<Aircraft>, AppError> {
    // Some readsb/tar1090 deployments expose aircraft.json on port 8080 inside
    // the Docker network.
    let url = "http://airwaves-readsb:8080/data/aircraft.json";
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(3))
        .build()
        .map_err(|e| AppError::Internal(e.to_string()))?;

    let resp = client
        .get(url)
        .send()
        .await
        .map_err(|e| AppError::Internal(format!("readsb unreachable: {}", e)))?;

    let data: ReadsbResponse = resp
        .json()
        .await
        .map_err(|e| AppError::Internal(format!("readsb parse error: {}", e)))?;

    Ok(data.aircraft)
}

async fn fetch_readsb_aircraft_protobuf() -> Result<Vec<Aircraft>, AppError> {
    // docker-readsb-protobuf exposes aircraft.pb instead of aircraft.json.
    let url = "http://airwaves-readsb:8080/data/aircraft.pb";
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(3))
        .build()
        .map_err(|e| AppError::Internal(e.to_string()))?;

    let resp = client
        .get(url)
        .send()
        .await
        .map_err(|e| AppError::Internal(format!("readsb protobuf unreachable: {}", e)))?;

    let bytes = resp
        .error_for_status()
        .map_err(|e| AppError::Internal(format!("readsb protobuf HTTP error: {}", e)))?
        .bytes()
        .await
        .map_err(|e| AppError::Internal(format!("readsb protobuf read error: {}", e)))?;

    let data = ReadsbAircraftsUpdate::decode(bytes)
        .map_err(|e| AppError::Internal(format!("readsb protobuf parse error: {}", e)))?;

    Ok(data
        .aircraft
        .into_iter()
        .map(|ac| {
            let squawk = if ac.squawk == 0 {
                None
            } else {
                Some(format!("{:04o}", ac.squawk))
            };
            Aircraft {
                hex: format!("{:06x}", ac.addr),
                flight: (!ac.flight.trim().is_empty()).then_some(ac.flight),
                lat: (ac.lat != 0.0).then_some(ac.lat),
                lon: (ac.lon != 0.0).then_some(ac.lon),
                alt_baro: (ac.alt_baro != 0).then_some(serde_json::json!(ac.alt_baro)),
                gs: (ac.gs != 0).then_some(ac.gs as f64),
                track: (ac.track != 0).then_some(ac.track as f64),
                squawk,
                seen: Some(ac.seen as f64 / 1000.0),
                rssi: Some(ac.rssi as f64),
            }
        })
        .collect())
}

/// Fetch ship positions from ais-catcher container
async fn fetch_ais_vessels() -> Result<Vec<Vehicle>, AppError> {
    // ais-catcher exposes a JSON API on port 8100
    let url = "http://airwaves-ais-catcher:8100/api/ships";
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(3))
        .build()
        .map_err(|e| AppError::Internal(e.to_string()))?;

    let resp = client
        .get(url)
        .send()
        .await
        .map_err(|e| AppError::Internal(format!("ais-catcher unreachable: {}", e)))?;

    // AIS-catcher returns various formats; parse what we can
    let data: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| AppError::Internal(format!("ais-catcher parse error: {}", e)))?;

    let mut ships = Vec::new();
    if let Some(arr) = data.as_array() {
        for ship in arr {
            let mmsi = ship["mmsi"]
                .as_str()
                .or(ship["mmsi"].as_u64().map(|_| ""))
                .map(|s| s.to_string())
                .unwrap_or_default();
            let lat = ship["lat"].as_f64().or(ship["latitude"].as_f64());
            let lon = ship["lon"].as_f64().or(ship["longitude"].as_f64());
            if let (Some(lat), Some(lon)) = (lat, lon) {
                ships.push(Vehicle {
                    id: mmsi.clone(),
                    callsign: ship["shipname"]
                        .as_str()
                        .unwrap_or(&mmsi)
                        .trim()
                        .to_string(),
                    vehicle_type: "ship".to_string(),
                    lat,
                    lng: lon,
                    altitude: 0.0,
                    speed: ship["speed"].as_f64().unwrap_or(0.0),
                    heading: ship["heading"]
                        .as_f64()
                        .or(ship["course"].as_f64())
                        .unwrap_or(0.0),
                    source: "ais-catcher".to_string(),
                });
            }
        }
    }

    Ok(ships)
}
