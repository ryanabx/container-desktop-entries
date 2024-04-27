use std::{
    env,
    fs::{self, create_dir, read, read_to_string},
    io,
    path::{Path, PathBuf},
    process::{self, Command},
};

use freedesktop_desktop_entry::DesktopEntry;
use regex::Regex;
use walkdir::WalkDir;
use zbus::Connection;

use crate::{desktop_entry::DesktopEntryProxy, ContainerType};

#[derive(Debug)]
pub enum ServerError {}

#[derive(Debug)]
pub enum ClientSetupError {
    IO(io::Error),
    Zbus(zbus::Error),
}

impl From<io::Error> for ClientSetupError {
    fn from(value: io::Error) -> Self {
        ClientSetupError::IO(value)
    }
}

impl From<zbus::Error> for ClientSetupError {
    fn from(value: zbus::Error) -> Self {
        ClientSetupError::Zbus(value)
    }
}

pub async fn server(containers: Vec<(String, ContainerType)>) -> Result<(), ServerError> {
    let home = match env::var("RUNTIME_DIRECTORY") {
        Ok(h) => h,
        Err(_) => {
            log::error!("RUNTIME_DIRECTORY NOT FOUND. Make sure you're using the service!");
            panic!()
        }
    };
    let to_path = Path::new(&home).join(Path::new(".cache/container-desktop-entries/"));
    for (container_name, container_type) in containers {
        if container_type.not_supported() {
            log::error!(
                "Container type {:?} is currently not supported!",
                container_type
            );
            continue;
        }
        if let Err(kind) = set_up_client(&container_name, container_type, &to_path).await {
            log::error!("Error setting up client {}: {:?}", container_name, kind);
        }
    }
    // let _ = fs::remove_dir_all(&to_path);
    loop {
        // Busy wait until logging off, keeping the desktop entries alive
        std::future::pending::<()>().await;
    }
}

