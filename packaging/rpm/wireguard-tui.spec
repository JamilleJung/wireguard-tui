Name:           wireguard-tui
Version:        1.10.0
Release:        1%{?dist}
Summary:        Terminal UI for managing WireGuard tunnels

# The binaries statically link the Rust crates pinned in Cargo.lock; they are
# shipped in Source1 together with their license files.
License:        MIT
URL:            https://github.com/JamilleJung/wireguard-tui
Source0:        %{url}/archive/refs/tags/v%{version}.tar.gz#/%{name}-%{version}.tar.gz
# Locked crate sources for an offline build: packaging/update-checksums.sh --vendor
Source1:        %{name}-%{version}-vendor.tar.xz
ExclusiveArch:  x86_64 aarch64

# Pure Rust, no GUI/C libraries.
BuildRequires:  cargo >= 1.88
BuildRequires:  rust >= 1.88
BuildRequires:  gcc
# The test suite: the helper's process tests spawn python3.
BuildRequires:  python3

Requires:       wireguard-tools
Requires:       iproute
Requires:       polkit
# The kill switch prefers nftables and falls back to iptables.
Requires:       (nftables or iptables-nft or iptables-legacy)
Recommends:     systemd-resolved
Suggests:       wl-clipboard
Suggests:       minisign

%description
A native Linux terminal UI for managing WireGuard tunnels: tunnel list with
live status, activate/deactivate, edit in $EDITOR, import from configuration
files or QR codes, in-terminal QR display, live throughput, and a small
auditable privileged helper authorized through polkit or a per-user rule.
Pure Rust, works great over SSH.

%prep
%autosetup -n %{name}-%{version} -a 1
# Build only from the vendored, checksummed crate sources (no network).
mkdir -p .cargo
cat >> .cargo/config.toml << 'CARGO_CONFIG'
[source.crates-io]
replace-with = "vendored-sources"

[source.vendored-sources]
directory = "vendor"
CARGO_CONFIG

%build
cargo build --frozen --release

%check
cargo test --frozen --release

%install
install -Dm0755 target/release/wg-tui %{buildroot}%{_bindir}/wg-tui
install -Dm0755 target/release/wg-helper %{buildroot}%{_prefix}/lib/%{name}/wg-helper
install -Dm0644 packaging/49-wireguard-tui.rules %{buildroot}%{_datadir}/polkit-1/rules.d/49-wireguard-tui.rules
install -Dm0644 packaging/wireguard-tui.desktop %{buildroot}%{_datadir}/applications/%{name}.desktop
install -Dm0644 packaging/wireguard-tui.svg %{buildroot}%{_datadir}/icons/hicolor/scalable/apps/%{name}.svg
# Tells the in-app updater that rpm owns this copy.
printf 'format=1\napp=%s\nmethod=package\nmanager=rpm\n' %{name} \
    > %{buildroot}%{_prefix}/lib/%{name}/install-manifest

%post
if [ -e /usr/local/bin/wg-tui ] || [ -e /usr/local/lib/%{name}/wg-helper ]; then
    echo "warning: another %{name} is installed under /usr/local and shadows this package;" >&2
    echo "         remove it with: PREFIX=/usr/local ./install.sh uninstall" >&2
fi
:

%files
%license LICENSE
%doc README.md
%{_bindir}/wg-tui
%dir %{_prefix}/lib/%{name}
%{_prefix}/lib/%{name}/wg-helper
%{_prefix}/lib/%{name}/install-manifest
%{_datadir}/polkit-1/rules.d/49-wireguard-tui.rules
%{_datadir}/applications/%{name}.desktop
%{_datadir}/icons/hicolor/scalable/apps/%{name}.svg

%changelog
* Thu Nov 05 2026 jamillejung <izeystudio@gmail.com> - 1.10.0-1
- Update to 1.10.0.

* Mon Nov 02 2026 jamillejung <izeystudio@gmail.com> - 1.9.3-1
- Reuse existing polkit group/membership on Alpine/BusyBox reinstalls.

* Mon Nov 02 2026 jamillejung <izeystudio@gmail.com> - 1.9.2-1
- Deep audit: shared config validation, bounded helpers, safe backups and updates.
- Serialize tunnel/firewall mutations and require complete kill-switch protection.

* Mon Nov 02 2026 jamillejung <izeystudio@gmail.com> - 1.9.1-1
- Update dependencies and repair backup import compatibility.
- Harden signed updates, helper command output, and kill switch isolation.

* Mon Jul 27 2026 jamillejung <izeystudio@gmail.com> - 1.9.0-1
- Version 1.9.0: detect an inactive resolvectl shim on RHEL-family hosts, so
  tunnels with a 'DNS =' line no longer fail after a healthy-looking check.
- Recommends: systemd-resolved.

* Tue Jun 23 2026 jamillejung <izeystudio@gmail.com> - 1.8.0-1
- Version 1.8.0: connection diagnostics, in-app guide, changelog viewer.
- Extend ExclusiveArch to x86_64 + aarch64 (static binaries for both).
- Add explicit BuildRequires: pkgconf-pkg-config.

* Wed Jun 17 2026 jamillejung <izeystudio@gmail.com> - 1.4.1-1
- Initial RPM packaging (for COPR): live throughput + health, Easy mode,
  bulk import, hardened helper with timeouts.
