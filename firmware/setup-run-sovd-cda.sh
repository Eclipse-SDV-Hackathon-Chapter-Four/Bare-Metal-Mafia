#!/usr/bin/env bash
#
# Build and run the Eclipse OpenSOVD Classic Diagnostic Adapter (CDA) against
# a real S32K148EVB-Q176 running the OpenBSW referenceApp over DoIP.
#
# Run this on the machine that is PHYSICALLY connected to the automotive
# Ethernet hardware (USB-Ethernet adapter -> media converter -> TJA1101
# daughterboard -> S32K148). The CDA needs direct L2/L3 access to that
# network segment to send DoIP broadcasts, so it will NOT work from inside
# WSL or a container unless that environment has real access to the adapter.
#
# What this script does:
#   1. Clone Eclipse-SDV-HackFest-Esslingen-2026/OpenBSW-Playground (with the
#      classic-diagnostic-adapter + odx-converter git submodules).
#   2. Install the Rust toolchain via rustup, if not already present.
#   3. Build the CDA in release mode (cargo build --release).
#   4. Run it, pointing --tester-address at your local IP on the automotive
#      Ethernet interface (the one you configured with
#      verify-automotive-ethernet.sh, e.g. 192.168.0.1).
#
# After it starts, open http://localhost:8080 in a browser:
#   - POST /vehicle/v15/authorize   (any client_id/client_secret works, this
#     is a demo CDA; returns a Bearer token you need for the calls below)
#   - GET  /vehicle/v15/components/openbsw/data/ADC_Value
#     -> the board's REAL onboard potentiometer reading, live over DoIP.
#   - GET  /vehicle/v15/components/openbsw/data/EngineTemp, BatteryVoltage,
#     VehicleSpeed, StaticData -> other demo data identifiers that already
#     ship with the pre-generated OpenBSW.mdd database.
#
# NOTE: the pre-generated OpenBSW.mdd database does NOT yet know about the
# custom "WindowPosition" DID (0xCF20) added for the Guardian Loop demo.
# That needs odx-gen/openbsw_ecu.json extended and generate_mdd.py re-run
# before it shows up here — a separate follow-up step, not part of this
# script.
#
# USAGE
#   ./setup-run-sovd-cda.sh [--tester-address 192.168.0.1] [--rebuild]
#
#   --tester-address  Local IP on the automotive Ethernet interface.
#                      Defaults to 192.168.0.1 (matches the default used by
#                      verify-automotive-ethernet.sh). Override if your
#                      laptop uses a different local IP.
#   --rebuild          Force a clean rebuild even if a binary already exists.

set -euo pipefail

REPO_URL="https://github.com/Eclipse-SDV-HackFest-Esslingen-2026/OpenBSW-Playground.git"
REPO_DIR="$HOME/sdv/OpenBSW-Playground"
CDA_DIR="$REPO_DIR/OpenBSW-SOVD-Demo/real-sovd-cda"
TESTER_ADDRESS="192.168.0.1"
FORCE_REBUILD=0

RED='\033[0;31m'
GREEN='\033[0;32m'
CYAN='\033[0;36m'
NC='\033[0m'

step() { echo -e "\n${CYAN}=== $1 ===${NC}"; }
ok()   { echo -e "${GREEN}[OK]${NC} $1"; }
err()  { echo -e "${RED}[ERROR]${NC} $1"; }

while [[ $# -gt 0 ]]; do
    case "$1" in
        --tester-address) TESTER_ADDRESS="$2"; shift 2 ;;
        --rebuild)        FORCE_REBUILD=1; shift ;;
        -h|--help)
            grep '^#' "$0" | sed 's/^# \{0,1\}//'
            exit 0
            ;;
        *) err "Unknown option: $1"; exit 1 ;;
    esac
done

# --- 1. Clone the repository (with submodules) --------------------------------
step "Cloning OpenBSW-Playground (with submodules)"
mkdir -p "$(dirname "$REPO_DIR")"
if [[ -d "$REPO_DIR/.git" ]]; then
    echo "Repository already cloned at $REPO_DIR, skipping clone."
    echo "Making sure submodules are up to date..."
    git -C "$REPO_DIR" submodule update --init --recursive
else
    git clone --recurse-submodules "$REPO_URL" "$REPO_DIR"
fi
ok "Repository ready at $REPO_DIR"

if [[ ! -d "$CDA_DIR/classic-diagnostic-adapter" || -z "$(ls -A "$CDA_DIR/classic-diagnostic-adapter" 2>/dev/null)" ]]; then
    err "classic-diagnostic-adapter submodule looks empty. Try: git -C \"$REPO_DIR\" submodule update --init --recursive"
    exit 1
fi

# --- 2. Install Rust toolchain if missing --------------------------------------
step "Checking Rust toolchain"
if ! command -v cargo >/dev/null 2>&1; then
    echo "cargo not found, installing Rust via rustup (this downloads from rustup.rs)..."
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
    # shellcheck disable=SC1090
    source "$HOME/.cargo/env"
fi
ok "cargo available: $(cargo --version)"

# --- 3. Build the CDA in release mode ------------------------------------------
step "Building the OpenSOVD CDA (release mode)"
BIN_PATH="$CDA_DIR/classic-diagnostic-adapter/target/release/opensovd-cda"
cd "$CDA_DIR/classic-diagnostic-adapter"
if [[ -x "$BIN_PATH" && "$FORCE_REBUILD" -eq 0 ]]; then
    echo "Binary already built at $BIN_PATH, skipping build (use --rebuild to force)."
else
    echo "First build can take several minutes (full Rust dependency compile)..."
    cargo build --release
fi
if [[ ! -x "$BIN_PATH" ]]; then
    err "Build finished but binary not found at $BIN_PATH"
    exit 1
fi
ok "CDA binary ready: $BIN_PATH"

# --- 4. Run the CDA against the real board --------------------------------------
step "Starting CDA (tester-address=$TESTER_ADDRESS, DoIP broadcast on the local segment)"
echo "REST API will come up on http://localhost:8080"
echo "Try, in another terminal or a browser/REST client:"
echo "  curl -X POST http://localhost:8080/vehicle/v15/authorize -H 'Content-Type: application/json' -d '{\"client_id\":\"demo\",\"client_secret\":\"demo\"}'"
echo "  curl http://localhost:8080/vehicle/v15/components/openbsw/data/ADC_Value -H 'Authorization: Bearer <token-from-above>'"
echo ""
echo "Press Ctrl+C to stop."
echo ""

cd "$CDA_DIR"
exec "$BIN_PATH" \
    --tester-address "$TESTER_ADDRESS" \
    -d "$CDA_DIR/odx-gen" \
    --config "$CDA_DIR/opensovd-cda.toml"
