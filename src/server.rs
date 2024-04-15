use std::{
    io,
    process::{self, Command},
};

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

pub async fn server(containers: Vec<(String, ContainerType)>) -> Result<(), ServerError> {
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
    loop {
        // Busy wait until logging off, keeping the desktop entries alive
        std::future::pending::<()>().await;
    }
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
            "container-desktop-entries -n {} -t {} -p {}",
            container_name,
            String::from(container_type),
            process::id()
        ),
    )?;
    Ok(())
}

/// start the client
fn start_client(container_name: &str, container_type: ContainerType) -> Result<(), io::Error> {
    shell_command(&container_type.format_start(container_name), true)
}

/// run a command on the container of choice
fn run_in_client(
    container_name: &str,
    container_type: ContainerType,
    command: &str,
) -> Result<(), io::Error> {
    shell_command(&container_type.format_exec(container_name, command), true)
}

fn shell_command(command: &str, wait_for_output: bool) -> Result<(), io::Error> {
    log::debug!("Full command: sh -c '{}'", command);
    if wait_for_output {
        let out = Command::new("sh")
            .arg("-c")
            .arg(command)
            .output()
            .expect(&format!("Command {} failed", command));
        log::debug!(
            "Output completed! stdout: '{}', stderr: '{}'",
            String::from_utf8(out.stdout).unwrap(),
            String::from_utf8(out.stderr).unwrap()
        );
    } else {
        let child_handle = Command::new("sh")
            .arg("-c")
            .arg(command)
            .spawn()
            .expect(&format!("Command {} failed", command));
        log::debug!("Started child process with pid {}", child_handle.id());
    }

    Ok(())
}
