use bollard::container::{
    Config, CreateContainerOptions, ListContainersOptions, LogOutput, LogsOptions,
    RemoveContainerOptions, StartContainerOptions, StatsOptions, StopContainerOptions,
};
use bollard::system::EventsOptions;
use bollard::Docker;
use futures::StreamExt;
use std::collections::{HashMap, HashSet};
use std::pin::Pin;

use crate::domain::*;
use crate::error::AppError;
use crate::ports::DockerPort;

pub struct DockerAdapter {
    client: Docker,
}

fn runtime_env_for_app(app: &CatalogApp) -> HashMap<String, String> {
    let mut env = app.env.clone();

    // The sdr-enthusiasts acarsdec image supports native RTL-SDR selection via
    // RTL_SERIAL. With KerberosSDR-style multi-tuner devices, SoapySDR can open
    // the selected exact USB node, but it logs alarming probe errors for the
    // intentionally hidden sibling tuners. Use the native path for RTL serials
    // and grant full USB bus access; the serial still pins the intended tuner.
    if app.id == "acarsdec" {
        normalize_acarsdec_gain(&mut env);

        if let Some(soapy) = env.get("SOAPYSDR").cloned() {
            let driver =
                crate::sdr::driver_from_sdr_value(&soapy).unwrap_or_else(|| "rtlsdr".to_string());
            if driver.eq_ignore_ascii_case("rtlsdr") {
                if let Some(serial) = crate::sdr::serial_from_sdr_value(&soapy) {
                    env.remove("SOAPYSDR");
                    env.insert("RTL_SERIAL".to_string(), serial);
                    env.insert(
                        crate::sdr::SDR_USB_ACCESS_ENV_KEY.to_string(),
                        "full-bus".to_string(),
                    );
                }
            }
        }
    }

    env
}

fn normalize_acarsdec_gain(env: &mut HashMap<String, String>) {
    let Some(gain) = env.get("GAIN").cloned() else {
        return;
    };

    let normalized = gain.trim();
    if normalized.is_empty() {
        env.remove("GAIN");
        return;
    }

    // docker-acarsdec uses -10 for AGC/autogain. The image rejects textual
    // aliases such as "auto", which older Airwaves catalog defaults used.
    if matches!(
        normalized.to_ascii_lowercase().as_str(),
        "auto" | "autogain" | "agc"
    ) {
        env.insert("GAIN".to_string(), "-10".to_string());
    }
}

impl DockerAdapter {
    pub async fn new() -> anyhow::Result<Self> {
        let client = Docker::connect_with_socket_defaults()?;
        // Verify connection
        client.ping().await?;
        tracing::info!("Connected to Docker daemon");
        Ok(Self { client })
    }
}

impl DockerAdapter {
    /// Create a bridge network if it doesn't already exist (idempotent).
    async fn ensure_network(&self, name: &str) -> Result<(), AppError> {
        if self
            .client
            .inspect_network::<String>(name, None)
            .await
            .is_ok()
        {
            return Ok(());
        }
        match self
            .client
            .create_network(bollard::network::CreateNetworkOptions {
                name: name.to_string(),
                driver: "bridge".to_string(),
                ..Default::default()
            })
            .await
        {
            Ok(_) => {
                tracing::info!("Created Docker network: {}", name);
                Ok(())
            }
            // Race: another caller created it between inspect and create.
            Err(bollard::errors::Error::DockerResponseServerError {
                status_code: 409, ..
            }) => Ok(()),
            Err(e) => Err(AppError::Docker(e)),
        }
    }

    /// Read a single label value from a container's image config (via the
    /// Docker socket). Returns None if the container/label is absent.
    pub async fn container_label(&self, name: &str, label: &str) -> Option<String> {
        let info = self.client.inspect_container(name, None).await.ok()?;
        info.config?.labels?.get(label).cloned()
    }

    /// The tag portion of a running container's image reference, e.g. the
    /// "1.0.10-dev.60" of "ghcr.io/airframesio/airwaves-manager:1.0.10-dev.60".
    /// This is the concrete version actually deployed, which is more precise than
    /// any version baked into the binary. Returns None if absent or untagged
    /// (e.g. pinned by digest), or for the non-informative "latest" tag.
    pub async fn container_image_tag(&self, name: &str) -> Option<String> {
        let info = self.client.inspect_container(name, None).await.ok()?;
        let image = info.config?.image?;
        // Strip any registry host (which may contain a port colon) before
        // taking the tag after the final ':'. "host:5000/img:tag" -> "tag".
        let last = image.rsplit('/').next().unwrap_or(&image);
        let (_, tag) = last.rsplit_once(':')?;
        let tag = tag.trim();
        if tag.is_empty() || tag == "latest" || tag.starts_with("sha256") {
            return None;
        }
        Some(tag.to_string())
    }

