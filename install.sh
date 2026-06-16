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

# System tools we rely on (visudo, resolvconf, runuser, ...) live in sbin. A
# normal user's PATH - which `su` carries into the root re-exec below - often
# omits sbin, which made `visudo -cf` and the `resolvconf` probe silently fail.
# Make sure they're findable no matter how we ended up running.
export PATH="/usr/local/sbin:/usr/sbin:/sbin:$PATH"

# ---------------------------------------------------------------------------
# Paths
# ---------------------------------------------------------------------------
PREFIX="${PREFIX:-/usr/local}"
LIBDIR="$PREFIX/lib/wireguard-tui"
BIN="$PREFIX/bin/wg-tui"
HELPER="$LIBDIR/wg-helper"
DESKTOP="$PREFIX/share/applications/wireguard-tui.desktop"
ICON_DIR="$PREFIX/share/icons/hicolor/scalable/apps"
ICON="$ICON_DIR/wireguard-tui.svg"
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
# Having the `sudo` binary is NOT enough: on many Debian servers the login user
# isn't in the sudoers file (sudo prompts, then says "is not in the sudoers
# file"). Treat sudo as usable only if a non-interactive check passes
# (passwordless / cached creds) or the user is in a typical admin group.
CAN_SUDO=0
if command -v sudo >/dev/null 2>&1; then
    if sudo -n true >/dev/null 2>&1; then
        CAN_SUDO=1
    else
        case " $(id -nG 2>/dev/null) " in
            *" sudo "*|*" wheel "*|*" admin "*) CAN_SUDO=1 ;;
        esac
    fi
fi

# The login user, taken from the REAL uid - never $USER, which the caller can set
# to anything (including a sudoers-injection payload). Validated before any use.
INVOKER="$(id -un 2>/dev/null || true)"

# If we can't use sudo, re-run the WHOLE installer as root ONCE (one ROOT-password
# prompt instead of one per privileged step). As root we'll install `sudo` if
# missing and write a passwordless drop-in for the invoking user - after which
# the app escalates with `sudo -n wg-helper` and never needs root again. Carry
# the invoking user across the re-exec so the drop-in targets *them*, not root.
if [ "$(id -u)" -ne 0 ] && [ "$CAN_SUDO" -eq 0 ] && command -v su >/dev/null 2>&1; then
    warn "No usable sudo here - re-running as root (enter the ROOT password once)."
    exec su root -c "WG_REAL_USER=$(printf '%q' "$INVOKER") $(printf '%q ' "$HERE/install.sh" "$@")"
fi

if [ "$(id -u)" -eq 0 ]; then
    as_root() { "$@"; }                                  # already root
    # Recover the human who invoked us: explicit hand-off, sudo, then the login tty.
    REAL_USER="${WG_REAL_USER:-${SUDO_USER:-$(logname 2>/dev/null || true)}}"
    [ -n "$REAL_USER" ] || REAL_USER="root"
elif [ "$CAN_SUDO" -eq 1 ]; then
    as_root() { sudo "$@"; }
    REAL_USER="${INVOKER:-$USER}"
else
    die "Need root. Re-run as root ('su -' then ./install.sh), or install 'su'/'sudo'."
fi

# Run a command as the invoking (non-root) user - used for the BUILD so cargo and
# rustup use that user's home and the source tree is never compiled as root (its
# build scripts / proc-macros would otherwise run with root privilege).
as_user() {
    if [ "$(id -u)" -eq 0 ] && [ -n "$REAL_USER" ] && [ "$REAL_USER" != "root" ]; then
        if command -v runuser >/dev/null 2>&1; then
            runuser -u "$REAL_USER" -- "$@"
        else
            su "$REAL_USER" -c "$(printf '%q ' "$@")"
        fi
    else
        "$@"
    fi
}

