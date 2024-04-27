use std::path::Path;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
pub enum ContainerType {
    Podman,
    Docker,
    Toolbox,
    Unknown,
}

impl ContainerType {
    pub fn not_supported(self) -> bool {
        matches!(
            self,
            ContainerType::Docker | ContainerType::Podman | ContainerType::Unknown
        )
    }

    pub fn format_copy(self, container_name: &str, from: &Path, to: &Path) -> String {
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

    pub fn format_exec(self, container_name: &str, command: &str) -> String {
        match self {
            ContainerType::Toolbox => {
                format!("toolbox run -c {} {}", container_name, command)
            }
            _ => "".to_string(), // TODO: Support more container types
        }
    }

    pub fn format_exec_regex_pattern(self) -> String {
        match self {
            ContainerType::Toolbox | ContainerType::Podman | ContainerType::Docker => {
                r"(Exec=\s?)(.*)".to_string()
            }
            _ => "".to_string(),
        }
    }

    pub fn format_desktop_exec(self, container_name: &str) -> String {
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

    pub fn format_name_regex_pattern(self) -> String {
        match self {
            ContainerType::Toolbox | ContainerType::Podman | ContainerType::Docker => {
                r"(Name=\s?)(.*)".to_string()
            }
            _ => "".to_string(),
        }
    }

    pub fn format_desktop_name(self, container_name: &str) -> String {
        match self {
            ContainerType::Toolbox => {
                format!(r"Name=${{2}} ({})", container_name)
            }
            _ => "".to_string(), // TODO: Support more container types
        }
    }

    pub fn format_start(self, container_name: &str) -> String {
        match self {
            ContainerType::Toolbox => {
                format!("toolbox run -c {} echo 'Started'", container_name)
            }
            _ => "".to_string(), // TODO: Support more container types
        }
    }
}