    /// Returns a stream of Docker daemon events (container start/stop/die/etc.)
    pub async fn watch_events(
        &self,
    ) -> Pin<
        Box<
            dyn futures::Stream<
                    Item = Result<bollard::models::EventMessage, bollard::errors::Error>,
                > + Send
                + '_,
        >,
    > {
        let opts = EventsOptions::<String> {
            filters: {
                let mut f = HashMap::new();
                f.insert("type".to_string(), vec!["container".to_string()]);
                f
            },
            ..Default::default()
        };
        Box::pin(self.client.events(Some(opts)))
    }

    /// Returns a stream of log lines from a container (for WebSocket streaming)
    pub async fn stream_logs(
        &self,
        id: &str,
        opts: LogsOptions<String>,
    ) -> Pin<Box<dyn futures::Stream<Item = Result<String, bollard::errors::Error>> + Send + '_>>
    {
        let stream = self.client.logs(id, Some(opts));
        Box::pin(stream.map(|result| {
            result.map(|output| match output {
                LogOutput::StdOut { message } | LogOutput::StdErr { message } => {
                    String::from_utf8_lossy(&message).to_string()
                }
                _ => String::new(),
            })
        }))
    }
}

impl DockerPort for DockerAdapter {
    async fn list_containers(&self) -> Result<Vec<ContainerInfo>, AppError> {
        // List all containers (both airwaves-managed and others)
        let opts = ListContainersOptions::<String> {
            all: true,
            ..Default::default()
        };

        let containers = self.client.list_containers(Some(opts)).await?;

        let result: Vec<ContainerInfo> = containers
            .into_iter()
            .map(|c| {
                let name = c
                    .names
                    .as_ref()
                    .and_then(|n| n.first())
                    .map(|n| n.trim_start_matches('/').to_string())
                    .unwrap_or_default();

                let ports = c
                    .ports
                    .unwrap_or_default()
                    .into_iter()
                    .map(|p| PortBinding {
                        container_port: p.private_port,
                        host_port: p.public_port,
                        protocol: p
                            .typ
                            .map(|t| format!("{:?}", t))
                            .unwrap_or_else(|| "tcp".to_string()),
                    })
                    .collect();

                ContainerInfo {
                    id: c.id.unwrap_or_default(),
                    name,
                    image: c.image.unwrap_or_default(),
                    status: c.status.unwrap_or_default(),
                    state: c.state.unwrap_or_default(),
                    created: c.created.unwrap_or(0),
                    ports,
                }
            })
            .collect();

        Ok(result)
    }

    async fn prune_unrecorded_app_containers(
        &self,
        recorded_ids: &HashSet<String>,
    ) -> Result<Vec<String>, AppError> {
        let opts = ListContainersOptions::<String> {
            all: true,
            ..Default::default()
        };
        let containers = self.client.list_containers(Some(opts)).await?;
        let mut removed = Vec::new();

        for container in containers {
            let name = container
                .names
                .as_ref()
                .and_then(|names| names.first())
                .map(|name| name.trim_start_matches('/').to_string())
                .unwrap_or_default();
            if !name.starts_with("airwaves-")
                || name == "airwaves-manager"
                || name == "airwaves-gateway"
            {
                continue;
            }

            let Some(id) = container.id else {
                continue;
            };
            let labels = container.labels.unwrap_or_default();
            if labels.get("managed-by").map(String::as_str) != Some("airwaves") {
                continue;
            }
            let app_id = labels
                .get("airwaves-app-id")
                .cloned()
                .unwrap_or_else(|| name.trim_start_matches("airwaves-").to_string());
            if recorded_ids.contains(&app_id) {
                continue;
            }

            self.client
                .remove_container(
                    &id,
                    Some(RemoveContainerOptions {
                        force: true,
                        ..Default::default()
                    }),
                )
                .await?;
            removed.push(name);
        }

        Ok(removed)
    }

