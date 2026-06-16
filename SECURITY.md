# Security Policy

## Reporting a vulnerability

Please report security issues privately via GitHub Security Advisories
(**Security → Report a vulnerability**) on this repository, rather than opening a
public issue. You'll get an acknowledgement as soon as possible.

## Design notes

- **Minimal privileged surface.** The app itself runs unprivileged. Everything
  that needs root goes through a single, small, auditable shell script
  (`packaging/wg-helper`), invoked with a fixed verb. The script:
  - exports a **fixed `PATH`**, so a hijacked caller environment can't redirect
    the commands it runs as root,
  - **validates tunnel/interface names** strictly (rejects path traversal),
  - writes configs **atomically** and keeps a **timestamped backup** before any
    overwrite or delete,
  - logs every privileged action to the journal (`logger -t wireguard-tui`).
- **Authorisation** is a passwordless **sudoers** drop-in scoped to just that one
  script, or a **polkit** rule for active local sessions; `pkexec` is the
  fallback.
- **Script directives.** Configs containing `PostUp`/`PreUp`/`PostDown`/`PreDown`
  (which `wg-quick` runs as root) are flagged in the UI before you save them.
- **Supply chain.** Third-party GitHub Actions are pinned to commit SHAs. Each
  release ships a `SHA256SUMS` file so artifacts can be verified.

## Verifying a download

```sh
sha256sum -c SHA256SUMS
```
