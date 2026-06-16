#!/usr/bin/env bash
#
# Universal installer for wireguard-tui (the wg-tui terminal app).
#
#   ./install.sh              build + install (auto-installs missing deps)
#   ./install.sh --polkit     use a polkit rule instead of a sudoers drop-in
#   ./install.sh uninstall    remove everything
#
# Works on Debian/Ubuntu, Fedora/RHEL, Arch/Manjaro, openSUSE, Alpine, Void
# and Solus. Run as a normal user — it calls sudo only where it must.
#
# Unlike the desktop client this app has NO C/GUI library dependencies: it only
# needs `wireguard-tools` at runtime and a Rust toolchain + a C linker to build.
set -euo pipefail

# ---------------------------------------------------------------------------
# Paths
# ---------------------------------------------------------------------------
PREFIX="${PREFIX:-/usr/local}"
LIBDIR="$PREFIX/lib/wireguard-tui"
BIN="$PREFIX/bin/wg-tui"
HELPER="$LIBDIR/wg-helper"
DESKTOP="$PREFIX/share/applications/wireguard-tui.desktop"
SUDOERS="/etc/sudoers.d/wireguard-tui"
POLKIT_RULE="/etc/polkit-1/rules.d/49-wireguard-tui.rules"
HERE="$(cd "$(dirname "$0")" && pwd)"

# ---------------------------------------------------------------------------
# Args
# ---------------------------------------------------------------------------
ACTION="install"; AUTH_MODE="sudoers"
for arg in "$@"; do
    case "$arg" in
        uninstall)  ACTION="uninstall" ;;
        --polkit)   AUTH_MODE="polkit" ;;
        --sudoers)  AUTH_MODE="sudoers" ;;
        *) ;;
    esac
done

# ---------------------------------------------------------------------------
# Pretty output
# ---------------------------------------------------------------------------
if [ -t 1 ]; then
    B="\033[1m"; G="\033[1;32m"; Y="\033[1;33m"; R="\033[1;31m"; C="\033[1;36m"; N="\033[0m"
else
    B=""; G=""; Y=""; R=""; C=""; N=""
fi
say()  { printf "${C}::${N} ${B}%s${N}\n" "$*"; }
ok()   { printf "${G}✓${N} %s\n" "$*"; }
warn() { printf "${Y}!${N} %s\n" "$*"; }
die()  { printf "${R}✗ %s${N}\n" "$*" >&2; exit 1; }

# ---------------------------------------------------------------------------
# Privilege helper
# ---------------------------------------------------------------------------
if [ "$(id -u)" -eq 0 ]; then
    SUDO=""; REAL_USER="${SUDO_USER:-root}"
else
    command -v sudo >/dev/null 2>&1 || die "Need root or sudo to install. Re-run as root."
    SUDO="sudo"; REAL_USER="$USER"
fi

# ---------------------------------------------------------------------------
# Detect the package manager
# ---------------------------------------------------------------------------
PM=""
for c in apt-get dnf yum pacman zypper apk xbps-install eopkg; do
    command -v "$c" >/dev/null 2>&1 && { PM="$c"; break; }
done

# Map (wireguard-tools, C toolchain) to per-distro package names.
install_pkgs() {
    say "Installing runtime + build dependencies"
    case "$PM" in
        apt-get) $SUDO apt-get update -qq
                 $SUDO apt-get install -y wireguard-tools build-essential curl ca-certificates ;;
        dnf|yum) $SUDO "$PM" install -y wireguard-tools gcc curl ca-certificates ;;
        pacman)  $SUDO pacman -Sy --noconfirm --needed wireguard-tools base-devel curl ;;
        zypper)  $SUDO zypper --non-interactive install wireguard-tools gcc curl ca-certificates ;;
        apk)     $SUDO apk add --no-cache wireguard-tools build-base curl ;;
        xbps-install) $SUDO xbps-install -Sy wireguard-tools base-devel curl ;;
        eopkg)   $SUDO eopkg install -y wireguard-tools gcc curl ;;
        *) warn "Unknown package manager — make sure 'wireguard-tools', a C compiler and curl are installed." ;;
    esac
    ok "Dependencies installed."
}