    async fn start_container(&self, id: &str) -> Result<(), AppError> {
        self.client
            .start_container(id, None::<StartContainerOptions<String>>)
            .await?;
        Ok(())
    }

    async fn stop_container(&self, id: &str) -> Result<(), AppError> {
        self.client
            .stop_container(id, Some(StopContainerOptions { t: 10 }))
            .await?;
        Ok(())
    }

    async fn restart_container(&self, id: &str) -> Result<(), AppError> {
        self.client
            .restart_container(
                id,
                Some(bollard::container::RestartContainerOptions { t: 10 }),
            )
            .await?;
        Ok(())
    }

    async fn get_logs(&self, id: &str, tail: usize) -> Result<String, AppError> {
        let opts = LogsOptions::<String> {
            stdout: true,
            stderr: true,
            tail: tail.to_string(),
            ..Default::default()
        };

        let mut stream = self.client.logs(id, Some(opts));
        let mut output = String::new();

        while let Some(msg) = stream.next().await {
            match msg {
                Ok(LogOutput::StdOut { message }) | Ok(LogOutput::StdErr { message }) => {
                    output.push_str(&String::from_utf8_lossy(&message));
                }
                _ => {}
            }
        }

        Ok(output)
    }

    async fn container_stats(&self) -> Result<Vec<ContainerStats>, AppError> {
        // List running containers, then sample one non-streaming stats point each.
        let opts = ListContainersOptions::<String> {
            all: false,
            ..Default::default()
        };
        let containers = self.client.list_containers(Some(opts)).await?;

        let mut results = Vec::new();
        for c in containers {
            let id = match c.id {
                Some(id) => id,
                None => continue,
            };
            let name = c
                .names
                .as_ref()
                .and_then(|n| n.first())
                .map(|n| n.trim_start_matches('/').to_string())
                .unwrap_or_default();

            let mut stream = self.client.stats(
                &id,
                // one_shot:false so Docker takes two samples internally and
                // populates precpu_stats, making the CPU% delta meaningful.
                Some(StatsOptions {
                    stream: false,
                    one_shot: false,
                }),
            );

            if let Some(Ok(s)) = stream.next().await {
                // CPU%: delta of container CPU vs system CPU, scaled by online CPUs.
                let cpu_delta = s.cpu_stats.cpu_usage.total_usage as f64
                    - s.precpu_stats.cpu_usage.total_usage as f64;
                let sys_delta = s
                    .cpu_stats
                    .system_cpu_usage
                    .unwrap_or(0)
                    .saturating_sub(s.precpu_stats.system_cpu_usage.unwrap_or(0))
                    as f64;
                let online = s.cpu_stats.online_cpus.unwrap_or_else(|| {
                    s.cpu_stats
                        .cpu_usage
                        .percpu_usage
                        .as_ref()
                        .map(|v| v.len() as u64)
                        .unwrap_or(1)
                }) as f64;
                let cpu_percent = if sys_delta > 0.0 && cpu_delta > 0.0 {
                    (cpu_delta / sys_delta) * online * 100.0
                } else {
                    0.0
                };

                // Memory usage vs limit (bytes).
                let memory_used = s.memory_stats.usage.unwrap_or(0);
                let memory_limit = s.memory_stats.limit.unwrap_or(0);
                let (network_rx_bytes, network_tx_bytes) = s
                    .networks
                    .as_ref()
                    .map(|networks| {
                        networks.values().fold((0_u64, 0_u64), |(rx, tx), n| {
                            (rx.saturating_add(n.rx_bytes), tx.saturating_add(n.tx_bytes))
                        })
                    })
                    .or_else(|| {
                        s.network
                            .map(|network| (network.rx_bytes, network.tx_bytes))
                    })
                    .unwrap_or((0, 0));
                let (block_read_bytes, block_write_bytes) = s
                    .blkio_stats
                    .io_service_bytes_recursive
                    .as_ref()
                    .map(|entries| {
                        entries.iter().fold((0_u64, 0_u64), |(read, write), e| {
                            match e.op.as_str() {
                                "Read" => (read.saturating_add(e.value), write),
                                "Write" => (read, write.saturating_add(e.value)),
                                _ => (read, write),
                            }
                        })
                    })
                    .unwrap_or((0, 0));

                results.push(ContainerStats {
                    id: id[..12.min(id.len())].to_string(),
                    name,
                    cpu_percent: (cpu_percent * 10.0).round() / 10.0,
                    memory_used,
                    memory_limit,
                    network_rx_bytes,
                    network_tx_bytes,
                    block_read_bytes,
                    block_write_bytes,
                    pids: s.num_procs,
                });
            }
        }

        Ok(results)
    }

