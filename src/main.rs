use clap::Parser;
use container_type::ContainerType;
use ron::de::SpannedError;
use serde::{Deserialize, Serialize};
use server::ClientSetupError;
use std::error::Error;
use std::fmt::Display;
use std::{env, fs, io};

use std::{fs::read_to_string, path::Path};

mod container_type;
mod desktop_entry;
mod server;

/// program to get desktop entries from containers
#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    #[arg(short, long, requires = "server", value_name = "CONFIG_PATH")]
    /// [AS SERVER] Path to an alternate config for the program.
    /// Default is $HOME/.config/container-desktop-entries/containers.ron
    config: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct ContainerList {
    pub containers: Vec<(String, ContainerType)>,
}

#[derive(Debug)]
enum CDEError {
    IO(io::Error),
    NoEnv(std::env::VarError),
    Ron(SpannedError),
    ClientSetup(ClientSetupError),
}

impl Error for CDEError {
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

impl Display for CDEError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::IO(e) => e.fmt(f),
            Self::NoEnv(e) => e.fmt(f),
            Self::Ron(e) => e.fmt(f),
            Self::ClientSetup(e) => e.fmt(f),
        }
    }
}

impl From<io::Error> for CDEError {
    fn from(value: io::Error) -> Self {
        Self::IO(value)
    }
}

impl From<std::env::VarError> for CDEError {
    fn from(value: std::env::VarError) -> Self {
        Self::NoEnv(value)
    }
}

impl From<SpannedError> for CDEError {
    fn from(value: SpannedError) -> Self {
        Self::Ron(value)
    }
}

impl From<ClientSetupError> for CDEError {
    fn from(value: ClientSetupError) -> Self {
        Self::ClientSetup(value)
    }
}

#[async_std::main]
async fn main() -> Result<(), CDEError> {
    env_logger::init();

    if !cfg!(target_os = "linux") {
        log::error!("Target OS is not Linux");
        panic!("target OS must be linux");
    }

    let args = Args::parse();

    let default_path_str = format!(
        "{}/.config/container-desktop-entries/containers.ron",
        env::var("HOME")?
    );
    let conf_path = match args.config.as_ref() {
        None => Path::new(&default_path_str),
        Some(path) => Path::new(path),
    };
    match conf_path.try_exists() {
        Ok(false) | Err(_) => {
            log::error!(
                "cannot find config at '{:?}'. creating directory and file...",
                conf_path
            );
            let _ = fs::create_dir(conf_path.parent().unwrap());
            let _ = fs::write(conf_path,"// Example config:\n/*\n(\n  containers:\n  [\n    (\"fedora-toolbox-40\", Toolbox),\n    (\"docker-container\", Docker),\n  ],\n)\n*/",
            );
            log::info!(
                "write a configuration file. an example has been written to the config directory"
            )
        }
        _ => {}
    }
    let config_data: ContainerList = ron::from_str(&read_to_string(conf_path)?)?;

    server::server(config_data, "container-desktop-entries").await?;

    Ok(())
}
