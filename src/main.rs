use clap::Parser;
use freedesktop_desktop_entry::DesktopEntry;
use freedesktop_icons::lookup;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::error::Error;

use std::fs::{self, create_dir_all};
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
    /// name of the toolbox to run this command for
    #[arg(short, long)]
    name: String,

    /// container protocol to use (docker | podman)
    #[arg(short, long)]
    protocol: String,

    /// run in client mode
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

    match mode {
        Mode::Client => {
            container_client(&args.name, &args.protocol);
        }
        #[cfg(feature = "server")]
        Mode::Server => {
            #[cfg(feature = "server")]
            return container_server(&args.name, &args.protocol).await;
        }
    }
    Ok(())
}

/// run a command on the container of choice
#[cfg(feature = "server")]
fn run_on_container(
    container_name: &str,
    container_type: &str,
    command: &str,
) -> io::Result<Output> {
    Command::new("sh")
        .arg("-c")
        .arg(format!(
            "{} run -c {} {}",
            container_type, container_name, command
        ))
        .output()
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

    if let Ok(_) = run_on_container(
        container_name,
        container_type,
        &format!(
            "toolbox-desktop-entries --client {} {}",
            container_name, container_type
        ),
    ) {
        log::info!("client successfully finished");

        let tmp_applications_from = Path::new("/tmp/toolbox-desktop-entries/applications/");
        let tmp_applications_to = Path::new("/tmp/toolbox-desktop-entries-server/applications/");
        let tmp_icons_from = Path::new("/tmp/toolbox-desktop-entries/icons/");
        let tmp_icons_to = Path::new("/tmp/toolbox-desktop-entries-server/icons/");

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
    }
    Ok(())
}

fn container_client(container_name: &str, container_type: &str) {
    let xdg_basedirs = BaseDirectories::new().expect("could not get xdg basedirectories");
    let data_dirs = xdg_basedirs.get_data_dirs();
    let tmp_applications_dir = Path::new("/tmp/toolbox-desktop-entries/applications/");
    let tmp_icons_dir = Path::new("/tmp/toolbox-desktop-entries/icons/");
    let _ = create_dir_all(tmp_applications_dir);
    let _ = create_dir_all(tmp_icons_dir);
    let mut icon_names: Vec<String> = Vec::new();
    let regex_handler = Regex::new(r"^Exec:\s?.*$").unwrap();
    for dirs in data_dirs {
        if let Ok(contents) = read_dir(dirs) {
            for entry in contents {
                let entry = entry.expect("can't find entry");
                let ty = entry.file_type().expect("can't get file type");
                if ty.is_file() {
                    if let Ok(txt) = read_to_string(&entry.path()) {
                        let mut f =
                            fs::File::create(tmp_applications_dir.join(entry.file_name())).unwrap();
                        let new_text = regex_handler
                            .replace(
                                &txt,
                                format!(
                                    "Exec: {} container run -c {} ",
                                    container_type, container_name
                                ),
                            )
                            .into_owned();
                        let _ = f.write_all(new_text.as_bytes());
                        if let Ok(desktop_entry) = DesktopEntry::decode(
                            &tmp_applications_dir.join(entry.file_name()),
                            &new_text,
                        ) {
                            if let Some(icon_name) = desktop_entry.icon() {
                                icon_names.push(icon_name.to_string());
                            }
                        }
                    }
                }
            }
        }
    }
    for icon in icon_names {
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
}