    async fn install_app(&self, app: &CatalogApp) -> Result<ContainerInfo, AppError> {
        // Pull image
        let mut stream = self.client.create_image(
            Some(bollard::image::CreateImageOptions {
                from_image: app.image.clone(),
                ..Default::default()
            }),
            None,
            None,
        );

        while let Some(result) = stream.next().await {
            match result {
                Ok(info) => {
                    if let Some(status) = info.status {
                        tracing::info!("Pull: {}", status);
                    }
                }
                Err(e) => return Err(AppError::Docker(e)),
            }
        }

        // Ensure the apps network exists (self-healing: devices provisioned via
        // bootstrap, or where the network was pruned, won't have it otherwise).
        self.ensure_network("airwaves-apps").await?;

        // Create container
        let container_name = format!("airwaves-{}", app.id);

        // Pre-flight: reject if any requested host port is already published by
        // another container, so the user gets a clear message instead of a raw
        // "port is already allocated" 500 (common when two ADS-B apps overlap).
        let wanted_ports: Vec<u16> = app.ports.iter().filter_map(|p| p.host_port).collect();
        if !wanted_ports.is_empty() {
            let existing = self.list_containers().await.unwrap_or_default();
            for ec in &existing {
                if ec.name == container_name {
                    continue; // our own (about to be replaced)
                }
                for p in &ec.ports {
                    if let Some(hp) = p.host_port {
                        if wanted_ports.contains(&hp) {
                            return Err(AppError::BadRequest(format!(
                                "Host port {hp} is already in use by {}. Stop or uninstall it first (apps that share ports, like ultrafeeder and readsb, can't run together).",
                                ec.name
                            )));
                        }
                    }
                }
            }
        }

        // Make install idempotent: remove any pre-existing container with the
        // same name (e.g. a stale/failed prior attempt) so retries succeed.
        let _ = self
            .client
            .remove_container(
                &container_name,
                Some(RemoveContainerOptions {
                    force: true,
                    ..Default::default()
                }),
            )
            .await;

        let mut labels = HashMap::new();
        labels.insert("managed-by".to_string(), "airwaves".to_string());
        labels.insert("airwaves-app-id".to_string(), app.id.clone());
        let runtime_env = runtime_env_for_app(app);

        // Publish catalog-declared ports on the host.
        let mut port_bindings: HashMap<String, Option<Vec<bollard::models::PortBinding>>> =
            HashMap::new();
        let mut exposed_ports: HashMap<String, HashMap<(), ()>> = HashMap::new();
        for p in &app.ports {
            let key = format!("{}/{}", p.container_port, p.protocol);
            exposed_ports.insert(key.clone(), HashMap::new());
            if let Some(host_port) = p.host_port {
                port_bindings.insert(
                    key,
                    Some(vec![bollard::models::PortBinding {
                        host_ip: Some("0.0.0.0".to_string()),
                        host_port: Some(host_port.to_string()),
                    }]),
                );
            }
        }

        // SDR apps need USB access. Most apps get only the selected USB node
        // when the UI recorded an exact Airwaves SDR id. Apps that need to
        // enumerate the bus, such as acarsdec's native RTL path, get a bus
        // bind plus the USB character-device cgroup rule.
        let (devices, binds, device_cgroup_rules) = if app.requires_sdr {
            let selected_paths = crate::sdr::usb_device_paths_for_env(&runtime_env);
            if crate::sdr::requires_full_usb_bus_access(&runtime_env) || selected_paths.is_empty() {
                (
                    None,
                    Some(vec!["/dev/bus/usb:/dev/bus/usb".to_string()]),
                    Some(vec!["c 189:* rwm".to_string()]),
                )
            } else {
                (
                    Some(
                        selected_paths
                            .into_iter()
                            .map(|path| bollard::models::DeviceMapping {
                                path_on_host: Some(path.clone()),
                                path_in_container: Some(path),
                                cgroup_permissions: Some("rwm".to_string()),
                            })
                            .collect(),
                    ),
                    None,
                    None,
                )
            }
        } else {
            (None, None, None)
        };

        let host_config = bollard::models::HostConfig {
            restart_policy: Some(bollard::models::RestartPolicy {
                name: Some(bollard::models::RestartPolicyNameEnum::UNLESS_STOPPED),
                ..Default::default()
            }),
            network_mode: Some("airwaves-apps".to_string()),
            port_bindings: if port_bindings.is_empty() {
                None
            } else {
                Some(port_bindings)
            },
            devices,
            binds,
            device_cgroup_rules,
            ..Default::default()
        };

        // Inject the app's environment (defaults merged with the install
        // wizard's overrides by the handler) as KEY=VALUE. This is how SDR
        // assignment, frequencies, gain, lat/lon, etc. actually reach the
        // container — without this the configuration is silently dropped.
        let env_vars: Vec<String> = {
            let mut e: Vec<String> = runtime_env
                .iter()
                .filter(|(k, _)| !crate::sdr::is_internal_sdr_env_key(k))
                .map(|(k, v)| format!("{k}={v}"))
                .collect();
            e.sort(); // stable ordering for reproducible container specs
            e
        };

        // Optional command (exec form) for apps that take options as CLI args
        // (e.g. rtl_433 `-d :<serial>`). Substitute {{ENV_KEY}} tokens from the
        // resolved env so the picked SDR flows into the command line too.
        let cmd: Option<Vec<String>> = if app.command.is_empty() {
            None
        } else {
            Some(
                app.command
                    .iter()
                    .map(|arg| {
                        let mut out = arg.clone();
                        for (k, v) in &runtime_env {
                            out = out.replace(&format!("{{{{{k}}}}}"), v);
                        }
                        out
                    })
                    .collect(),
            )
        };

        let config = Config {
            image: Some(app.image.clone()),
            labels: Some(labels),
            env: if env_vars.is_empty() {
                None
            } else {
                Some(env_vars)
            },
            cmd,
            exposed_ports: if exposed_ports.is_empty() {
                None
            } else {
                Some(exposed_ports)
            },
            host_config: Some(host_config),
            ..Default::default()
        };

        let opts = CreateContainerOptions {
            name: &container_name,
            platform: None,
        };

        self.client.create_container(Some(opts), config).await?;

        // If start fails (e.g. networking error), remove the created container
        // so a failed install doesn't leave a stopped container that the UI
        // would show as "installed". Keeps install atomic.
        if let Err(e) = self
            .client
            .start_container(&container_name, None::<StartContainerOptions<String>>)
            .await
        {
            let _ = self
                .client
                .remove_container(
                    &container_name,
                    Some(RemoveContainerOptions {
                        force: true,
                        ..Default::default()
                    }),
                )
                .await;
            return Err(AppError::Docker(e));
        }

        // Return info about the created container
        Ok(ContainerInfo {
            id: container_name.clone(),
            name: container_name,
            image: app.image.clone(),
            status: "Up".to_string(),
            state: "running".to_string(),
            created: chrono::Utc::now().timestamp(),
            ports: app.ports.clone(),
        })
    }

