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
