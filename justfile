name := 'container-desktop-entries'
client-name := 'container-desktop-entries'

rootdir := ''
prefix := '/usr'

base-dir := absolute_path(clean(rootdir / prefix))

conf-dir := '/etc'
lib-dir := '/lib'

exec-src := 'target' / 'release' / name
exec-dst := base-dir / 'bin' / name
exec-client-dst := base-dir / 'bin' / client-name

service-src := 'systemd' / 'container-desktop-entries'
service-dst := lib-dir / 'systemd' / 'user' / 'container-desktop-entries.service'

build *args:
    cargo build --release {{args}}

build-client *args:
    cargo build --release {{args}}

install:
    install -Dm0755 {{exec-src}} {{exec-dst}}
    install -Dm0644 {{service-src}} {{service-dst}}

install-client:
    install -Dm0755 {{exec-src}} {{exec-client-dst}}

uninstall:
    rm -f {{exec-dst}}
    rm -f {{service-dst}}

uninstall-client:
    rm -f {{exec-client-dst}}