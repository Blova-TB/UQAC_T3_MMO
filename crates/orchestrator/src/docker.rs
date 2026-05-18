use bollard::models::{ContainerCreateBody, HostConfig, PortBinding};
use bollard::query_parameters::{CreateContainerOptionsBuilder, StartContainerOptions};
use bollard::Docker;
use std::collections::HashMap;
use uuid::Uuid;

pub struct DockerOrchestrator {
    docker: Docker,
    orchestrator_ip: String,
}

impl DockerOrchestrator {
    pub async fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let docker = Docker::connect_with_socket_defaults()?;
        let my_id = std::fs::read_to_string("/etc/hostname")
            .unwrap_or_else(|_| "orchestrator".to_string())
            .trim()
            .to_string();

        println!("🔍 [Orchestrateur] Mon identifiant Docker est : {}", my_id);

        let orchestrator_ip = match docker.inspect_container(&my_id, None).await {
            Ok(inspect_result) => {
                inspect_result
                    .network_settings
                    .and_then(|settings| settings.networks)
                    .and_then(|networks| {
                        networks.get("game-network")
                            .and_then(|net| net.ip_address.clone())
                            .filter(|ip| !ip.is_empty())
                    })
                    .unwrap_or_else(|| {
                        eprintln!("⚠️ Impossible de trouver l'IP sur 'game-network'. Fallback local.");
                        "127.0.0.1".to_string()
                    })
            }
            Err(bollard::errors::Error::DockerResponseServerError { status_code: 404, .. }) => {
                println!("⚠️ Conteneur '{}' introuvable. Mode Dev (local) détecté.", my_id);
                std::env::var("ORCHESTRATOR_IP").unwrap_or_else(|_| "127.0.0.1".to_string())
            }
            Err(e) => return Err(e.into()),
        };

        println!("✅ [Orchestrateur] Mon IP brute (sur game-network) est : {}", orchestrator_ip);

        Ok(Self { docker, orchestrator_ip })
    }

    pub async fn spawn_game_server(
        &self,
        container_name: &str,
        image_name: &str,
        external_port: &str,
    ) -> Result<(String, String), Box<dyn std::error::Error>> {

        let server_id = Uuid::new_v4().to_string();

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
            env: Some(vec![
                format!("ORCHESTRATOR_ADDR={}:4000", self.orchestrator_ip),
                format!("SERVER_ID={}", server_id),
            ]),
            host_config: Some(HostConfig {
                port_bindings: Some(port_bindings),
                network_mode: Some("game-network".to_string()),
                auto_remove: Some(false),
                extra_hosts: Some(vec!["host.docker.internal:host-gateway".to_string()]),
                ..Default::default()
            }),
            ..Default::default()
        };

        let options = CreateContainerOptionsBuilder::default()
            .name(container_name)
            .build();

        let response = self
            .docker
            .create_container(Some(options), config)
            .await?;

        self.docker
            .start_container(&response.id, None::<StartContainerOptions>)
            .await?;

        Ok((response.id, server_id))
    }
}