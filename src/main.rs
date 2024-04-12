use clap::Parser;
use freedesktop_desktop_entry::DesktopEntry;
use freedesktop_icons::lookup;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::env;
use std::error::Error;

use std::fs::{self, create_dir_all, remove_dir_all, File};

#[cfg(feature = "server")]
use std::fmt::Display;
#[cfg(feature = "server")]
use std::io;
#[cfg(feature = "server")]
use std::process::Command;
#[cfg(feature = "server")]
use std::process::Output;
use std::{
    fs::{read_dir, read_to_string},
    io::Write,
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

enum Mode {
    Client,
    #[cfg(feature = "server")]
    Server,
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

    let names_and_protocols = if args.name_and_protocol.is_some() {
        let s = args
            .name_and_protocol
            .as_ref()
            .unwrap()
            .split_once(" ")
            .unwrap();
        vec![(s.0.to_string(), s.1.to_string())]
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
            (ss.0.to_string(), ss.1.to_string())
        })
        .collect::<Vec<_>>()
    };

    for (name, protocol) in names_and_protocols {
        match mode {
            Mode::Client => {
                log::info!("Running as client: {} {}", name, protocol);
                container_client(&name, &protocol);
            }
            #[cfg(feature = "server")]
            Mode::Server => {
                log::info!("Running as server: {} {}", name, protocol);
                if let Err(e) = container_server(&name, &protocol).await {
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
    container_type: &str,
    command: &str,
) -> Result<(String, String), ContainerError> {
    log::debug!(
        "Full command: sh -c {} container exec {} {}",
        container_type,
        container_name,
        command
    );
    let out = Command::new("sh")
        .arg("-c")
        .arg(format!(
            "{} container exec {} {}",
            container_type, container_name, command
        ))
        .output();
    match out {
        Ok(ref o) => {
            let std_out = String::from_utf8(o.stdout.clone()).unwrap();
            let std_err = String::from_utf8(o.stderr.clone()).unwrap();
            log::debug!("{:?}", std_out);
            log::debug!("{:?}", std_err);
            if std_err.contains("command not found") || std_out.contains("command not found") {
                Err(ContainerError::CommandNotFound)
            } else {
                Ok((std_out, std_err))
            }
        }
        Err(ref e) => {
            log::debug!("error: {:?}", e);
            Err(ContainerError::IO(out.unwrap_err()))
        }
    }
}

#[cfg(feature = "server")]
/// copy files on the container of choice
fn copy_from_container(
    container_name: &str,
    container_type: &str,
    from: &str,
    to: &str,
) -> io::Result<Output> {
    Command::new("sh")
        .arg("-c")
        .arg(format!(
            "{} cp {}:{}/. {}",
            container_type, container_name, from, to
        ))
        .output()
}

#[derive(Serialize, Deserialize)]
struct ClientData {
    desktop_entries: Vec<String>,
    icons: Vec<String>,
}

#[cfg(feature = "server")]
async fn container_server(
    container_name: &str,
    container_type: &str,
) -> Result<(), Box<dyn Error>> {
    let connection = Connection::session().await?;

    let proxy = DesktopEntryProxy::new(&connection).await?;

    let _ = run_on_container(
        container_name,
        container_type,
        &format!(
            "container-desktop-entries-client --client --name-and-protocol '{} {}'",
            container_name, container_type
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
    );
    let _ = copy_from_container(
        container_name,
        container_type,
        tmp_icons_from.to_str().unwrap(),
        tmp_icons_to.to_str().unwrap(),
    );

    if let Ok(read_dir) = read_dir(tmp_applications_to) {
        let entries = read_dir
            .into_iter()
            .filter_map(|f| match f {
                Ok(s) => Some(s.path().to_str().unwrap().to_string()),
                Err(_) => None,
            })
            .collect::<Vec<_>>();

        let _ = proxy
            .register_entries(&entries.iter().map(String::as_ref).collect::<Vec<_>>())
            .await?;
    }

    if let Ok(read_dir) = read_dir(tmp_icons_to) {
        let icons = read_dir
            .into_iter()
            .filter_map(|f| match f {
                Ok(s) => Some(s.path().to_str().unwrap().to_string()),
                Err(_) => None,
            })
            .collect::<Vec<_>>();

        let _ = proxy
            .register_icons(&icons.iter().map(String::as_ref).collect::<Vec<_>>())
            .await?;
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

fn container_client(container_name: &str, container_type: &str) {
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
    let regex_handler = Regex::new(r"^Exec:\s?.*$").unwrap();
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
                            let mut f =
                                fs::File::create_new(tmp_applications_dir.join(entry.file_name()))
                                    .expect("file already exists");
                            let new_text = regex_handler
                                .replace(
                                    &txt,
                                    format!(
                                        "Exec: {} container exec {} ",
                                        container_type, container_name
                                    ),
                                )
                                .into_owned();
                            let _ = f.write_all(new_text.as_bytes());
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
                    } else {
                        log::warn!("Skipping {:?} as it is either not a file or does not end with .desktop", entry)
                    }
                }
            }
            Err(_) => {
                log::warn!("Could not read data directory {:?}", app_dir);
            }
        }
    }
    for icon in &icon_names {
        let f = lookup(&icon).with_cache().find();
        if let Some(fpath) = f {
            let _ = fs::copy(
                fpath.as_path(),
                tmp_icons_dir.join(Path::new(
                    fpath.as_path().file_name().unwrap().to_str().unwrap(),
                )),
            );
        }
    }
    log::info!(
        "Successfully saved {} .desktop entries and {} icons!",
        entries_count,
        icon_names.len()
    )
}