    async fn uninstall_app(&self, id: &str) -> Result<(), AppError> {
        // Stop first
        let _ = self
            .client
            .stop_container(id, Some(StopContainerOptions { t: 5 }))
            .await;

        // Remove
        self.client
            .remove_container(
                id,
                Some(RemoveContainerOptions {
                    force: true,
                    ..Default::default()
                }),
            )
            .await?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn acarsdec_with_gain(gain: &str) -> CatalogApp {
        let mut app = CatalogApp {
            id: "acarsdec".to_string(),
            ..Default::default()
        };
        app.env.insert("GAIN".to_string(), gain.to_string());
        app
    }

    #[test]
    fn acarsdec_runtime_env_converts_auto_gain_aliases_to_agc_value() {
        for alias in ["auto", "autogain", "AGC"] {
            let env = runtime_env_for_app(&acarsdec_with_gain(alias));
            assert_eq!(env.get("GAIN").map(String::as_str), Some("-10"));
        }
    }

    #[test]
    fn acarsdec_runtime_env_preserves_numeric_gain() {
        let env = runtime_env_for_app(&acarsdec_with_gain("48"));
        assert_eq!(env.get("GAIN").map(String::as_str), Some("48"));
    }

    #[test]
    fn acarsdec_runtime_env_drops_blank_gain() {
        let env = runtime_env_for_app(&acarsdec_with_gain("  "));
        assert!(!env.contains_key("GAIN"));
    }
}
