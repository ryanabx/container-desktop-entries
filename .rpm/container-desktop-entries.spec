# Generated by rust2rpm 26
%bcond_without check

# prevent library files from being installed
%global cargo_install_lib 0

%global crate container-desktop-entries

%global ver ###
%global commit ###
%global date ###

Name:           container-desktop-entries
Version:        %{ver}~git%{date}.%{sub %{commit} 1 7}
Release:        %autorelease
Summary:        A tool to add guest container desktop entries to the host system

SourceLicense:  Apache-2.0
# FIXME: paste output of %%cargo_license_summary here
License:        # FIXME
# LICENSE.dependencies contains a full license breakdown

URL:            https://github.com/ryanabx/container-desktop-entries
Source:         container-desktop-entries-%{commit}.tar.xz
Source:         container-desktop-entries-%{commit}-vendor.tar.xz

BuildRequires:  cargo-rpm-macros >= 26
BuildRequires:  rustc
BuildRequires:  cargo
BuildRequires:  just

BuildRequires:  systemd-rpm-macros

Requires:       desktop-entry-daemon

%global _description %{expand:
%{summary}.}

%description %{_description}

%prep
%autosetup -n %{name}-%{commit} -p1 -a1
%cargo_prep -N
cat .vendor/config.toml >> .cargo/config

%build
%cargo_build
%{cargo_license_summary}
%{cargo_license} > LICENSE.dependencies
%{cargo_vendor_manifest}

%install
install -Dm0755 target/release/container-desktop-entries %{buildroot}/%{_bindir}/container-desktop-entries
install -Dm0644 systemd/container-desktop-entries.service %{buildroot}/%{_userunitdir}/container-desktop-entries.service

%if %{with check}
%check
%cargo_test
%endif

%post
%systemd_post %{name}.service

%preun
%systemd_preun %{name}.service

%postun
%systemd_postun_with_restart %{name}.service

%files
%license LICENSE
%license LICENSE.dependencies
# %%license cargo-vendor.txt
%doc README.md
%{_bindir}/%{name}
%{_userunitdir}/%{name}.service

%changelog
%autochangelog
