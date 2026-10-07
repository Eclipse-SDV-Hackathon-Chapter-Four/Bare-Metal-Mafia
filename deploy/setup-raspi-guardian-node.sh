#!/usr/bin/env bash
#
# Turn a Raspberry Pi (64-bit Raspberry Pi OS / any Debian-based ARM64 Linux)
# into a shared, always-on Guardian Loop node: runs the FULL docker-compose
# stack (zenohd, guardian, sensors, actuation chain, dashboard) locally,
# plus the S32K148 DoIP bridge with real access to the automotive Ethernet
# hardware plugged directly into the Pi.
#
# Why a Pi instead of a laptop: this removes the whole "bridge runs on one
# laptop, Guardian runs on another, they have to find each other on the
# network" problem we had before. Everything lives on one small machine
# that can just stay on and plugged in - the team reaches the dashboard
# at http://<pi-ip>:8094 from any device on the same network, no laptop
# needs to be open. `network_mode: host` (used by the DoIP bridge service)
# only actually grants direct access to a physical adapter on real Linux -
# which a Raspberry Pi is, unlike Docker Desktop on Windows/macOS.
#
# What this script does:
#   1. Install Docker + the Compose plugin, if missing.
#   2. Clone (or update) this repository.
#   3. Detect the automotive Ethernet USB adapter and configure a static IP
#      on it (matches the convention used by verify-automotive-ethernet.sh:
#      192.168.0.1/24, board at 192.168.0.200).
#   4. Bring up the full stack, including the s32k148 profile (DoIP bridge).
#   5. Print the Pi's LAN IP and the dashboard URL for the rest of the team.
#
# USAGE
#   ./setup-raspi-guardian-node.sh [--local-ip 192.168.0.1] [--prefix 24] [--no-s32k148]
#
#   --local-ip     Static IP to assign on the automotive Ethernet interface.
#   --prefix       Subnet prefix length (default: 24).
#   --no-s32k148   Skip the S32K148 DoIP bridge profile (just run the base
#                  simulated stack, e.g. if the hardware isn't plugged in
#                  yet).
#
# PREREQUISITES (physical, before running this)
#   - TJA1101 daughterboard jumper removed (Master mode)
#   - Media converter DIP switch set to Slave mode
#   - USB-Ethernet adapter -> media converter -> TJA1101 -> S32K148, plugged
#     directly into this Pi
#   - S32K148 already flashed with the referenceApp + WindowPosition DID
#     (0xCF20) - see firmware/S32K148_HARDWARE_BRINGUP.md and
#     firmware/0001-windowposition-did-0xCF20.patch in this repo for how.

set -euo pipefail

REPO_URL="https://github.com/Eclipse-SDV-Hackathon-Chapter-Four/Bare-Metal-Mafia.git"
REPO_DIR="$HOME/Bare-Metal-Mafia"
LOCAL_IP="192.168.0.1"
PREFIX="24"
RUN_S32K148=1

RED='\033[0;31m'
GREEN='\033[0;32m'
CYAN='\033[0;36m'
YELLOW='\033[0;33m'
NC='\033[0m'

step() { echo -e "\n${CYAN}=== $1 ===${NC}"; }
ok()   { echo -e "${GREEN}[OK]${NC} $1"; }
warn() { echo -e "${YELLOW}[WARN]${NC} $1"; }
err()  { echo -e "${RED}[ERROR]${NC} $1"; }

while [[ $# -gt 0 ]]; do
    case "$1" in
        --local-ip)    LOCAL_IP="$2"; shift 2 ;;
        --prefix)      PREFIX="$2"; shift 2 ;;
        --no-s32k148)  RUN_S32K148=0; shift ;;
        -h|--help)
            grep '^#' "$0" | sed 's/^# \{0,1\}//'
            exit 0
            ;;
        *) err "Unknown option: $1"; exit 1 ;;
    esac
done

# --- 1. Docker + Compose plugin ------------------------------------------------
step "Docker"
if command -v docker >/dev/null 2>&1; then
    ok "Docker already installed: $(docker --version)"
