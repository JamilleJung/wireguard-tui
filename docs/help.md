# WireGuard, explained

A friendly guide to what WireGuard is, what every field means, and how to drive
wg-tui. You don't need to be a network engineer — read the first two sections and
you can set up a tunnel. (Press `?` for the key map; this is the *concepts*.)

---

## 1. The 60-second mental model

A **WireGuard tunnel** is an encrypted point-to-point link between two machines.
Think of it as a private cable plugged between you and a server: anything that
goes through the cable is encrypted and looks, to the rest of your network, like
it's coming from the machine at the other end.

Each tunnel has exactly two halves:

- **The Interface** = *you* (this machine): a private key, an address inside the
  tunnel, and optionally a DNS server to use while connected.
- **The Peer** = *the other machine* (usually your VPN server): a public key, a
  public address (the **endpoint**) to reach it at, and a list of which traffic
  should go through it (**AllowedIPs**).

When you **activate** a tunnel (`Enter` / `a`), the two halves do a quick
**handshake** (a cryptographic hello); once it succeeds, traffic flows. If the
handshake never completes, you're "active" but not actually connected — press
**`D`** (Diagnose) to find out why.

---

## 2. The keys (this is the whole security model)

WireGuard uses **public-key cryptography**. Every machine has a **key pair**:

- **Private key** — your secret. It *never* leaves this machine; you never share
  it. Generate one with `g`.
- **Public key** — derived from the private key. You *do* share this; it's how the
  server identifies you.

The rule is symmetric: **the server must know your public key, and you must know
the server's public key.** If the server doesn't have your public key in its peer
list, it silently ignores your handshakes (a very common cause of "won't
connect").

- **Preshared key (PSK)** — *optional* extra symmetric secret on top, for
  post-quantum belt-and-suspenders. If your config has one, the server must have
  the *same* PSK or the handshake fails.

> You usually don't create keys by hand: download a config from your provider /
> server (e.g. wg-easy) and import it (`i`) — the keys are already filled in and
> already registered on the server.

---

## 3. Every field, in plain words

**Interface (you):**

- **Name** — a label for the tunnel (e.g. `home`). Becomes the `wg0`-style name.
- **PrivateKey** — your secret (see above).
- **Address** — your IP *inside* the tunnel, e.g. `10.100.0.17/24` (assigned by
  the server; not your normal LAN IP).
- **DNS** — DNS server(s) to use while connected, e.g. `1.1.1.1`. Optional; needs a
  resolvconf provider.
- **ListenPort / MTU** — advanced, usually blank (auto). MTU sometimes needs
  lowering on flaky links.

**Peer (the server):**

- **PublicKey** — the server's public key.
- **PresharedKey** — optional shared secret (see above).
- **Endpoint** — where to reach the server: `host:port`, e.g.
  `82.26.104.2:51820`. The only address that goes over your *normal* internet.
- **AllowedIPs** — which destinations route *into* the tunnel:
  - `0.0.0.0/0` = **full tunnel** (all IPv4 traffic via the VPN).
  - `10.100.0.0/24` = **split tunnel** (only that subnet; the rest stays direct).
- **PersistentKeepalive** — a tiny packet every N seconds (commonly `25`) to hold
  the connection open through NAT/firewalls. Recommended behind a home router.

---

## 4. Driving wg-tui (press `?` for the full key map)

**Get a tunnel in:**

- `i` — **import** a `.conf` (or a QR image) via the file browser. The usual way.
- `n` — **new** tunnel from a preset (Interface only / Full / Split); generates a
  key pair, then opens `$EDITOR` to fill in the peer's public key + endpoint.

**Day to day:**

- Up/Down to select; the detail pane shows live handshake / transfer / speed.
- `Enter` / `a` — **activate / deactivate** (`wg-quick` up/down).
- `h`/`l` pick a detail field, `y` copies it.
- `s` — **start on boot** (systemd). `Q` — show the config as a **QR code**.

**Advanced (all keys work, no mode — see `?`):** `e` edit raw config, `g`
generate keys, `c` show running config, `p` save live state, `K` kill switch, `+`
add a peer, `R` rename, `x` export all.

**Other views:** `Tab` → **Log** (filter, copy, save). `B` → **Backup** manager.
`D` → **Diagnose**. `u` → check for / apply an **update**.

**When something's wrong:** press **`D`**. It checks, most-common-cause first: your
**system clock** (a wrong clock silently breaks handshakes — really), tunnel up,
handshake completing, endpoint reachable, DNS. It tells you which step failed.

---

## 5. Two gotchas worth knowing

- **"Active but nothing loads."** The interface is up but the handshake isn't
  completing, so no real traffic flows. Causes, most common first: the system
  clock is off; the server doesn't have your public key; the endpoint is
  unreachable. `D` pinpoints it.
- **Don't manage the same tunnel from two places** (e.g. NetworkManager + wg-tui).
  They'll fight. Pick one.

---

wg-tui never phones home, never lets your keys leave the machine, and only asks
for privilege to run `wg` / `wg-quick`. Your configs live in `/etc/wireguard`.
