use clap::Parser;
use server::ServerError;
use std::{env, fs, io};

use std::{fs::read_to_string, path::Path};

mod desktop_entry;
mod server;

/// program to get desktop entries from containers
#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    #[arg(short, long, requires = "server", value_name = "CONFIG_PATH")]
    /// [AS SERVER] Path to an alternate config for the program.
    /// Default is $HOME/.config/container-desktop-entries/containers.conf
    config: Option<String>,
}

#[derive(Debug, Clone, Copy)]
enum ContainerType {
    Podman,
    Docker,
    Toolbox,
    Unknown,
}

impl From<String> for ContainerType {
    fn from(value: String) -> Self {
        match value.as_str() {
            "toolbox" => ContainerType::Toolbox,
            "docker" => ContainerType::Docker,
            "podman" => ContainerType::Podman,
            _ => ContainerType::Unknown,
        }
    }
}
impl From<ContainerType> for String {
    fn from(value: ContainerType) -> Self {
        match value {
            ContainerType::Toolbox => "toolbox".to_string(),
            ContainerType::Docker => "docker".to_string(),
            ContainerType::Podman => "podman".to_string(),
            ContainerType::Unknown => "".to_string(),
        }
    }
}

impl ContainerType {
    fn not_supported(self) -> bool {
        matches!(
            self,
            ContainerType::Docker | ContainerType::Podman | ContainerType::Unknown
        )
    }

    fn format_copy(self, container_name: &str, from: &Path, to: &Path) -> String {
        match self {
            ContainerType::Toolbox => {
                format!(
                    "podman container cp {}:{}/. {}/",
                    container_name,
                    from.to_str().unwrap(),
                    to.to_str().unwrap()
                )
            }
            _ => "".to_string(), // TODO: Support more container types
        }
    }

    fn format_exec(self, container_name: &str, command: &str) -> String {
        match self {
            ContainerType::Toolbox => {
                format!("toolbox run -c {} {}", container_name, command)
            }
            _ => "".to_string(), // TODO: Support more container types
        }
    }

    fn format_exec_regex_pattern(self) -> String {
        match self {
            ContainerType::Toolbox | ContainerType::Podman | ContainerType::Docker => {
                r"(Exec=\s?)(.*)".to_string()
            }
            _ => "".to_string(),
        }
    }

    fn format_desktop_exec(self, container_name: &str) -> String {
        match self {
            ContainerType::Toolbox => {
                format!(r"Exec=toolbox run -c {} ${{2}}", container_name)
            }
            ContainerType::Podman => {
                // TODO: Currently not always functional
                format!(
                    r"Exec=sh -c 'podman container start {} && podman container exec {} ${{2}}'",
                    container_name, container_name
                )
            }
            _ => "".to_string(), // TODO: Support more container types
        }
    }

    fn format_name_regex_pattern(self) -> String {
        match self {
            ContainerType::Toolbox | ContainerType::Podman | ContainerType::Docker => {
                r"(Name=\s?)(.*)".to_string()
            }
            _ => "".to_string(),
        }
    }

    fn format_desktop_name(self, container_name: &str) -> String {
        match self {
            ContainerType::Toolbox => {
                format!(r"Name=${{2}} ({})", container_name)
            }
            _ => "".to_string(), // TODO: Support more container types
        }
    }

    fn format_start(self, container_name: &str) -> String {
        match self {
            ContainerType::Toolbox => {
                format!("toolbox run -c {} echo 'Started'", container_name)
            }
            _ => "".to_string(), // TODO: Support more container types
        }
    }
}

#[derive(Debug)]
enum Error {
    Server(ServerError),
    IO(io::Error),
    NoEnv(std::env::VarError),
}

impl From<ServerError> for Error {
    fn from(value: ServerError) -> Self {
        Error::Server(value)
    }
}

impl From<io::Error> for Error {
    fn from(value: io::Error) -> Self {
        Error::IO(value)
    }
}

impl From<std::env::VarError> for Error {
    fn from(value: std::env::VarError) -> Self {
        Error::NoEnv(value)
    }
}

#[async_std::main]
async fn main() -> Result<(), Error> {
    env_logger::init();

    if !cfg!(target_os = "linux") {
        log::error!("Target OS is not Linux");
        panic!("target OS must be linux");
    }

    let args = Args::parse();

    let default_path_str = format!(
        "{}/.config/container-desktop-entries/containers.conf",
        env::var("HOME")?
    );
    let conf_path = match args.config.as_ref() {
        None => Path::new(&default_path_str),
        Some(path) => Path::new(path),
    };
    match conf_path.try_exists() {
        Ok(false) | Err(_) => {
            log::error!("Cannot find config at '{:?}'", conf_path);
            let _ = fs::write(conf_path, "");
        }
        _ => {}
    }
    log::info!("Running as server! Getting config at '{:?}'", conf_path);
    let config_data = read_to_string(conf_path)?
        .lines()
        .map(|s| {
            let ss = s
                .split_once(" ")
                .expect("Config invalid. make sure all lines are <<NAME>> <<TYPE>>");
            (ss.0.to_string(), ContainerType::from(ss.1.to_string()))
        })
        .collect::<Vec<_>>();

    server::server(config_data).await?;

    Ok(())
}
