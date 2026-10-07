# Bare-Metal-Mafia
Hack to the Future – Guardian Loop: portable child presence detection that moves from simulation to real hardware without changing the feature. Eclipse SDV Hackathon 2026.

## Quickstart: run the whole stack on a Raspberry Pi

<!-- This section (Pi quickstart) was largely AI-generated.
     Assisted-by: Anthropic Claude (Sonnet 5) -->

> Looking for AutoSD instead of Raspberry Pi OS + Docker Compose (the
> hackathon challenge brief calls this out explicitly)? See
> [`deploy/AUTOSD_ON_PI.md`](deploy/AUTOSD_ON_PI.md) - boots the real,
> official AutoSD image under KVM-accelerated QEMU directly on this same
> Pi, verified live (login, Podman, network all confirmed working); what's
> still open is running our actual stack inside it.

The Pi hosts the entire Guardian Loop stack (Guardian, dashboard, sensors,
actuation chain, the S32K148 DoIP bridge, and the AZ3166 ThreadX sensor
bridge) locally, so the whole team reaches the dashboard over the network
without depending on anyone's laptop. Details and rationale:
[`firmware/S32K148_HARDWARE_BRINGUP.md`](firmware/S32K148_HARDWARE_BRINGUP.md)
and [`az3166-sensor-bridge-firmware/`](az3166-sensor-bridge-firmware/).

### 1. Flash the SD card

- Tool: [Raspberry Pi Imager](https://www.raspberrypi.com/software/)
- Image: **Raspberry Pi OS Lite (64-bit)** — 64-bit is required (our
  container images are built on `debian:bookworm`), "Lite" because the Pi
  runs headless as a shared server.
- In the imager's advanced options (gear icon) before writing: set a
  hostname, **enable SSH**, and set Wi-Fi credentials if you won't use a
  wired LAN connection for normal network access.

> The Pi needs **two** separate network connections: the onboard
> Ethernet/Wi-Fi for normal LAN access (so the team can reach the
> dashboard), and a **separate USB Ethernet adapter** for the automotive
> Ethernet link to the S32K148. Set up the LAN/Wi-Fi side first (via the
> imager) — the setup script below only configures the automotive side.

### 2. Hardware checklist (before running the script)

**S32K148 (automotive Ethernet / DoIP):**
- [ ] TJA1101 daughterboard jumper removed (Master mode)
- [ ] Media converter DIP switch set to Slave mode
- [ ] USB-Ethernet adapter → media converter → TJA1101 → S32K148, plugged
      directly into the Pi
- [ ] S32K148 already flashed with the referenceApp + WindowPosition DID
      (`0xCF20`) — see
      [`firmware/S32K148_HARDWARE_BRINGUP.md`](firmware/S32K148_HARDWARE_BRINGUP.md)
      and [`firmware/0001-windowposition-did-0xCF20.patch`](firmware/0001-windowposition-did-0xCF20.patch)

**AZ3166 (Eclipse ThreadX sensor bridge, USB-serial):**
- [ ] AZ3166 already flashed with the ThreadX sensor bridge firmware —
      flash it from a laptop first (drag-and-drop
      `az3166-sensor-bridge-firmware/build/az3166-cortexm4/az3166_sensor_bridge.bin`
      onto the board's ST-Link mass-storage drive; see
      [`az3166-sensor-bridge-firmware/`](az3166-sensor-bridge-firmware/)).
      The Pi itself never builds or flashes the firmware, it only reads
      the board's UART once it's already running.
- [ ] AZ3166 plugged into the Pi via USB (the same cable carries both the
      ST-Link session and the UART data channel the bridge reads)

### 3. Run the setup script

SSH into the Pi, then:

```bash
curl -fsSL https://raw.githubusercontent.com/Eclipse-SDV-Hackathon-Chapter-Four/Bare-Metal-Mafia/AZ3166-ThreadX-Sensor/deploy/setup-raspi-guardian-node.sh | bash
```

This installs Docker, clones this repo, configures the automotive Ethernet
interface, adds the Pi user to the `dialout` group (for the AZ3166's USB
serial port), and builds + starts the full stack (first run compiles the
Rust workspace from scratch — expect it to take a while; later runs reuse
the Cargo cache and are fast).

Don't trust piping a script straight into `bash`? Fair — clone first and
read it, then run it locally:

```bash
git clone --branch AZ3166-ThreadX-Sensor https://github.com/Eclipse-SDV-Hackathon-Chapter-Four/Bare-Metal-Mafia.git
cd Bare-Metal-Mafia
less deploy/setup-raspi-guardian-node.sh   # read it
./deploy/setup-raspi-guardian-node.sh
```

Useful flags: `--local-ip <ip>` / `--prefix <n>` if your automotive Ethernet
subnet differs from the default `192.168.0.1/24`; `--no-s32k148` and/or
`--no-az3166` to skip either piece of real hardware (e.g. while it isn't
plugged in yet) and fall back to the simulated stack for that signal.
After the Pi user is added to `dialout`, log out/in (or reboot) once for
that to take effect without `sudo`.

### 4. Open the dashboard

The script prints the Pi's LAN IP at the end. From any device on the same
network:

```
http://<pi-ip>:8094
```

Check logs: `docker compose -f ~/Bare-Metal-Mafia/docker-compose.yml logs -f`
Stop everything: `docker compose -f ~/Bare-Metal-Mafia/docker-compose.yml down`
