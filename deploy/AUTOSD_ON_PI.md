# Running the Guardian stack on AutoSD, on top of the Raspberry Pi

SPDX-License-Identifier: Apache-2.0 AND CC0-1.0

AI Disclosure: This file was largely AI-generated, including live
verification steps run interactively against the real Raspberry Pi 5
over SSH (boot test, login, disk resize, container builds/runs - see
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

**Confirm with the hackathon organizers** (e.g. "lrossett" on the
Eclipse SDV Slack) whether this self-devised path - official image, own
hardware, nested VM, instead of the organizers' QEMU/Jumpstarter flow -
actually satisfies the AutoSD scoring criterion. Not yet confirmed.

## Status: Guardian runs, end to end, as a real Podman container inside AutoSD

Verified live against the real Pi (`192.168.88.243`, Raspberry Pi 5, 8GB,
Debian 13 "trixie" host / Raspberry Pi OS, kernel
`6.18.50+rpt-rpi-2712`), on 2026-10-07:

```
curl http://<pi-ip>:8080/state
{"state":"CLEAR","child_present":false,"temperature_celsius":26.0}
```

That response came from `guardian` actually running as a `podman`
container *inside* the AutoSD VM, reached through:
Windows laptop → Pi's LAN IP:8080 → qemu `hostfwd` → VM's port 8080 →
`podman -p 8080:8080` → Guardian's own Axum HTTP server. Every hop of
that chain was hit and confirmed working, not assumed.

Three real problems had to be found and fixed to get there - each is
documented below because each will resurface if this is ever redone
from scratch (new nightly image, different Pi, etc.):

### 1. The VM's disk was too small for a full Rust build (fixed: resize it)

The downloaded AutoSD image ships with a 7.5GB root partition. A
`podman build` of this repo's `Containerfile` needs more than that -
the first attempt compiled successfully (`zenoh`, `up-rust`,
`guardian-sil`, all of it, in ~18 minutes) and then failed at the very
last step, `cp`'ing the ~15MB final binary out, with "No space left on
device". `podman system df` only showed 1.5GB in images; the real
consumer was `--mount=type=cache` build-cache volumes (cargo registry +
the full `target/debug` directory, several GB with `debuginfo=2` and
this many dependencies) that `system df` doesn't account for. A
`podman system prune -a -f` reclaimed ~12GB once (stale/duplicated
image layers), which was enough for exactly one more `cp` - but a
second full build from a cold cache hit the same wall again at ~75%
disk use, confirming the prune was a one-time band-aid, not a fix.

The actual fix: grow the virtual disk, from the Pi host (VM must be
stopped - the qcow2 file can't be resized while qemu holds it open),
then grow the partition and filesystem from inside the guest:

```bash
# On the Pi, VM stopped (pkill -f qemu-system-aarch64 first)
qemu-img resize ~/autosd-qemu-test/auto-osbuild-qemu-autosd10-*.qcow2 +16G

# Boot the VM again (see launch command further below), then inside the guest:
dnf install -y cloud-utils-growpart e2fsprogs   # growpart/resize2fs aren't on the base image
growpart /dev/vda 5          # vda5 is the root partition on this image - check lsblk if that ever changes
resize2fs /dev/vda5
df -hT /                     # should now show ~24G total, ~17G available
```

### 2. Guardian crashed instantly: AutoSD boots with IPv6 disabled

`podman logs guardian` showed it exiting immediately with:

```
Unable to open listener tcp/[::]:0: Can not create a new TCP listener ...
[Os { code: 97, kind: Uncategorized, message: "Address family not supported by protocol" }]
Error: Failed to open Zenoh session
```

AutoSD boots with `ipv6.disable=1` on the kernel command line (`cat
/proc/cmdline` on the guest). Zenoh's default "peer" mode always opens
its own listener for peer-to-peer gossip, including an IPv6 wildcard
one - which can't even create an `AF_INET6` socket when the kernel has
IPv6 disabled outright, regardless of container config.

The fix, in `services/src/lib.rs`'s `open_up_transport()` and
`open_zenoh_session()`: force `mode: client` in the Zenoh config.
Every service in this project already talks through a `zenohd` router
via `ZENOH_CONNECT`, never directly peer-to-peer, so client mode is the
*correct* configuration here, not a workaround - it only ever connects
out, so there's nothing left to fail on an IPv6-disabled kernel. This
also likely explains some odd "Unable to connect to any locator of
scouted peer" warnings noticed earlier in this project's Windows/WSL
testing (peer-mode gossip trying to reach ephemeral one-shot processes
directly) - not re-verified, but consistent with the same root cause.

### 3. Podman's default network does not do container-name DNS

With the IPv6 crash fixed, `guardian` ran but immediately failed again,
differently:

```
WARN Unable to connect to tcp/zenohd:7447! failed to lookup address
information: Name or service not known
```

This directly answers the open question this doc used to carry:
**plain `podman run` containers on Podman's default bridge do not get
by-name DNS resolution between each other** - same as plain `docker
run` without Compose. `docker-compose`/`podman-compose` get this "for
free" because they always create a project-scoped user-defined network;
doing it by hand with bare `podman run` needs that step done
explicitly:

```bash
podman network create guardian-net
podman run -d --name zenohd --network guardian-net -p 7447:7447 \
  docker.io/eclipse/zenoh:latest -l tcp/0.0.0.0:7447
podman run -d --name guardian --network guardian-net -p 8080:8080 \
  -e ZENOH_CONNECT=tcp/zenohd:7447 -e RUST_LOG=info \
  localhost/guardian:dev
```

Once both containers are on `guardian-net`, `guardian` resolves
`zenohd` by name immediately - no further workaround needed. (Also
easy to miss: `guardian`'s own `-p 8080:8080` has to be given
explicitly too, same as `zenohd`'s `-p 7447:7447` - a container on a
custom network is reachable *from other containers on that network* by
name automatically, but still needs its own explicit port publish to
be reachable from outside Podman entirely, e.g. through the VM's
`hostfwd`.)

## Reproducing this from scratch

```bash
# --- On the Pi ---
sudo apt-get install -y qemu-system-arm qemu-efi-aarch64 cpu-checker
sudo usermod -aG kvm "$USER"   # new SSH session picks this up, no reboot needed
kvm-ok                          # confirm: "KVM acceleration can be used"

mkdir -p ~/autosd-qemu-test && cd ~/autosd-qemu-test
BASE=https://autosd.sig.centos.org/AutoSD-10/nightly/sample-images
FILE=auto-osbuild-qemu-autosd10-developer-regular-aarch64-2920323994.b34b5e04.qcow2.xz
# ^ nightly build ID changes daily - re-check the directory listing for the
#   current aarch64 "qemu" (not "rpi4"/"rpi5" - no such image exists) filename.
wget "$BASE/$FILE" "$BASE/$FILE.sha256"
sha256sum -c "$FILE.sha256"
unxz -k "$FILE"
qemu-img resize "${FILE%.xz}" +16G    # see "problem 1" above - do this before first boot
cp /usr/share/AAVMF/AAVMF_VARS.fd ./AAVMF_VARS.fd

sudo loginctl enable-linger "$USER"   # keeps the VM alive after this SSH session ends
systemd-run --user --unit=autosd-vm -- qemu-system-aarch64 \
  -M virt -cpu host -enable-kvm -smp 2 -m 4096 \
  -drive if=pflash,format=raw,readonly=on,file=/usr/share/AAVMF/AAVMF_CODE.fd \
  -drive if=pflash,format=raw,file=./AAVMF_VARS.fd \
  -drive file="${FILE%.xz}",if=virtio,format=qcow2 \
  -nic user,model=virtio-net-pci,hostfwd=tcp::2222-:22,hostfwd=tcp::8080-:8080,hostfwd=tcp::8094-:8094 \
  -nographic -monitor none -serial file:./boot.log
# NOTE: plain `systemd-run --user --scope -- ...` (no --unit, with --scope)
# blocks synchronously attached to the invoking SSH session and can look
# like it hung/errored even though the VM boots fine independently thanks
# to `loginctl enable-linger`. Using `--unit` without `--scope` (as above)
# avoids that confusion. Either way, verify independently with
# `ssh -p 2222 root@<pi-ip>` rather than trusting how the launch command
# itself returned.

# After ~30-40s, SSH straight into the guest (much easier than the serial
# console for anything beyond the first boot check):
ssh -p 2222 root@<pi-ip>   # root / password

# --- Inside the guest ---
dnf install -y cloud-utils-growpart e2fsprogs
growpart /dev/vda 5 && resize2fs /dev/vda5    # see "problem 1"

git clone --branch AZ3166-ThreadX-Sensor \
  https://github.com/Eclipse-SDV-Hackathon-Chapter-Four/Bare-Metal-Mafia.git
cd Bare-Metal-Mafia
podman build -t localhost/guardian:dev --build-arg BIN_NAME=guardian -f Containerfile .

podman network create guardian-net
podman run -d --name zenohd --network guardian-net -p 7447:7447 \
  docker.io/eclipse/zenoh:latest -l tcp/0.0.0.0:7447
podman run -d --name guardian --network guardian-net -p 8080:8080 \
  -e ZENOH_CONNECT=tcp/zenohd:7447 -e RUST_LOG=info \
  localhost/guardian:dev

curl http://localhost:8080/state   # from inside the guest
# from the Pi, or any other machine on the LAN:
curl http://<pi-ip>:8080/state
```

## What's genuinely still open

- **Only `guardian` + `zenohd` have been proven, not the rest of the
  stack.** `dashboard`, `child-presence-sim`, `temperature-sim`, the
  actuation chain, the S32K148/AZ3166 bridges - none of these have been
  built or run inside the VM yet. Each is a `podman build` +
  `podman run --network guardian-net ...` away, following the exact
  pattern above; do them one at a time, not all at once.
- **Hardware passthrough for S32K148 (automotive Ethernet) and AZ3166
  (USB-serial) *into the VM*.** This is a different, extra layer versus
  passing them straight into a Podman container on bare Raspberry Pi OS
  (the non-VM path, see [`setup-raspi-guardian-node.sh`](./setup-raspi-guardian-node.sh)):
  now the device/NIC has to cross the VM boundary too
  (`-usb -device usb-host,...` / `-netdev tap,ifname=...` on the qemu
  command line) before Podman inside the guest can see it at all. Not
  attempted - likely the next real wall to hit.
- **Orchestration via systemd + Podman quadlets.** Per the
  `autosd-starter-template` repo, that's the intended mechanism on
  AutoSD (`.container` unit files under `/etc/containers/systemd/`),
  not bare `podman run` commands kept alive by hand. Everything above
  uses bare `podman run` to prove the pattern fastest; converting to
  quadlets (so the stack survives a VM reboot on its own) is unstarted.
- **Dashboard/public ports beyond 8080.** Only `8080` (Guardian) and
  `7447` (Zenoh, needed for the `podman run` commands above) have
  `hostfwd` entries so far. The dashboard's `8094` needs the same
  treatment once it's actually running in the VM.
- **The AutoSD-scoring-criterion question from "Why" above** - still
  not confirmed with the organizers.
