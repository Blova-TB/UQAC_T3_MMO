use bollard::Docker;
use bollard::models::{ContainerCreateBody, HostConfig, PortBinding};
use bollard::query_parameters::{CreateContainerOptionsBuilder, StartContainerOptions};
use std::collections::HashMap;

pub struct DockerOrchestrator {
    docker: Docker,
}

impl DockerOrchestrator {
    pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let docker = Docker::connect_with_local_defaults()?;
        Ok(Self { docker })
    }

    pub async fn spawn_game_server(
        &self,
        container_name: &str,
        image_name: &str,
        orchestrator_url: &str,
        external_port: &str,
    ) -> Result<String, Box<dyn std::error::Error>> {

        let mut port_bindings = HashMap::new();
        port_bindings.insert(
            "4000/udp".to_string(),
            Some(vec![PortBinding {
                host_ip: Some("0.0.0.0".to_string()),
                host_port: Some(external_port.to_string()),
            }]),
        );

        let config = ContainerCreateBody {
            image: Some(image_name.to_string()),
            // 👇 On passe l'adresse de l'orchestrateur au serveur
            env: Some(vec![format!("ORCHESTRATOR_URL={}", orchestrator_url)]),
            host_config: Some(HostConfig {
                port_bindings: Some(port_bindings),
                network_mode: Some("game-network".to_string()),
                auto_remove: Some(true),
                ..Default::default()
            }),
            ..Default::default()
        };

        let options = CreateContainerOptionsBuilder::default()
            .name(container_name)
            .build();

        let response = self.docker
            .create_container(Some(options), config)
            .await?;

        self.docker
            .start_container(&response.id, None::<StartContainerOptions>)
            .await?;

        Ok(response.id)
    }
}