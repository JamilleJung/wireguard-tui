Name:           wireguard-tui
Version:        1.8.0
Release:        1%{?dist}
Summary:        A native Linux terminal UI (TUI) for managing WireGuard tunnels

License:        MIT
URL:            https://github.com/JamilleJung/wireguard-tui
Source0:        %{url}/archive/refs/tags/v%{version}.tar.gz#/%{name}-%{version}.tar.gz
ExclusiveArch:  x86_64 aarch64

# Pure Rust, no GUI/C libraries.
BuildRequires:  cargo
BuildRequires:  rust
BuildRequires:  gcc
BuildRequires:  pkgconf-pkg-config

Requires:       wireguard-tools
Requires:       polkit

%description
A native Linux terminal UI for managing WireGuard tunnels: tunnel list with
live status, activate/deactivate, edit in $EDITOR, import from .conf or QR,
in-terminal QR display, live throughput, and a small auditable privileged
helper (sudoers/polkit). Pure Rust, works great over SSH.

%prep
%autosetup

%build
cargo build --release --locked

%check
cargo test --release --locked

%install
install -Dm0755 target/release/wg-tui %{buildroot}%{_bindir}/wg-tui
install -Dm0755 target/release/wg-helper %{buildroot}%{_prefix}/lib/%{name}/wg-helper
install -Dm0644 packaging/49-wireguard-tui.rules %{buildroot}%{_datadir}/polkit-1/rules.d/49-wireguard-tui.rules

%files
%license LICENSE
%doc README.md
%{_bindir}/wg-tui
%dir %{_prefix}/lib/%{name}
%{_prefix}/lib/%{name}/wg-helper
%{_datadir}/polkit-1/rules.d/49-wireguard-tui.rules

%changelog
* Mon Jun 23 2026 jamillejung <izeystudio@gmail.com> - 1.8.0-1
- Version 1.8.0: connection diagnostics, in-app guide, changelog viewer.
- Extend ExclusiveArch to x86_64 + aarch64 (static binaries for both).
- Add explicit BuildRequires: pkgconf-pkg-config.

* Tue Jun 17 2026 jamillejung <izeystudio@gmail.com> - 1.4.1-1
- Initial RPM packaging (for COPR): live throughput + health, Easy mode,
  bulk import, hardened helper with timeouts.
