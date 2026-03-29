use serde::Serialize;

/// Events broadcast to WebSocket clients
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", content = "data")]
pub enum Event {
    ContainerStatusChanged {
        id: String,
        name: String,
        status: String,
    },
    SystemStats {
        cpu_usage: f32,
        memory_percent: f32,
        disk_percent: f32,
        temperature: Option<f32>,
    },
    SdrDeviceChanged {
        action: String,
        device_id: String,
    },
    AppInstalled {
        app_id: String,
        container_id: String,
    },
    AppUninstalled {
        app_id: String,
    },
}
