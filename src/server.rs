use std::{io, process::Command};

use crate::ContainerType;

#[derive(Debug)]
pub enum ServerError {}

#[derive(Debug)]
pub enum ClientSetupError {
    IO(io::Error),
}

impl From<io::Error> for ClientSetupError {
    fn from(value: io::Error) -> Self {
        ClientSetupError::IO(value)
    }
}

pub fn server(containers: Vec<(String, ContainerType)>) -> Result<(), ServerError> {
    for (container_name, container_type) in containers {
        if container_type.not_supported() {
            log::error!(
                "Container type {:?} is currently not supported!",
                container_type
            );
            continue;
        }
        if let Err(kind) = set_up_client(&container_name, container_type) {
            log::error!("Error setting up client {}: {:?}", container_name, kind);
        }
    }
    Ok(())
}

fn set_up_client(
    container_name: &str,
    container_type: ContainerType,
) -> Result<(), ClientSetupError> {
    // Start client if client is not running
    start_client(container_name, container_type)?;
    // Run client's container-desktop-entries
    run_in_client(
        container_name,
        container_type,
        &format!(
            "container-desktop-entries --name {} --protocol {}",
            container_name,
            String::from(container_type)
        ),
    )?;
    Ok(())
}

/// start the client
fn start_client(container_name: &str, container_type: ContainerType) -> Result<(), io::Error> {
    shell_command(&container_type.format_start(container_name))
}

/// run a command on the container of choice
fn run_in_client(
    container_name: &str,
    container_type: ContainerType,
    command: &str,
) -> Result<(), io::Error> {
    shell_command(&container_type.format_exec(container_name, command))
}

fn shell_command(command: &str) -> Result<(), io::Error> {
    log::debug!("Full command: sh -c '{}'", command);
    let _ = Command::new("sh").arg("-c").arg(command).spawn();
    Ok(())
}
