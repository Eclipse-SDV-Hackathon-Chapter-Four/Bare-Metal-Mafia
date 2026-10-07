# Running the Guardian stack on AutoSD, on top of the Raspberry Pi

SPDX-License-Identifier: Apache-2.0 AND CC0-1.0

AI Disclosure: This file was largely AI-generated, including live
verification steps run interactively against the real Raspberry Pi 5
over SSH (boot test, login, `podman`/`systemctl`/network checks - see
"Verified, live" below for exactly what was and wasn't checked this
way). The human contributor directed and reviewed each step.

Assisted-by: Anthropic Claude (Sonnet 5)

## Why, and why this approach specifically

The Eclipse SDV Hackathon Chapter Four challenge brief calls out AutoSD
as a scored criterion. The project's own
[`autosd-starter-template`](https://github.com/Eclipse-SDV-Hackathon-Chapter-Four/autosd-starter-template)
repo was read in full before writing this: it targets either local QEMU
development or a shared remote device farm ("Jumpstarter", Renesas
R-Car S4 / TI J784S4EVM boards) - **Raspberry Pi does not appear in it at
all**, and neither does Eclipse Ankaios (an earlier version of this doc
assumed both; both assumptions were wrong - see git history if curious).

What this doc actually describes instead: running the **official** AutoSD
QEMU disk image, under **KVM-accelerated** `qemu-system-aarch64`, directly
on our own Raspberry Pi 5. This sidesteps waiting for Jumpstarter device
farm access (which needs a GitHub-org invite from the hackathon
organizers) while still running the real, unmodified AutoSD image the
challenge asks for - just as a nested VM on hardware we already have,
instead of on the organizers' shared boards.

## Verified, live (2026-10-07, against the real Pi)

Everything below was actually run over SSH against the Pi at
`192.168.88.243` (Raspberry Pi 5, 8GB, Debian 13 "trixie" / Raspberry Pi
OS, kernel `6.18.50+rpt-rpi-2712`), not simulated or assumed:

- `kvm-ok` → **"KVM acceleration can be used"**. `/dev/kvm` exists; the
  `pi` user needed adding to the `kvm` group (`sudo usermod -aG kvm pi`,
  takes effect on next login - no reboot needed).
- Downloaded and SHA-256-verified
  `auto-osbuild-qemu-autosd10-developer-regular-aarch64-2920323994.b34b5e04.qcow2.xz`
  (693MB) from
  `https://autosd.sig.centos.org/AutoSD-10/nightly/sample-images/` -
  **the aarch64 variant** (matches the Pi's own CPU architecture;
  there's also an x86_64 one in that directory, which would need much
  slower TCG software emulation instead of KVM on this hardware).
- Booted it with `qemu-system-aarch64 -M virt -cpu host -enable-kvm`
  plus AAVMF UEFI firmware (`apt install qemu-efi-aarch64`) for the
  CODE/VARS pflash drives - **reached a login prompt in well under 40
  seconds**: `Automotive Stream Distribution 10 / Kernel
  6.12.0-273.el10iv.aarch64`.
- Logged in as `root` / `password` (works as documented upstream).
  `podman --version` → `podman version 6.1.0`. `systemctl
  is-system-running` → `running` (healthy, nothing degraded). `ip -4 a`
  showed a working NAT'd network interface (`10.0.2.15/24`, QEMU's
  default user-mode networking).
- Clean `poweroff` worked.

One cosmetic warning appeared in the boot log every time
(`Initramfs unpacking failed: Decoding failed`) but never stopped the
boot from reaching a healthy login - looks like a secondary/non-critical
UKI boot stage, not investigated further since it isn't blocking.

The exact commands, so this is reproducible without re-deriving them:

```bash
# On the Pi itself (not this Windows laptop)
sudo apt-get install -y qemu-system-arm qemu-efi-aarch64 cpu-checker
sudo usermod -aG kvm "$USER"   # re-login (new SSH session is enough) for this to take effect
kvm-ok                          # should say "KVM acceleration can be used"

mkdir -p ~/autosd-qemu-test && cd ~/autosd-qemu-test
BASE=https://autosd.sig.centos.org/AutoSD-10/nightly/sample-images
FILE=auto-osbuild-qemu-autosd10-developer-regular-aarch64-2920323994.b34b5e04.qcow2.xz
# ^ nightly build ID changes daily - re-check the directory listing for
#   the current aarch64 "qemu" (not "rpi4") filename before reusing this.
wget "$BASE/$FILE" "$BASE/$FILE.sha256"
sha256sum -c "$FILE.sha256"
unxz -k "$FILE"
cp /usr/share/AAVMF/AAVMF_VARS.fd ./AAVMF_VARS.fd   # writable copy; AAVMF_CODE.fd stays read-only

qemu-system-aarch64 \
  -M virt -cpu host -enable-kvm -smp 2 -m 4096 \
  -drive if=pflash,format=raw,readonly=on,file=/usr/share/AAVMF/AAVMF_CODE.fd \
  -drive if=pflash,format=raw,file=./AAVMF_VARS.fd \
  -drive file=./auto-osbuild-qemu-autosd10-developer-regular-aarch64-2920323994.b34b5e04.qcow2,if=virtio,format=qcow2 \
  -nic user,model=virtio-net-pci \
  -nographic -monitor none
# login: root / password: password
# Ctrl+A then X to quit qemu (or `poweroff` inside the guest first)
```

