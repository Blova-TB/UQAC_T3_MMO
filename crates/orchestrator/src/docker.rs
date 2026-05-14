use bollard::Docker;
use bollard::query_parameters::{CreateContainerOptionsBuilder, StartContainerOptions};
use bollard::models::{ContainerCreateBody, HostConfig};
use std::default::Default;

pub struct DockerOrchestrator {
    docker: Docker,
}

impl DockerOrchestrator {
    pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let docker = Docker::connect_with_local_defaults()?;
        Ok(Self { docker })
    }

    pub async fn test_spawn(&self) -> Result<String, Box<dyn std::error::Error>> {
        let container_name = "test-rust-container";

        let config = ContainerCreateBody {
            image: Some("alpine".to_string()),
            cmd: Some(vec!["sleep".to_string(), "60".to_string()]),
            host_config: Some(HostConfig {
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