use clap::Parser;
use freedesktop_desktop_entry::DesktopEntry;
use freedesktop_icon_lookup::{Cache, IconInfo, LookupParam};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::env;
use std::error::Error;

use std::fs::{self, create_dir_all, remove_dir_all, File};

#[cfg(feature = "server")]
use std::fmt::Display;
#[cfg(feature = "server")]
use std::fs::DirEntry;
#[cfg(feature = "server")]
use std::io;
#[cfg(feature = "server")]
use std::process::Command;
use std::{
    fs::{read_dir, read_to_string},
    path::Path,
};
use xdg::BaseDirectories;

#[cfg(feature = "server")]
use crate::desktop_entry::DesktopEntryProxy;
#[cfg(feature = "server")]
use zbus::Connection;
#[cfg(feature = "server")]
mod desktop_entry;

/// program to get desktop entries from containers
#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    /// name and protocol of the toolbox to run this command with
    /// for example, "fedora-toolbox-39 docker".
    /// leave blank to use the config at `$HOME/.config/container-desktop-entries/containers.conf`
    #[arg(short, long)]
    name_and_protocol: Option<String>,

    /// run in client mode
    #[cfg(feature = "server")]
    #[arg(short, long, action = clap::ArgAction::SetTrue)]
    client: bool,
}

#[derive(Debug, Clone)]
enum Mode {
    Client,
    #[cfg(feature = "server")]
    Server,
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

    fn format_copy(self, container_name: &str, from: &str, to: &str) -> String {
        match self {
            ContainerType::Toolbox | ContainerType::Podman => {
                format!("podman cp {}:{}/. {}", container_name, from, to)
            }
            _ => "".to_string(), // TODO: Support more container types
        }
    }