else
    echo "Installing Docker via the official convenience script..."
    curl -fsSL https://get.docker.com | sh
    sudo usermod -aG docker "$USER"
    warn "Added $USER to the docker group - log out/in (or reboot) for this to take effect without sudo."
fi

if ! docker compose version >/dev/null 2>&1; then
    err "docker compose (plugin) not found even after install. Install docker-compose-plugin manually and re-run."
    exit 1
fi
ok "docker compose available: $(docker compose version --short 2>/dev/null || echo present)"

# --- 2. Clone or update the repo ------------------------------------------------
step "Repository"
if [[ -d "$REPO_DIR/.git" ]]; then
    echo "Already cloned at $REPO_DIR, pulling latest..."
    git -C "$REPO_DIR" fetch origin
    git -C "$REPO_DIR" checkout S32_Hardware_Implement
    git -C "$REPO_DIR" pull origin S32_Hardware_Implement
else
    git clone --branch S32_Hardware_Implement "$REPO_URL" "$REPO_DIR"
fi
ok "Repository ready at $REPO_DIR"

# --- 3. Configure the automotive Ethernet interface ----------------------------
step "Automotive Ethernet interface"
IFACE=""
for candidate in $(ip -o link show up | awk -F': ' '{print $2}'); do
    case "$candidate" in
        lo|wl*|docker*|veth*|br-*|eth0) continue ;;  # eth0 is usually the Pi's onboard/LAN port, not our USB adapter
    esac
    IFACE="$candidate"
    break
done

if [[ -z "$IFACE" ]]; then
    warn "Could not auto-detect the automotive Ethernet USB adapter (nothing plugged in yet?)."
    warn "Skipping IP configuration - plug it in and run:"
    warn "  sudo ip addr add $LOCAL_IP/$PREFIX dev <iface> && sudo ip link set <iface> up"
else
    echo "Using interface: $IFACE"
    if ip addr show dev "$IFACE" | grep -q "inet $LOCAL_IP/"; then
        echo "IP $LOCAL_IP already configured."
    else
        sudo ip addr add "$LOCAL_IP/$PREFIX" dev "$IFACE"
        echo "Set $LOCAL_IP/$PREFIX on $IFACE."
    fi
    sudo ip link set dev "$IFACE" up
    ok "Interface $IFACE is up with $LOCAL_IP/$PREFIX"
    echo "NOTE: this IP is NOT persistent across reboots yet. For a permanently"
    echo "      shared Pi, add a NetworkManager connection profile instead:"
    echo "        sudo nmcli con add type ethernet ifname $IFACE con-name automotive-eth \\"
    echo "          ip4 $LOCAL_IP/$PREFIX"
fi

# --- 4. Bring up the stack ------------------------------------------------------
step "Starting the Guardian stack"
cd "$REPO_DIR"
if [[ "$RUN_S32K148" -eq 1 ]]; then
    echo "Building and starting base stack + s32k148 (DoIP bridge) profile..."
    docker compose --profile s32k148 up -d --build
else
    echo "Building and starting base stack only (--no-s32k148 given)..."
    docker compose up -d --build zenohd artifact-server guardian dashboard \
        child-presence-sim temperature-sim actuation-adapter cda-sim window-controller-sim
fi
ok "Stack is up."

# --- 5. Print access info ---------------------------------------------------------
step "Access"
PI_IP=$(hostname -I 2>/dev/null | awk '{print $1}')
echo "Dashboard (share this with the team):"
echo "  http://${PI_IP:-<this-pi-ip>}:8094"
echo ""
echo "Other endpoints:"
echo "  Guardian state:         http://${PI_IP:-<pi-ip>}:8080/state"
echo "  Window controller:      http://${PI_IP:-<pi-ip>}:8092/state"
echo ""
echo "Check logs with:  docker compose -f $REPO_DIR/docker-compose.yml logs -f"
echo "Stop everything with:  docker compose -f $REPO_DIR/docker-compose.yml down"