To keep it running in the background after the SSH session ends, use
`systemd-run --user --scope -- qemu-system-aarch64 ...` rather than
`nohup ... &` - a plain backgrounded shell job turned out to silently
die when the SSH/PTY session closed during testing (worth knowing before
assuming a `nohup` job survived - it may not have; check `ps aux | grep
qemu-system` after reconnecting, don't just trust that the launch
command returned without an error).

## What's genuinely still open (not yet verified)

- **Running our actual Guardian stack inside this VM.** Only confirmed
  the base AutoSD image boots and has a healthy Podman/systemd - haven't
  yet built this project's `Containerfile`s with `podman build` inside
  the VM, or run the stack there. Memory looks fine on paper (4GB given
  to the VM, Guardian's services are small Rust binaries) but untested.
- **Networking from outside the Pi into the VM's workloads.** The
  `-nic user` NAT mode used above is the simplest to get booting, but it
  only lets the VM reach *out* - the team's dashboard (needs to be
  reachable at `http://<pi-ip>:8094` from other laptops on the network)
  would need `-nic tap` + a bridge, or explicit `hostfwd=` port forwards
  on the `-nic user` line, to be reachable from outside the Pi at all.
  Not yet set up or tested.
- **Hardware passthrough for S32K148 (automotive Ethernet) and AZ3166
  (USB-serial) into the VM.** Passing a USB device or a specific NIC
  *through* a VM boundary (`-usb -device usb-host,...` /
  `-netdev tap,ifname=...`) is a different, extra layer of indirection
  on top of what already worked passing them straight into a Podman
  *container* on bare Raspberry Pi OS (see the main
  [`setup-raspi-guardian-node.sh`](./setup-raspi-guardian-node.sh) path).
  Not attempted yet - this is the most likely place for new, Pi+VM-specific
  problems to show up that didn't exist in the non-VM deployment.
- **Orchestration inside the VM**: per the `autosd-starter-template` repo,
  the real mechanism is systemd + Podman quadlets (`.container` unit
  files under `/etc/containers/systemd/`), not `docker compose` and not
  Ankaios. None of this project's services have been converted to
  quadlet units yet.
- **Whether this actually satisfies the hackathon's AutoSD scoring
  criterion**, given it runs on self-owned hardware via a nested VM
  rather than the organizers' QEMU/Jumpstarter path. Worth confirming
  with the hackathon organizers (e.g. "lrossett" on the Eclipse SDV
  Slack) rather than assuming.

## Suggested next step, if someone picks this up

Don't try to port the whole stack at once. Inside the VM: `podman build`
this repo's `dashboard` or `zenohd`-equivalent image, run it manually
with `podman run`, confirm it starts and is reachable *from inside the
VM* first. Only once that round-trips should port-forwarding/bridging
(to reach it from outside the Pi) and the rest of the services get
tackled.

## How to add a "messenger" (alert on CRITICAL) service, running inside AutoSD

This section is for anyone who wants Guardian to *send a message*
(Slack webhook, MQTT, e-mail, a log line, whatever) when its state
reaches `CRITICAL` - without touching `guardian.rs` itself.

### The one rule that matters