# A NOPASSWD sudoers drop-in scoped to the helper grants `sudo -n wg-helper` to
# REAL_USER even if they aren't otherwise a sudoer (sudoers rules are per-user,
# independent of the `sudo` group) - but it needs the `sudo` binary present. So
# "no sudo -> install sudo", then set it up. Skipped for an explicit --polkit.
ensure_sudo() {
    [ "$AUTH_MODE" = "polkit" ] && return 0
    command -v sudo >/dev/null 2>&1 && return 0
    say "Installing sudo (so the app can use a passwordless helper drop-in)"
    case "$PM" in
        apt-get)      as_root apt-get install -y sudo ;;
        dnf|yum)      as_root "$PM" install -y sudo ;;
        pacman)       as_root pacman -Sy --noconfirm sudo ;;
        zypper)       as_root zypper --non-interactive install sudo ;;
        apk)          as_root apk add --no-cache sudo ;;
        xbps-install) as_root xbps-install -Sy sudo ;;
        eopkg)        as_root eopkg install -y sudo ;;
        *) warn "Unknown package manager - install 'sudo' manually for passwordless use." ;;
    esac
    if command -v sudo >/dev/null 2>&1; then
        ok "sudo installed."
    elif command -v pkexec >/dev/null 2>&1; then
        warn "Could not install sudo - using a polkit rule (pkexec) instead."
        AUTH_MODE="polkit"
    else
        die "Could not install sudo, and pkexec/polkit isn't available either. \
Install 'sudo' manually and re-run, or run wg-tui as root."
    fi
}

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
        apt-get) as_root apt-get update -qq || true
                 as_root apt-get install -y wireguard-tools build-essential curl ca-certificates ;;
        dnf|yum) as_root "$PM" install -y wireguard-tools gcc curl ca-certificates ;;
        pacman)  as_root pacman -Sy --noconfirm --needed wireguard-tools base-devel curl ;;
        zypper)  as_root zypper --non-interactive install wireguard-tools gcc curl ca-certificates ;;
        apk)     as_root apk add --no-cache wireguard-tools build-base curl ;;
        xbps-install) as_root xbps-install -Sy wireguard-tools base-devel curl ;;
        eopkg)   as_root eopkg install -y wireguard-tools gcc curl ;;
        *) warn "Unknown package manager — make sure 'wireguard-tools', a C compiler and curl are installed." ;;
    esac
    ok "Dependencies installed."
}

# Ensure a Rust toolchain and build the release binary AS THE INVOKING USER (via
# as_user - never as root, so build scripts/proc-macros don't run with privilege
# and the toolchain + artifacts land in that user's home, not /root).
build_app() {
    say "Building release binary (first build downloads crates, ~1-2 min)"
    # Look up the build user's home safely (no eval on the name) so cargo/rustup
    # land there regardless of how runuser/su set the environment.
    local rh
    rh="$(getent passwd "$REAL_USER" 2>/dev/null | cut -d: -f6)"
    [ -n "$rh" ] || rh="$(awk -F: -v u="$REAL_USER" '$1==u {print $6; exit}' /etc/passwd 2>/dev/null || true)"
    [ -n "$rh" ] || rh="$HOME"
    as_user env HOME="$rh" sh -s "$HERE" <<'BUILD'
set -e
HERE="$1"
export PATH="$HOME/.cargo/bin:$PATH"
if ! command -v cargo >/dev/null 2>&1; then
    echo ":: Rust toolchain not found - installing via rustup"
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --no-modify-path
    # shellcheck disable=SC1091
    . "$HOME/.cargo/env"
fi
cd "$HERE"
cargo build --release
BUILD
    [ -f "$HERE/target/release/wg-tui" ] || die "Build did not produce a binary."
    ok "Built."
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

# wg-quick needs a resolvconf provider to apply a config's `DNS =` line. Install
# one (best-effort) when it's missing and systemd-resolved isn't handling DNS -
# the fix for minimal Debian, where tunnels with DNS= otherwise fail to connect.
ensure_resolvconf() {
    command -v resolvconf >/dev/null 2>&1 && return 0
    [ -d /run/systemd/resolve ] && return 0   # systemd-resolved already handles DNS=
    say "Installing a resolvconf provider (so tunnels with 'DNS =' can connect)"
    case "$PM" in
        apt-get)      as_root apt-get install -y openresolv || true ;;
        dnf|yum)      as_root "$PM" install -y openresolv || true ;;
        pacman)       as_root pacman -Sy --noconfirm openresolv || true ;;
        zypper)       as_root zypper --non-interactive install openresolv || true ;;
        apk)          as_root apk add openresolv || true ;;
        xbps-install) as_root xbps-install -Sy openresolv || true ;;
        eopkg)        as_root eopkg install -y openresolv || true ;;
    esac
    if command -v resolvconf >/dev/null 2>&1; then
        ok "resolvconf provider installed."
    else
        warn "Could not install a resolvconf provider; tunnels with a 'DNS =' line"
        warn "may fail until you install 'openresolv' (or use systemd-resolved)."
    fi
}

