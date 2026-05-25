use bollard::models::{ContainerCreateBody, HostConfig, PortBinding};
use bollard::query_parameters::{CreateContainerOptionsBuilder, StartContainerOptions, RemoveContainerOptions};
use bollard::Docker;
use std::collections::HashMap;
use uuid::Uuid;
use anyhow::{Context, Result};

pub struct DockerOrchestrator {
    docker: Docker,
    orchestrator_ip: String,
    broker_ip: String,
}

impl DockerOrchestrator {
    pub async fn new() -> Result<Self> {
        let docker = Docker::connect_with_socket_defaults()
            .context("Impossible de se connecter au démon Docker. Est-il lancé ?")?;

        let my_id = std::fs::read_to_string("/etc/hostname")
            .unwrap_or_else(|_| "orchestrator".to_string())
            .trim()
            .to_string();

        // 1. Récupération de l'IP de l'Orchestrateur
        let orchestrator_ip = match docker.inspect_container(&my_id, None).await {
            Ok(inspect_result) => Self::extract_ip_from_inspection(&inspect_result, "game-network")
                .unwrap_or_else(|| "127.0.0.1".to_string()),
            Err(bollard::errors::Error::DockerResponseServerError { status_code: 404, .. }) => {
                std::env::var("ORCHESTRATOR_IP").unwrap_or_else(|_| "127.0.0.1".to_string())
            }
            Err(e) => return Err(e.into()),
        };

        // 2. Récupération unique et définitive de l'IP du Broker
        let broker_ip = match docker.inspect_container("broker", None).await {
            Ok(inspect_result) => Self::extract_ip_from_inspection(&inspect_result, "game-network")
                .context("Le Broker est introuvable sur le game-network.")?,
            Err(_) => {
                println!("⚠️ Conteneur 'broker' introuvable. Fallback mode Dev (local).");
                "127.0.0.1".to_string()
            }
        };

        println!("✅ [Orchestrateur] Mon IP : {} | IP Broker mise en cache : {}", orchestrator_ip, broker_ip);

        Ok(Self { docker, orchestrator_ip, broker_ip })
    }

    pub async fn spawn_game_server(
        &self,
        container_name: &str,
        image_name: &str,
        external_port: &str,
        max_players: usize,
    ) -> Result<(String, String, String)> {

        let server_id = Uuid::new_v4().to_string();
        let internal_port_str = format!("{}/udp", external_port);

        let mut port_bindings = HashMap::new();
        port_bindings.insert(
            internal_port_str.clone(),
            Some(vec![PortBinding {
                host_ip: Some("0.0.0.0".to_string()),
                host_port: Some(external_port.to_string()),
            }]),
        );

        let broker_addr = format!("{}:5000", self.broker_ip);

        let config = ContainerCreateBody {
            image: Some(image_name.to_string()),
            env: Some(vec![
                format!("ORCHESTRATOR_ADDR={}:4000", self.orchestrator_ip),
                format!("SERVER_ID={}", server_id),
                format!("GAME_PORT={}", external_port),
                format!("SERVER_MAX_PLAYERS={}", max_players),
                format!("BROKER_ADDR={}", broker_addr),
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

        let response = self.docker.create_container(Some(options), config).await
            .context(format!("Échec de la création du conteneur {}", container_name))?;

        self.docker.start_container(&response.id, None::<StartContainerOptions>).await
            .context(format!("Échec du démarrage du conteneur {}", container_name))?;

        let inspect_result = self.docker.inspect_container(&response.id, None).await
            .context(format!("Échec de l'inspection du conteneur {}", container_name))?;

        let internal_ip = Self::extract_ip_from_inspection(&inspect_result, "game-network")
            .context(format!("Impossible de trouver l'IP du conteneur {} sur game-network", container_name))?;

        Ok((response.id, server_id, internal_ip))
    }
    pub async fn remove_game_server(&self, container_name: &str) -> Result<()> {
        let options = Some(RemoveContainerOptions {
            force: true,
            v: true,
            ..Default::default()
        });

        match self.docker.remove_container(container_name, options).await {
            Ok(_) => {
                println!("🐳 [Docker] Conteneur '{}' détruit avec succès.", container_name);
                Ok(())
            }
            Err(e) => {
                eprintln!("⚠️ [Docker] Impossible de détruire le conteneur '{}' : {}", container_name, e);
                Err(e.into())
            }
        }
    }

    fn extract_ip_from_inspection(inspect_result: &bollard::models::ContainerInspectResponse, network_name: &str) -> Option<String> {
        inspect_result
            .network_settings
            .as_ref()?
            .networks
            .as_ref()?
            .get(network_name)?
            .ip_address
            .clone()
            .filter(|ip| !ip.is_empty())
    }
}