`guardian.rs` already publishes its own state on every change. A
messenger is just **one more uProtocol subscriber** on that same topic -
exactly the same pattern `dashboard.rs` and `cda_sim.rs` already use.
Do **not** add notification code inside `guardian.rs` - that file is
deliberately kept ignorant of anything hardware- or
notification-specific (the project's "Golden Rule"). A new, separate
binary is the right shape for this.

### What Guardian already publishes

- **Topic**: `up/sdv/guardian/vss/Vehicle.Cabin.Guardian.State`
  (constant `TOPIC_GUARDIAN_STATE`, URI helper `vss_guardian_state_uri()`
  in `services/src/lib.rs`).
- **Payload** (JSON, one message per state change):
  ```json
  { "state": "CRITICAL", "child_present": true, "temperature_celsius": 29.4 }
  ```
  `state` is one of `CLEAR`, `MONITORING`, `WARNING`, `CRITICAL`,
  `MITIGATING` (the Rust type is `GuardianState`, the wrapping struct is
  `GuardianSnapshot` - both already defined in `services/src/lib.rs`,
  reuse them, don't redefine).

### Minimal implementation

Add a new file `services/src/bin/guardian_messenger.rs` (name it
whatever fits; this guide uses that name throughout):

```rust
//! guardian_messenger — sends an alert whenever Guardian's state
//! reaches CRITICAL. Subscribes to the existing vss_guardian_state_uri()
//! topic; does not touch guardian.rs.

use async_trait::async_trait;
use guardian_sil::{
    decode_json_payload, make_uri_provider, open_up_transport, vss_guardian_state_uri,
    GuardianSnapshot, GuardianState,
};
use tracing::{info, warn};
use up_rust::{UListener, UMessage, UTransport};

struct GuardianStateListener;

#[async_trait]
impl UListener for GuardianStateListener {
    async fn on_receive(&self, message: UMessage) {
        match decode_json_payload::<GuardianSnapshot>(&message) {
            Ok(snapshot) if snapshot.state == GuardianState::Critical => {
                // TODO: replace with a real Slack/MQTT/email call.
                info!(
                    "ALERT: Guardian is CRITICAL - child_present={} temperature={:.1}C",
                    snapshot.child_present, snapshot.temperature_celsius
                );
            }
            Ok(_) => {} // ignore non-critical states
            Err(err) => warn!("failed to decode GuardianSnapshot: {}", err),
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_target(false)
        .with_env_filter(
            std::env::var("RUST_LOG").unwrap_or_else(|_| "guardian_messenger=info,info".into()),
        )
        .init();

    let uri_provider = make_uri_provider("guardian-messenger", 0x9207, 0x01);
    let transport = open_up_transport(uri_provider).await?;

    transport
        .register_listener(&vss_guardian_state_uri(), None, std::sync::Arc::new(GuardianStateListener))
        .await?;

    info!("guardian_messenger listening on {:?}", guardian_sil::TOPIC_GUARDIAN_STATE);
    loop {
        tokio::time::sleep(std::time::Duration::from_secs(3600)).await;
    }
}
```

Notes on the entity ID `0x9207`: every binary that opens a uProtocol
transport needs its own unique entity ID in `make_uri_provider(...)`.
`0x9207` is free as of this writing - grep `services/src/lib.rs` and the
other `src/bin/*.rs` files for `make_uri_provider(` first to confirm
nothing else has claimed it since.

### Don't forget the `Containerfile` placeholder list

`Containerfile` fakes out every binary *except* the one being built with
a one-line `fn main() {}` stub, so a multi-binary cargo workspace build
doesn't have to compile binaries it doesn't need. **This list is
hardcoded** and has bitten this project before (`cargo build --bin X`
fails with "no bin target named X" if X is missing from it). Add one
line for the new binary, next to the existing ones:

```dockerfile
&& printf "fn main() {}\n" > services/src/bin/guardian_messenger.rs
```

### Build and run it inside the AutoSD VM

The existing VM-side stack (`zenohd`, `guardian`, `dashboard`) already
runs as plain `podman` containers built *from inside the VM* (see
"Suggested next step" above) - do the same for the messenger:

```bash
# Inside the AutoSD VM (ssh root@<pi-ip> -p 2222, or however you reach it)
cd ~/Bare-Metal-Mafia   # however the repo got onto the VM - git clone/pull
git pull

podman build -t guardian-messenger:dev \
  --build-arg BIN_NAME=guardian_messenger \
  -f Containerfile .

podman run -d --name guardian-messenger \
  --network guardian-net \
  -e ZENOH_CONNECT=tcp/zenohd:7447 \
  -e RUST_LOG=info \
  localhost/guardian-messenger:dev

podman logs -f guardian-messenger
```

`--network guardian-net` matters: it's the same user-defined Podman
network `zenohd`/`guardian`/`dashboard` already use so container names
resolve to each other (the default Podman bridge network does **not**
give you that - see the zenoh/Podman gotchas earlier in this doc). If
that network doesn't exist yet: `podman network create guardian-net`.

### Testing it without waiting for real hardware

The dashboard already has a "Set Child Present" / "Set Child Absent"
button (`POST /api/child-presence`) that publishes a real
`ChildPresenceEvent`. Combined with the real AZ3166 temperature already
feeding Guardian (or `temperature-sim` if that's running instead), you
can drive Guardian into `CRITICAL` on demand and watch
`guardian-messenger`'s log line fire:

```bash
curl -s -X POST http://<dashboard-host>:<port>/api/child-presence \
  -H 'Content-Type: application/json' -d '{"present":true}'

podman logs --tail 20 guardian-messenger
# should show: ALERT: Guardian is CRITICAL - child_present=true temperature=...
```
