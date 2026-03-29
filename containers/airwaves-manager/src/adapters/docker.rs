use bollard::container::{
    Config, CreateContainerOptions, ListContainersOptions, LogOutput, LogsOptions,
    StartContainerOptions, StopContainerOptions, RemoveContainerOptions,
};
use bollard::system::EventsOptions;
use bollard::Docker;
use futures::StreamExt;
use std::collections::HashMap;
use std::pin::Pin;

use crate::domain::*;
use crate::error::AppError;
use crate::ports::DockerPort;

pub struct DockerAdapter {
    client: Docker,
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
    /// Returns a stream of Docker daemon events (container start/stop/die/etc.)
    pub async fn watch_events(
        &self,
    ) -> Pin<Box<dyn futures::Stream<Item = Result<bollard::models::EventMessage, bollard::errors::Error>> + Send + '_>>
    {
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
                let name = c.names
                    .as_ref()
                    .and_then(|n| n.first())
                    .map(|n| n.trim_start_matches('/').to_string())
                    .unwrap_or_default();

                let ports = c.ports
                    .unwrap_or_default()
                    .into_iter()
                    .map(|p| PortBinding {
                        container_port: p.private_port,
                        host_port: p.public_port,
                        protocol: p.typ.map(|t| format!("{:?}", t)).unwrap_or_else(|| "tcp".to_string()),
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
            .restart_container(id, Some(bollard::container::RestartContainerOptions { t: 10 }))
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

        // Create container
        let container_name = format!("airwaves-{}", app.id);
        let mut labels = HashMap::new();
        labels.insert("managed-by".to_string(), "airwaves".to_string());
        labels.insert("airwaves-app-id".to_string(), app.id.clone());

        let host_config = bollard::models::HostConfig {
            restart_policy: Some(bollard::models::RestartPolicy {
                name: Some(bollard::models::RestartPolicyNameEnum::UNLESS_STOPPED),
                ..Default::default()
            }),
            network_mode: Some("airwaves-apps".to_string()),
            ..Default::default()
        };

        let config = Config {
            image: Some(app.image.clone()),
            labels: Some(labels),
            host_config: Some(host_config),
            ..Default::default()
        };

        let opts = CreateContainerOptions {
            name: &container_name,
            platform: None,
        };

        self.client.create_container(Some(opts), config).await?;
        self.client
            .start_container(&container_name, None::<StartContainerOptions<String>>)
            .await?;

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
        let _ = self.client
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
