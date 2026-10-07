# Running the Guardian stack on AutoSD + Ankaios (plan, not yet implemented)

SPDX-License-Identifier: Apache-2.0 AND CC0-1.0

AI Disclosure: This file was largely AI-generated. The AI-generated
portions are made available under CC0-1.0 and not subject to the
project's licence. The human contributor has reviewed and verified
the accuracy of the content to the extent it could be verified without
access to real AutoSD hardware (see "Open risks" below for what could
not be verified this way).

Assisted-by: Anthropic Claude (Sonnet 5)

## Why

The Eclipse SDV Hackathon Chapter Four challenge description explicitly calls
out this combination as a scored criterion: *"Participants run the Guardian
on an AutoSD-based runtime, supervised by Ankaios, exchanging heartbeat,
fault, and mitigation events over uProtocol."* Our stack already does the
uProtocol/Zenoh part - this plan is about swapping the *runtime* (Raspberry
Pi OS + Docker Compose) for the *official* one (AutoSD + Ankaios + Podman).

This is **not a config tweak** - it's a different OS, a different container
runtime, and a different orchestrator. Treat this doc as a researched
starting point for whoever picks it up next, not a finished solution. Two
things in particular (networking between workloads, SELinux + hardware
passthrough) are flagged below as **genuinely unverified** - the Ankaios
docs don't cover them, and they can only be tested on a real AutoSD system,
not simulated here.

## What has to change, concretely

1. **OS on the Pi**: AutoSD instead of Raspberry Pi OS. Good news - a
   pre-built aarch64 image for Raspberry Pi 4 already exists, no custom
   `osbuild`/`mkosi` build needed.
2. **Container runtime**: Podman instead of Docker. Our `Containerfile`s are
   plain OCI builds, so `podman build` should produce an equivalent image -
   untested on this project's images specifically.
3. **Orchestrator**: Eclipse Ankaios (one `ank-server` + one `ank-agent` on
   the Pi) instead of `docker compose`. Each of the ~9 services in
   `docker-compose.yml` becomes one workload entry in an Ankaios
   `state.yaml`.
4. **Networking** (unverified - see "Open risks" below).
5. **Hardware passthrough** for the S32K148 (`network_mode: host`) and the
   AZ3166 (`devices: /dev/ttyACM0`) needs a Podman-flavored equivalent, and
   AutoSD's default SELinux-enforcing posture may block it where Debian's
   Raspberry Pi OS didn't (see "Open risks" below).

## Step 1 — Get AutoSD onto the Pi

Pre-built nightly images (developer variant, root login):

```bash
# On your own Linux machine (needs a real SD card reader)
wget https://autosd.sig.centos.org/AutoSD-10/nightly/sample-images/<pick-the-rpi4-developer-aarch64-image>.raw.xz
unxz <image>.raw.xz
sudo dd if=<image>.raw of=/dev/sdX status=progress bs=4M conv=fsync   # /dev/sdX = your SD card, NOT a partition
sync
```

Browse available builds at
<https://autosd.sig.centos.org/AutoSD-10/nightly/sample-images/> and pick an
`rpi4` + `developer` + `aarch64` image. Update the Pi 4's EEPROM first if
it's old - an outdated EEPROM is a known cause of boot failures on this
image. Default login is `root` / password `password` - **change this**
before leaving the Pi on a network others can reach.

## Step 2 — Install Podman + Ankaios

AutoSD ships Podman already (it's the whole point of the distro). If it's
somehow missing:

```bash
sudo dnf install -y podman   # or whatever AutoSD's package manager actually is - verify on the real system
```

Install Ankaios (server + agent, latest release):

```bash
curl -sfL https://github.com/eclipse-ankaios/ankaios/releases/latest/download/install.sh | bash -
sudo systemctl enable --now ank-server ank-agent
```

(The official docs mention an Ubuntu-24.04-specific AppArmor workaround for
Podman - irrelevant on AutoSD/SELinux, but if container start fails with a
permission-denied-looking error, that's the class of problem to suspect;
the SELinux equivalent isn't documented anywhere we found.)

## Step 3 — One workload, to prove the pattern

Don't try to port all 9 services at once. Get the simplest one
(`zenohd`, no custom build, just an upstream image) running first and
confirm you can actually reach it, *then* scale up.

`/etc/ankaios/state.yaml`:

```yaml
apiVersion: v0.1
workloads:
  zenohd:
    runtime: podman
    agent: agent_A
    restartPolicy: ALWAYS
    runtimeConfig: |
      image: docker.io/eclipse/zenoh:latest
      commandOptions: ["-p", "7447:7447", "--", "-l", "tcp/0.0.0.0:7447"]
```

Apply/start workloads with the `ank` CLI (installed alongside the
server/agent) - check `ank --help` on the real system; the docs we found
only clearly document `ank -k get state`, `ank -k run workload <name>` and
`ank -k delete workload <name>` for *adding* workloads dynamically, not a
single `ank apply state.yaml` the way `kubectl apply -f` works. Confirm
which one is actually right before assuming either.

Once `zenohd` is reachable (`podman ps`, then from another device:
`nc -zv <pi-ip> 7447`), build and add `guardian` and `dashboard` next (these
need a locally-built image from this repo's `Containerfile` via
`podman build`, not a pull from a registry - reference the resulting local
image name/tag in `runtimeConfig.image`). Only once that round-trip works
should the rest of the services get ported the same way.

## Open risks — genuinely unverified, need the real hardware

- **Inter-workload networking.** Nothing in the Ankaios docs says whether
  Podman workloads it starts share a network the way `docker compose`'s
  default bridge + DNS-by-service-name does. If they don't, every service's
  `ZENOH_CONNECT=tcp/zenohd:7447`-style env var (hostname-based, matching
  `docker-compose.yml`'s service-name DNS) will need to change to either a
  fixed IP, `host.containers.internal`-style Podman magic, or an explicit
  `podman network create` + `--network <name>` in every workload's
  `commandOptions`. Test this with the `zenohd` + one consumer pair before
  committing to a project-wide answer.
- **SELinux + hardware passthrough.** `network_mode: host` (S32K148 DoIP
  bridge) and `devices: ["/dev/ttyACM0:/dev/ttyACM0"]` (AZ3166 bridge) both
  worked on Debian-based Raspberry Pi OS because nothing there enforces
  SELinux. AutoSD, being Fedora/CentOS-Stream-based, defaults to SELinux
  enforcing. The Podman equivalents (`--network host`, `--device
  /dev/ttyACM0`) exist, but SELinux may block the actual I/O even once the
  device node is visible inside the container - watch
  `journalctl`/`ausearch` for `avc: denied` lines if a service that starts
  fine can't actually talk to its hardware. If so, the device node may need
  an SELinux context fix (`chcon`/a custom policy) that doesn't yet exist in
  this repo.
- **`podman build` vs `docker build` compatibility.** The existing
  `Containerfile`s use nothing exotic (no BuildKit-only syntax beyond
  standard cache mounts), so `podman build` should work, but no one has
  actually run it yet against this repo's files.

## What this doc deliberately does not include

- Ankaios manifests for the other 8 services - not written yet, intended as
  the next step once the `zenohd` round-trip above is confirmed working.
- A `deploy/setup-autosd-guardian-node.sh` equivalent of the existing
  `setup-raspi-guardian-node.sh` - premature until the open risks above are
  resolved; scripting around unknowns just produces a script that fails in
  a new way each time.