async fn set_up_client(
    container_name: &str,
    container_type: ContainerType,
    to_path: &Path,
) -> Result<(), ClientSetupError> {
    // Start client if client is not running
    start_client(container_name, container_type)?;
    if !to_path.exists() {
        log::warn!(
            "Runtime directory {} does not exist! Attempting to create directory manually...",
            to_path.to_str().unwrap()
        );
        match create_dir(to_path) {
            Ok(_) => {
                log::info!("App directory created!");
            }
            Err(e) => {
                log::error!("App directory could not be created. Reason: {}", e);
                panic!("App directory could not be created");
            }
        }
    }
    let _ = fs::create_dir(&to_path.join("applications"));
    let _ = fs::create_dir(&to_path.join("icons"));
    let _ = fs::create_dir(&to_path.join("pixmaps"));
    // Find the data dirs and iterate over them
    let data_dirs = run_in_client(
        container_name,
        container_type,
        "env | grep XDG_DATA_DIRS | cut -d'=' -f2",
        true,
    )?
    .unwrap()
    .trim()
    .to_string();
    log::debug!("Data dirs: '{}'", data_dirs);
    for x in data_dirs.split(":").map(|p| Path::new(p)) {
        copy_from_client(
            container_name,
            container_type,
            &x.join("applications"),
            &to_path.join("applications"),
        )?;
        copy_from_client(
            container_name,
            container_type,
            &x.join("icons"),
            &to_path.join("icons"),
        )?;
    }
    copy_from_client(
        container_name,
        container_type,
        Path::new("/usr/share/pixmaps"),
        &to_path.join("pixmaps"),
    )?;
    let connection = Connection::session().await?;
    let proxy = DesktopEntryProxy::new(&connection).await?;

    // Desktop file parsing + icon lookup
    let exec_regex = Regex::new(container_type.format_exec_regex_pattern().as_str()).unwrap();
    let name_regex = Regex::new(container_type.format_name_regex_pattern().as_str()).unwrap();

    for entry_path in fs::read_dir(to_path.join("applications")).unwrap() {
        let path_buf = entry_path.unwrap().path();
        log::debug!("Looking at path: {:?}", path_buf);
        if !path_buf.exists() {
            log::warn!("Path {:?} doesn't exist!", path_buf);
            continue;
        }
        match read_to_string(&path_buf) {
            Ok(file_text) => {
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

                match DesktopEntry::decode(&path_buf, &file_text) {
                    Ok(entry) => {
                        // We have a valid desktop entry
                        if entry.no_display() {
                            log::warn!("No display entry");
                            continue; // We don't want to push NoDisplay entries into our host
                        }

                        match proxy.new_session_entry(&entry.appid, &file_text).await {
                            Ok(_) => {
                                log::info!("Daemon registered entry: {}", entry.appid);
                                if let Some(icon_name) = entry.icon() {
                                    if let Some(icon_path) = lookup_icon(
                                        icon_name,
                                        &to_path.join("icons"),
                                        &to_path.join("pixmaps"),
                                    ) {
                                        log::debug!(
                                            "Found icon path that matches! {:?}",
                                            icon_path
                                        );
                                        match icon_path.extension().map(|p| p.to_str().unwrap()) {
                                            Some("png" | "svg") => {
                                                let file_bytes = read(icon_path).unwrap();
                                                match proxy
                                                    .new_session_icon(
                                                        icon_name,
                                                        file_bytes.as_slice(),
                                                    )
                                                    .await
                                                {
                                                    Ok(_) => {
                                                        log::info!(
                                                            "Daemon registered icon: {}",
                                                            icon_name
                                                        );
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
                    Err(e) => {
                        log::error!(
                            "Could not read as valid desktop entry '{}' reason: {}",
                            file_text,
                            e.to_string()
                        );
                    }
                }
            }
            Err(e) => {
                log::error!(
                    "Could not read path {:?} to string. Reason: {}",
                    path_buf,
                    e.to_string()
                );
            }
        }
    }
    let _ = fs::remove_dir_all(&to_path.join("applications"));
    let _ = fs::remove_dir_all(&to_path.join("icons"));
    let _ = fs::remove_dir_all(&to_path.join("pixmaps"));
    Ok(())
}

fn lookup_icon(name: &str, base_path: &Path, pixmap_path: &Path) -> Option<PathBuf> {
    WalkDir::new(base_path)
        .follow_links(false)
        .into_iter()
        .filter_map(|e| match e {
            Ok(d) => {
                if d.path()
                    .file_stem()
                    .is_some_and(|stem| stem.to_str().unwrap() == name)
                {
                    Some(d)
                } else {
                    None
                }
            }
            Err(_) => None,
        })
        .max_by_key(|entry| {
            if let Some(ext_os) = entry.path().extension() {
                let ext = ext_os.to_str().unwrap();
                match ext {
                    "svg" => u32::MAX,
                    "png" => {
                        if let Some(p1) = entry.path().parent() {
                            if let Some(p2) = p1.parent() {
                                if let Some((a, _)) =
                                    p2.file_name().unwrap().to_str().unwrap().split_once("x")
                                {
                                    if let Ok(size) = a.parse::<u32>() {
                                        return size;
                                    }
                                }
                            }
                        }
                        u32::MIN
                    }
                    _ => u32::MIN,
                }
            } else {
                u32::MIN
            }
        })
        .map(|entry| entry.path().to_path_buf())
        .or(WalkDir::new(pixmap_path)
            .follow_links(false)
            .into_iter()
            .find(|e| {
                e.as_ref().is_ok_and(|d| {
                    d.path()
                        .file_stem()
                        .is_some_and(|stem| stem.to_str().unwrap() == name)
                })
            })
            .map(|f| f.unwrap().path().to_path_buf()))
}

/// start the client
fn start_client(
    container_name: &str,
    container_type: ContainerType,
) -> Result<Option<String>, io::Error> {
    shell_command(&container_type.format_start(container_name), true)
}

/// run a command on the container of choice
fn run_in_client(
    container_name: &str,
    container_type: ContainerType,
    command: &str,
    wait_for_output: bool,
) -> Result<Option<String>, io::Error> {
    shell_command(
        &container_type.format_exec(container_name, command),
        wait_for_output,
    )
}

/// copy a folder from the container of choice
fn copy_from_client(
    container_name: &str,
    container_type: ContainerType,
    from: &Path,
    to: &Path,
) -> Result<Option<String>, io::Error> {
    shell_command(&container_type.format_copy(container_name, from, to), true)
}

fn shell_command(command: &str, wait_for_output: bool) -> Result<Option<String>, io::Error> {
    log::debug!("Full command: sh -c '{}'", command);
    if wait_for_output {
        let out = Command::new("sh")
            .arg("-c")
            .arg(format!("{}", command))
            .output()
            .expect(&format!("Command {} failed", command));
        log::debug!(
            "Output completed! stdout: '{}', stderr: '{}'",
            String::from_utf8(out.stdout.clone()).unwrap(),
            String::from_utf8(out.stderr).unwrap()
        );
        Ok(Some(String::from_utf8(out.stdout).unwrap()))
    } else {
        let child_handle = Command::new("sh")
            .arg("-c")
            .arg(format!("{}", command))
            .spawn()
            .expect(&format!("Command {} failed", command));
        log::debug!("Started child process with pid {}", child_handle.id());
        Ok(None)
    }
}