ensure_rust() {
    [ -x "$HOME/.cargo/bin/cargo" ] && export PATH="$HOME/.cargo/bin:$PATH"
    if command -v cargo >/dev/null 2>&1; then
        ok "Found cargo: $(cargo --version)"; return 0
    fi
    say "Rust toolchain not found — installing via rustup"
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --no-modify-path
    # shellcheck disable=SC1091
    . "$HOME/.cargo/env"
    export PATH="$HOME/.cargo/bin:$PATH"
    command -v cargo >/dev/null 2>&1 || die "Rust install failed; install cargo manually and re-run."
    ok "Installed $(cargo --version)"
}

# Verify the toolchain is actually usable (minimal installs often lack a linker).
verify_build_deps() {
    say "Checking the build toolchain"
    if ! command -v cc >/dev/null 2>&1 && ! command -v gcc >/dev/null 2>&1; then
        die "No C compiler (cc/gcc) found — install one (e.g. build-essential / base-devel) and re-run."
    fi
    ok "Build toolchain OK."
}

verify_runtime_deps() {
    command -v wg >/dev/null 2>&1 && command -v wg-quick >/dev/null 2>&1 \
        || warn "wireguard-tools (wg/wg-quick) not found — the app needs them at runtime."
}

uninstall() {
    say "Removing wireguard-tui"
    $SUDO rm -f "$BIN" "$HELPER" "$DESKTOP" "$SUDOERS" "$POLKIT_RULE"
    $SUDO rmdir "$LIBDIR" 2>/dev/null || true
    ok "Uninstalled. (Your /etc/wireguard configs were left untouched.)"
    exit 0
}

# ---------------------------------------------------------------------------
# Go
# ---------------------------------------------------------------------------
printf "${B}wireguard-tui installer${N}\n"
[ "$ACTION" = "uninstall" ] && uninstall
[ -n "$PM" ] && say "Detected package manager: ${PM}" || true

install_pkgs
ensure_rust
verify_build_deps
verify_runtime_deps

say "Building release binary (first build downloads crates, ~1–2 min)"
( cd "$HERE" && cargo build --release )
[ -f "$HERE/target/release/wg-tui" ] || die "Build did not produce a binary."
ok "Built."

say "Installing into $PREFIX"
$SUDO install -d "$LIBDIR" "$PREFIX/bin" "$PREFIX/share/applications"
$SUDO install -m755 "$HERE/target/release/wg-tui" "$BIN"
$SUDO install -m755 "$HERE/packaging/wg-helper" "$HELPER"
$SUDO install -m644 "$HERE/packaging/wireguard-tui.desktop" "$DESKTOP"
command -v update-desktop-database >/dev/null 2>&1 && \
    $SUDO update-desktop-database "$PREFIX/share/applications" 2>/dev/null || true
ok "Files installed."

# ---------------------------------------------------------------------------
# Authorisation: sudoers drop-in (default) or polkit rule
# ---------------------------------------------------------------------------
if [ "$AUTH_MODE" = "polkit" ]; then
    say "Installing polkit rule (passwordless for active local sessions)"
    $SUDO install -d /etc/polkit-1/rules.d
    $SUDO install -m644 "$HERE/packaging/49-wireguard-tui.rules" "$POLKIT_RULE"
    ok "polkit rule installed."
else
    say "Installing sudoers drop-in (passwordless wg-helper for $REAL_USER)"
    tmp="$(mktemp)"
    printf '%s ALL=(root) NOPASSWD: %s\n' "$REAL_USER" "$HELPER" > "$tmp"
    if $SUDO visudo -cf "$tmp" >/dev/null 2>&1; then
        $SUDO install -m440 "$tmp" "$SUDOERS"
        ok "sudoers drop-in installed."
    else
        warn "sudoers validation failed; skipping. You'll be prompted by pkexec instead."
    fi
    rm -f "$tmp"
fi

printf "\n${G}Done!${N} Launch with:  ${B}wg-tui${N}\n"
printf "Press ${B}?${N} inside the app for the full key map.\n"
