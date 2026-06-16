# Security Policy

## Reporting a vulnerability

Please report security issues privately via GitHub Security Advisories
(**Security → Report a vulnerability**) on this repository, rather than opening a
public issue. You'll get an acknowledgement as soon as possible. Please do not
include real private keys or production configs in a report.

## Threat model

`wg-tui` manages WireGuard tunnels under `/etc/wireguard`, which requires root.
The design goal is to keep the part that runs as root as small and auditable as
possible, and to keep everything else unprivileged.

**In scope** (things we actively defend against):

- A bug or hijacked environment in the **unprivileged UI** escalating to root.
- The privileged helper being tricked into touching files outside
  `/etc/wireguard` (path traversal) or running attacker-chosen commands.
- A malicious or malformed `.conf`/QR import corrupting existing tunnels or
  being saved without the user understanding what it does.
- Truncated/lost configs from interrupted writes.

**Out of scope** (cannot be defended against here):

- An attacker who is **already root**, or who can already run code **as your
  user** (they can read your keys directly; pinning a binary path doesn't help).
- The security of WireGuard itself, the kernel module, or `wg`/`wg-quick`.
- Physical access / a compromised terminal emulator.

## The privilege boundary

The TUI runs as your normal user. **The only thing that runs as root is one
small shell script**, `packaging/wg-helper`, invoked as
`sudo -n wg-helper <verb> [name]` (sudoers mode) or `pkexec wg-helper …`
(polkit / fallback). Authorisation is scoped to **exactly that one script path**:

- the **sudoers** drop-in grants passwordless execution of only
  `/usr/local/lib/wireguard-tui/wg-helper` for your user;
- the **polkit** rule allows `pkexec` of only that program for an active local
  session.

Because the grant is bound to the absolute helper path, pointing the app at a
different script (e.g. via `$WG_HELPER`) cannot silently gain root — it would
fall outside the sudoers/polkit grant and prompt or fail. In release builds the
helper-path override is additionally refused unless `WG_ALLOW_UNSAFE_HELPER=1`
is set and the target is an absolute, root-owned, non-world-writable file.

The helper itself:

- exports a **fixed `PATH`** (`/usr/sbin:/usr/bin:/sbin:/bin`) so a hijacked
  caller `PATH` can't redirect the `wg`/`wg-quick`/`logger` it runs as root;
- **validates every tunnel name** against `^[A-Za-z0-9][A-Za-z0-9_.-]{0,14}$`
  and rejects `..`, so `"$WG_DIR/<name>.conf"` can never escape `/etc/wireguard`;
- writes configs **atomically** (temp file + `rename`, `umask 077`) and keeps a
  **timestamped 0600 backup** before any overwrite or delete;
- **logs every mutating action** (with the invoking user) to the journal
  (`logger -t wireguard-tui`).

## `PostUp` / `PreUp` / `PostDown` / `PreDown` run as root

These `wg-quick` directives execute **arbitrary shell commands as root** when a
tunnel is brought up or down. A `.conf` from an untrusted source can therefore
run anything. `wg-tui`:

- **flags** imported/edited configs that contain these directives, and
- only ever activates a tunnel through `wg-quick`, never by interpreting the
  script lines itself.

**Treat a `.conf` like a script you are about to run as root.** Only activate
tunnels whose source you trust.

## Private keys and QR codes

- A tunnel `.conf` contains the interface **private key**. Files are written
  `0600`; backups are `0600` in `/etc/wireguard/.backup`.
- The **editor** opens a temporary copy of the config. It is created with
  `O_EXCL` and mode `0600` inside a per-user private directory
  (`$XDG_RUNTIME_DIR`, or a `0700` fallback), so another local user cannot
  pre-plant a symlink to read the key or steer your editor, and it is removed
  afterwards.
- **Show QR** renders the full config — *including the private key* — as a QR
  code. Anyone who photographs your screen gets the key. Only display it when
  it's safe to do so, and prefer a maximised window so it scans.
- **Export** writes every tunnel's `.conf` into a `.zip`; that archive contains
  private keys. Store it somewhere safe and delete it when done.

## Supply chain & verifying a download

- Third-party GitHub Actions are pinned to commit SHAs.
- Each release ships a `SHA256SUMS` file. Verify a download with:

```sh
sha256sum -c SHA256SUMS
```

When in doubt, **build from source** — the project is pure Rust with no GUI/C
dependencies, so `cargo build --release` is reproducible on any supported distro.
