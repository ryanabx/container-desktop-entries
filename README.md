# container-desktop-entries: Linux Desktop Entries for Containers!

![](res/container-desktop-entries.png)

This project adds linux desktop entries for applications installed inside containers! Support for toolbox is implemented, but podman and docker will need some testing (feel free to submit PRs!)

> **NOTE:** Requires: https://github.com/desktop-integration/desktop-entry-daemon

## Build/Install/Uninstall (Server, on host system)

**BUILD**

    just build

**INSTALL**

    just install
    systemctl --user enable container-desktop-entries

Reboot after installing!

**UNINSTALL**
    
    systemctl --user disable container-desktop-entries
    just uninstall

Reboot after uninstalling!

> **NOTE:** You **must** install the client software on every guest container you want to receive desktop entries from!


## Build/Install/Uninstall (Client, on guest container)

**BUILD**

    just build-client

**INSTALL**

    just install-client

**UNINSTALL**
    
    just uninstall-client

## Configuration

Configuring clients to get entries from is done in `$HOME/.config/container-desktop-entries/containers.conf`:

Example configuration (lines of [container name] [container type]):

    fedora-toolbox-39 toolbox
    fedora-toolbox-40 toolbox
    my-podman-container podman
    docker-linux-name docker

## Contributing

Just make a pull request! It'd be good to first make an issue in the issue tracker so that it's made known what you'd like to work on.