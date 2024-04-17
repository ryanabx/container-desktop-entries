use freedesktop_desktop_entry::{default_paths, DesktopEntry};
use freedesktop_icon_lookup::Cache;
use regex::Regex;
use std::{
    fs::{read, read_to_string},
    process,
};
use zbus::Connection;

use crate::{desktop_entry::DesktopEntryProxy, ContainerType};

#[derive(Debug)]
pub enum ClientError {
    Zbus(zbus::Error),
    Icon(freedesktop_icon_lookup::Error),
}

impl From<zbus::Error> for ClientError {
    fn from(value: zbus::Error) -> Self {
        ClientError::Zbus(value)
    }
}

impl From<freedesktop_icon_lookup::Error> for ClientError {
    fn from(value: freedesktop_icon_lookup::Error) -> Self {
        ClientError::Icon(value)
    }
}

/// Client implementation for container-desktop-entries
pub async fn client(
    container_name: &str,
    container_type: ContainerType,
) -> Result<(), ClientError> {
    let connection = Connection::session().await?;
    let proxy = DesktopEntryProxy::new(&connection).await?;

    let icon_lookup = Cache::new()?;

    let exec_regex = Regex::new(container_type.format_exec_regex_pattern().as_str()).unwrap();
    let name_regex = Regex::new(container_type.format_name_regex_pattern().as_str()).unwrap();
    // First, we look up all the desktop files
    log::debug!("Paths before dedup: {:?}", default_paths());
    let mut paths = default_paths();
    paths.sort();
    paths.dedup();
    log::debug!("Paths after dedup: {:?}", paths);
    for path in freedesktop_desktop_entry::Iter::new(paths) {
        // let path_src = PathSource::guess_from(&path);
        if let Ok(file_text) = read_to_string(&path) {
            // run regex on it now
            let file_text = exec_regex
                .replace_all(
                    &file_text,
                    container_type.format_desktop_exec(container_name),
                )
                .to_string();
            let file_text = name_regex
                .replace_all(
                    &file_text,
                    container_type.format_desktop_name(container_name),
                )
                .to_string();

            if let Ok(entry) = DesktopEntry::decode(&path, &file_text) {
                // We have a valid desktop entry
                if entry.no_display() {
                    continue; // We don't want to push NoDisplay entries into our host
                }

                println!("{}", entry.to_string());

                match proxy.register_entry(&entry.appid, &file_text).await {
                    Ok(_) => {
                        log::info!("Daemon registered entry: {}", entry.appid);
                        if let Some(icon_name) = entry.icon() {
                            if let Some(icon_path) = icon_lookup.lookup(icon_name, None) {
                                match icon_path.extension().map(|p| p.to_str().unwrap()) {
                                    Some("png" | "svg") => {
                                        let file_bytes = read(icon_path).unwrap();
                                        match proxy
                                            .register_icon(icon_name, file_bytes.as_slice())
                                            .await
                                        {
                                            Ok(_) => {
                                                log::info!("Daemon registered icon: {}", icon_name);
                                            }
                                            Err(e) => {
                                                log::error!("Error (icons): {:?}", e);
                                            }
                                        }
                                    }
                                    _ => {}
                                }
                            }
                        }
                    }
                    Err(e) => {
                        log::error!("Error (entry): {}", e);
                    }
                }
            }
        }
    }
    loop {
        // Busy wait until logging off, keeping the desktop entries alive
        std::future::pending::<()>().await;
    }
}