    fn format_exec(self, container_name: &str, command: &str) -> String {
        match self {
            ContainerType::Toolbox | ContainerType::Podman => {
                format!("podman container exec {} {}", container_name, command)
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

    fn format_start(self, container_name: &str, container_args: &str) -> String {
        match self {
            ContainerType::Toolbox | ContainerType::Podman => {
                format!("podman start {} {}", container_name, container_args)
            }
            _ => "".to_string(), // TODO: Support more container types
        }
    }
}

#[async_std::main]
async fn main() -> Result<(), Box<dyn Error>> {
    env_logger::init();

    let args = Args::parse();

    if !cfg!(target_os = "linux") {
        panic!("target OS must be linux");
    }

    #[cfg(feature = "server")]
    let mode = if args.client {
        Mode::Client
    } else {
        Mode::Server
    };
    #[cfg(not(feature = "server"))]
    let mode = Mode::Client;

    log::info!("args: {:?}, mode: {:?}", args, mode);

    let names_and_protocols = if args.name_and_protocol.is_some() {
        let s = args
            .name_and_protocol
            .as_ref()
            .unwrap()
            .split_once(" ")
            .unwrap();
        vec![(s.0.to_string(), ContainerType::from(s.1.to_string()))]
    } else {
        if !Path::new(&env::var("HOME").unwrap())
            .join(Path::new(
                ".config/container-desktop-entries/containers.conf",
            ))
            .exists()
        {
            if let Err(e) = fs::create_dir(
                Path::new(&env::var("HOME").unwrap())
                    .join(Path::new(".config/container-desktop-entries/")),
            ) {
                println!("{}", e);
            }
            File::create_new(Path::new(&env::var("HOME").unwrap()).join(Path::new(
                ".config/container-desktop-entries/containers.conf",
            )))
            .expect("could not create container config file");
        }
        read_to_string(Path::new(&env::var("HOME").unwrap()).join(Path::new(
            ".config/container-desktop-entries/containers.conf",
        )))
        .expect("could not find config directory")
        .lines()
        .map(|s| {
            let ss = s
                .split_once(" ")
                .expect("config invalid. make sure all lines are <<NAME>> <<TYPE>>");
            (ss.0.to_string(), ContainerType::from(ss.1.to_string()))
        })
        .collect::<Vec<_>>()
    };

    for (name, protocol) in names_and_protocols {
        if protocol.not_supported() {
            log::warn!("Protocol {:?} not supported currently. See https://github.com/ryanabx/container-desktop-entries to contribute.", protocol);
        }
        match mode {
            Mode::Client => {
                log::info!("Running as client: {} {:?}", name, protocol);
                container_client(&name, protocol);
            }
            #[cfg(feature = "server")]
            Mode::Server => {
                log::info!("Running as server: {} {:?}", name, protocol);
                if let Err(e) = container_server(&name, protocol).await {
                    return Err(e);
                }
            }
        }
    }

    Ok(())
}

#[cfg(feature = "server")]
#[derive(Debug)]
enum ContainerError {
    IO(io::Error),
    CommandNotFound,
}

#[cfg(feature = "server")]
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

#[cfg(feature = "server")]
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

/// run a command on the container of choice
#[cfg(feature = "server")]
fn run_on_container(
    container_name: &str,
    container_type: ContainerType,
    command: &str,
) -> Result<(String, String), ContainerError> {
    shell_command(&container_type.format_exec(container_name, command))
}

#[cfg(feature = "server")]
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

#[cfg(feature = "server")]
fn start_container(
    container_name: &str,
    container_type: ContainerType,
    container_args: &str,
) -> Result<(String, String), ContainerError> {
    shell_command(&container_type.format_start(container_name, container_args))
}

#[cfg(feature = "server")]
/// copy files on the container of choice
fn copy_from_container(
    container_name: &str,
    container_type: ContainerType,
    from: &str,
    to: &str,
) -> Result<(String, String), ContainerError> {
    shell_command(&container_type.format_copy(container_name, from, to))
}

#[derive(Serialize, Deserialize)]
struct ClientData {
    desktop_entries: Vec<String>,
    icons: Vec<String>,
}

#[cfg(feature = "server")]
async fn container_server(
    container_name: &str,
    container_type: ContainerType,
) -> Result<(), Box<dyn Error>> {
    let connection = Connection::session().await?;

    let proxy = DesktopEntryProxy::new(&connection).await?;

    let _ = start_container(container_name, container_type, "")?;

    let _ = run_on_container(
        &format!("-e RUST_LOG=trace {}", container_name),
        container_type,
        &format!(
            "container-desktop-entries-client --name-and-protocol '{} {}'",
            container_name,
            String::from(container_type)
        ),
    )?;

    log::info!("client successfully finished");

    let tmp_applications_from = Path::new("/tmp/container-desktop-entries-client/applications/");
    let tmp_applications_to = Path::new("/tmp/container-desktop-entries/applications/");
    let tmp_icons_from = Path::new("/tmp/container-desktop-entries-client/icons/");
    let tmp_icons_to = Path::new("/tmp/container-desktop-entries/icons/");

    let _ = create_dir_all(tmp_applications_to);
    let _ = create_dir_all(tmp_icons_to);

    let _ = copy_from_container(
        container_name,
        container_type,
        tmp_applications_from.to_str().unwrap(),
        tmp_applications_to.to_str().unwrap(),
    )?;
    let _ = copy_from_container(
        container_name,
        container_type,
        tmp_icons_from.to_str().unwrap(),
        tmp_icons_to.to_str().unwrap(),
    )?;

    if let Ok(read_dir) = read_dir(tmp_applications_to) {
        let entries = read_dir
            .into_iter()
            .filter_map(|f| match f {
                Ok(s) => Some(s.path().to_str().unwrap().to_string()),
                Err(_) => None,
            })
            .collect::<Vec<_>>();

        match proxy
            .register_entries(&entries.iter().map(String::as_ref).collect::<Vec<_>>())
            .await
        {
            Ok(resulting_entries) => {
                log::info!("daemon registered entries: {:?}", resulting_entries);
            }
            Err(e) => {
                log::error!("Error (entries): {:?}", e);
            }
        }
    } else {
        log::error!("Could not read applications directory")
    }

    let mut icon_full_paths = Vec::new();
    let mut icon_partial_paths = Vec::new();

    let _ = visit_dirs(tmp_icons_to, &mut |entry| {
        log::debug!("Found icon: {:?}", entry);
        if let Some((_, icon_path)) = &entry.path().to_str().unwrap().split_once("/icons/") {
            icon_full_paths.push(entry.path().to_str().unwrap().to_string());
            icon_partial_paths.push(
                Path::new(icon_path)
                    .parent()
                    .unwrap()
                    .to_str()
                    .unwrap()
                    .to_string(),
            );
        } else {
            log::error!("Icon path didn't have /icons/ in it! {:?}", entry);
        }
    });

    match proxy
        .register_icons(
            &icon_full_paths
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            &icon_partial_paths
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>(),
        )
        .await
    {
        Ok(resulting_icons) => {
            log::info!("daemon registered icons: {:?}", resulting_icons);
        }
        Err(e) => {
            log::error!("Error (icons): {:?}", e);
        }
    }

    let _ = remove_dir_all(tmp_applications_to);
    let _ = remove_dir_all(tmp_icons_to);

    let _ = run_on_container(
        container_name,
        container_type,
        &format!("rm -r {}", tmp_applications_from.to_str().unwrap()),
    )?;

    let _ = run_on_container(
        container_name,
        container_type,
        &format!("rm -r {}", tmp_icons_from.to_str().unwrap()),
    )?;
    Ok(())
}

// one possible implementation of walking a directory only visiting files
#[cfg(feature = "server")]
fn visit_dirs<F>(dir: &Path, cb: &mut F) -> io::Result<()>
where
    F: FnMut(DirEntry),
{
    if dir.is_dir() {
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                visit_dirs(&path, cb)?;
            } else {
                cb(entry);
            }
        }
    }
    Ok(())
}

fn container_client(container_name: &str, container_type: ContainerType) {
    let xdg_basedirs = BaseDirectories::new().expect("could not get xdg basedirectories");
    let data_dirs = xdg_basedirs.get_data_dirs();
    log::trace!("xdg data dirs: {:?}", data_dirs);
    let tmp_applications_dir = Path::new("/tmp/container-desktop-entries-client/applications/");
    let tmp_icons_dir = Path::new("/tmp/container-desktop-entries-client/icons/");
    let _ = remove_dir_all(tmp_applications_dir);
    let _ = remove_dir_all(tmp_icons_dir);
    let _ = create_dir_all(tmp_applications_dir);
    let _ = create_dir_all(tmp_icons_dir);
    let mut icon_names: Vec<String> = Vec::new();
    let exec_regex = Regex::new(container_type.format_exec_regex_pattern().as_str()).unwrap();
    let name_regex = Regex::new(container_type.format_name_regex_pattern().as_str()).unwrap();
    let mut entries_count = 0;
    for dirs in data_dirs {
        let app_dir = dirs.as_path().join(Path::new("applications"));
        match read_dir(&app_dir) {
            Ok(contents) => {
                log::trace!("Got contents of dir: {:?}", contents);
                for entry in contents {
                    let entry = entry.expect("can't find entry");
                    log::trace!("Entry: {:?}", entry);
                    let ty = entry.file_type().expect("can't get file type");
                    if ty.is_file() && entry.path().extension().is_some_and(|ext| ext == "desktop")
                    {
                        if let Ok(txt) = read_to_string(&entry.path()) {
                            let new_text = exec_regex.replace_all(
                                &txt,
                                container_type.format_desktop_exec(container_name),
                            );
                            let new_text = exec_regex.replace_all(
                                &new_text,
                                container_type.format_desktop_name(container_name),
                            );
                            match std::fs::write(
                                tmp_applications_dir.join(entry.file_name()),
                                new_text.as_bytes(),
                            ) {
                                Ok(_) => {
                                    if let Ok(desktop_entry) = DesktopEntry::decode(
                                        &tmp_applications_dir.join(entry.file_name()),
                                        &new_text,
                                    ) {
                                        entries_count += 1;
                                        if let Some(icon_name) = desktop_entry.icon() {
                                            icon_names.push(icon_name.to_string());
                                        }
                                    }
                                }
                                Err(e) => {
                                    log::error!(
                                        "Could not write to file '{:?}' error: {:?}",
                                        tmp_applications_dir.join(entry.file_name()),
                                        e
                                    );
                                }
                            }
                        }
                    } else {
                        log::warn!("Skipping {:?} as it is either not a file or does not end with .desktop. Extension: {:?} is_file: {:?}", entry, entry.path().extension(), ty.is_file());
                    }
                }
            }
            Err(_) => {
                log::warn!("Could not read data directory {:?}", app_dir);
            }
        }
    }
    let mut icon_paths = Vec::new();
    let cache = Cache::new().unwrap();
    for icon in &icon_names {
        let f = cache.lookup(&icon, None);
        if let Some(fpath) = f {
            let full_path = fpath.as_path().to_str().unwrap().to_string();
            let file_name = fpath
                .as_path()
                .file_name()
                .unwrap()
                .to_str()
                .unwrap()
                .to_string();
            let icon_subpath = if let Some((_, icon_dir)) = full_path.split_once("/icons/") {
                log::info!(
                    "Icon {:?} successful! logging subpath {:?}",
                    &fpath,
                    icon_dir
                );
                Path::new(icon_dir).to_path_buf()
            } else if let Some((_, _)) = full_path.split_once("/pixmaps/") {
                log::info!(
                    "Icon {:?} successful! logging subpath hicolor/48x48/apps/ as it was a pixmap",
                    &fpath
                );
                Path::new(&format!("hicolor/48x48/apps/{}", file_name)).to_path_buf()
            } else {
                log::warn!("Neither pixmap or icon. what is this?");
                Path::new(&format!("hicolor/48x48/apps/{}", file_name)).to_path_buf()
            };
            let tmp_icon_to = tmp_icons_dir.join(&icon_subpath);
            let tmp_icon_dir = tmp_icon_to.parent().unwrap();
            if !tmp_icon_dir.exists() {
                let _ = create_dir_all(tmp_icon_dir);
            }
            match fs::copy(fpath.as_path(), tmp_icon_to) {
                Ok(_) => {
                    log::info!(
                        "Copied successfully from {:?} to path {:?}",
                        fpath,
                        icon_subpath
                    );
                    icon_paths.push(icon_subpath.to_str().unwrap().to_string());
                }
                Err(e) => {
                    log::warn!(
                        "could not copy file {:?} to path {:?} error: {:?}",
                        fpath,
                        icon_subpath,
                        e
                    );
                }
            }
        }
    }
    log::info!(
        "Successfully saved {} .desktop entries and {} icons!",
        entries_count,
        icon_paths.len()
    )
}