uninstall() {
    say "Removing wireguard-tui"
    as_root rm -f "$BIN" "$HELPER" "$DESKTOP" "$ICON" "$SUDOERS" "$POLKIT_RULE"
    as_root rmdir "$LIBDIR" 2>/dev/null || true
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
ensure_sudo
verify_build_deps
verify_runtime_deps
ensure_resolvconf
build_app

say "Installing into $PREFIX"
as_root install -d "$LIBDIR" "$PREFIX/bin" "$PREFIX/share/applications" "$ICON_DIR"
as_root install -m755 "$HERE/target/release/wg-tui" "$BIN"
as_root install -m755 "$HERE/packaging/wg-helper" "$HELPER"
as_root install -m644 "$HERE/packaging/wireguard-tui.desktop" "$DESKTOP"
as_root install -m644 "$HERE/packaging/wireguard-tui.svg" "$ICON"
command -v update-desktop-database >/dev/null 2>&1 && \
    as_root update-desktop-database "$PREFIX/share/applications" 2>/dev/null || true
ok "Files installed."

# ---------------------------------------------------------------------------
# Authorisation: sudoers drop-in (default) or polkit rule
# ---------------------------------------------------------------------------
if [ "$AUTH_MODE" = "polkit" ]; then
    say "Installing polkit rule (passwordless for active local sessions)"
    as_root install -d /etc/polkit-1/rules.d
    as_root install -m644 "$HERE/packaging/49-wireguard-tui.rules" "$POLKIT_RULE"
    ok "polkit rule installed."
elif [ -z "$REAL_USER" ] || [ "$REAL_USER" = "root" ]; then
    warn "Couldn't determine the invoking user - skipping the passwordless drop-in."
    warn "Run wg-tui as root, re-run as that user, or pass WG_REAL_USER=<name>."
elif ! printf '%s' "$REAL_USER" | grep -qE '^[a-zA-Z_][a-zA-Z0-9_-]*$' \
        || ! id "$REAL_USER" >/dev/null 2>&1; then
    # Reject anything that isn't a real, plain username - visudo alone does NOT
    # catch an injected spec like "u ALL=(ALL) NOPASSWD: ALL #".
    die "Refusing to write a sudoers rule for an invalid/unknown user: '$REAL_USER'."
else
    say "Installing sudoers drop-in (passwordless wg-helper for $REAL_USER)"
    tmp="$(mktemp)"
    printf '%s ALL=(root) NOPASSWD: %s\n' "$REAL_USER" "$HELPER" > "$tmp"
    if as_root visudo -cf "$tmp" >/dev/null 2>&1; then
        as_root install -m440 "$tmp" "$SUDOERS"
        ok "Passwordless helper set up for $REAL_USER (works even without sudo-group membership)."
    else
        warn "sudoers validation failed; skipping. Run wg-tui as root, or re-run: ./install.sh --polkit"
    fi
    rm -f "$tmp"
fi

printf "\n${G}Done!${N} Launch with:  ${B}wg-tui${N}\n"
if [ -n "$REAL_USER" ] && [ "$REAL_USER" != "root" ]; then
    printf "Run it as ${B}%s${N} - no sudo prompt, no need to be root.\n" "$REAL_USER"
fi
printf "Press ${B}?${N} inside the app for the full key map.\n"
