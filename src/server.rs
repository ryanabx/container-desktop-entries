use std::{error::Error, fmt::Display, io, process::Command};

use crate::ContainerType;

#[derive(Debug)]
pub enum ServerError {}

#[derive(Debug)]
pub enum ClientSetupError {
    Container(ContainerError),
}

impl From<ContainerError> for ClientSetupError {
    fn from(value: ContainerError) -> Self {
        ClientSetupError::Container(value)
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
            "RUST_LOG=debug container-desktop-entries --name {} --protocol {}",
            container_name,
            String::from(container_type)
        ),
    )?;
    Ok(())
}

#[derive(Debug)]
pub enum ContainerError {
    IO(io::Error),
    CommandNotFound,
}

impl Display for ContainerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ContainerError::IO(e) => {
                write!(f, "IO: {}", e)
            }
            ContainerError::CommandNotFound => {
                write!(f, "Command for container not found")
            }
        }
    }
}

impl Error for ContainerError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        None
    }

    fn description(&self) -> &str {
        "description() is deprecated; use Display"
    }

    fn cause(&self) -> Option<&dyn Error> {
        self.source()
    }
}

/// start the client
fn start_client(
    container_name: &str,
    container_type: ContainerType,
) -> Result<(String, String), ContainerError> {
    shell_command(&container_type.format_start(container_name))
}

/// run a command on the container of choice
fn run_in_client(
    container_name: &str,
    container_type: ContainerType,
    command: &str,
) -> Result<(String, String), ContainerError> {
    shell_command(&container_type.format_exec(container_name, command))
}

fn shell_command(command: &str) -> Result<(String, String), ContainerError> {
    log::debug!("Full command: sh -c '{}'", command);
    let out = Command::new("sh").arg("-c").arg(command).output();
    match out {
        Ok(ref o) => {
            let std_out = String::from_utf8(o.stdout.clone()).unwrap();
            let std_err = String::from_utf8(o.stderr.clone()).unwrap();
            log::debug!("std_out: '{:?}'", std_out);
            log::debug!("std_err: '{:?}'", std_err);
            if std_err.contains("command not found") || std_out.contains("command not found") {
                Err(ContainerError::CommandNotFound)
            } else {
                Ok((std_out, std_err))
            }
        }
        Err(ref e) => {
            log::error!("error: {:?}", e);
            Err(ContainerError::IO(out.unwrap_err()))
        }
    }
}
