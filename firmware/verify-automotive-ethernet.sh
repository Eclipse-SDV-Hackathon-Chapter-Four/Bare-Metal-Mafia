#!/usr/bin/env bash
#
# Schnellcheck: S32K148EVB-Q176 Automotive-Ethernet-Link verifizieren (Linux).
#
# Prueft in einem Rutsch, ob die Verbindung Laptop <-> Medienkonverter <-> TJA1101
# <-> S32K148 funktioniert: IP-Konfiguration setzen, physischen Link pruefen, und
# den dokumentierten UDP-Echo-Server (Port 49444) sowie TCP-Echo-Server (Port 49555)
# der OpenBSW referenceApp direkt ansprechen (NICHT per Ping -- ICMP wird von der
# referenceApp evtl. nicht beantwortet, das ist kein Fehlerzeichen).
#
# Board muss bereits geflasht und mit Strom versorgt laufen (Serial-Konsole zeigt
# bei Erfolg: "link is UP by change" im Boot-Log).
#
# USAGE
#   1. Hardware verkabeln: USB-Ethernet-Adapter, Medienkonverter, TJA1101-Daughterboard
#      + S32K148 (bereits korrekt gejumpert/geflasht).
#      - Jumper auf TJA1101-Board entfernt (Master-Mode)
#      - Medienkonverter-DIP-Schalter 1 auf OFF (Slave-Mode)
#   2. sudo ./verify-automotive-ethernet.sh [--iface ethX] [--board-ip 192.168.0.200]
#      [--local-ip 192.168.0.1] [--prefix 24]
#
# Board-IP ist per Packet-Capture verifiziert: 192.168.0.200, MAC 10-11-22-77-77-77
# (siehe OpenBSW referenceApp Boot-Log, lwIP "netif: added interface" Eintraege).
#
# Benoetigt: python3 (fuer die UDP/TCP-Echo-Tests), iproute2 (ip), root/sudo.

set -uo pipefail

IFACE=""
BOARD_IP="192.168.0.200"
LOCAL_IP="192.168.0.1"
PREFIX="24"

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[0;33m'
CYAN='\033[0;36m'
NC='\033[0m'

step()  { echo -e "\n${CYAN}=== $1 ===${NC}"; }
pass()  { echo -e "${GREEN}[PASS]${NC} $1"; }
fail()  { echo -e "${RED}[FAIL]${NC} $1"; }
warn()  { echo -e "${YELLOW}[WARN]${NC} $1"; }

declare -A RESULTS

while [[ $# -gt 0 ]]; do
    case "$1" in
        --iface)    IFACE="$2"; shift 2 ;;
        --board-ip) BOARD_IP="$2"; shift 2 ;;
        --local-ip) LOCAL_IP="$2"; shift 2 ;;
        --prefix)   PREFIX="$2"; shift 2 ;;
        -h|--help)
            grep '^#' "$0" | sed 's/^# \{0,1\}//'
            exit 0
            ;;
        *) echo "Unbekannte Option: $1"; exit 1 ;;
    esac
done

# --- 0. Root-Check -----------------------------------------------------------
if [[ "$EUID" -ne 0 ]]; then
    fail "Dieses Skript braucht root/sudo (fuer die IP-Konfiguration). Bitte mit sudo erneut ausfuehren."
    exit 1
fi

if ! command -v python3 >/dev/null 2>&1; then
    fail "python3 wird fuer die UDP/TCP-Echo-Tests benoetigt, ist aber nicht installiert."
    exit 1
fi

# --- 1. Adapter finden ---------------------------------------------------------
step "Adapter-Erkennung"
if [[ -z "$IFACE" ]]; then
    # Erster aktiver (operstate=up) Link, der nicht lo/wlan/virtuell ist.
    for candidate in $(ip -o link show up | awk -F': ' '{print $2}'); do
        case "$candidate" in
            lo|wl*|docker*|veth*|br-*|virbr*|vmnet*|tun*|tap*) continue ;;
        esac
        IFACE="$candidate"
        break
    done
    if [[ -z "$IFACE" ]]; then
        fail "Kein passender aktiver Ethernet-Adapter gefunden. USB-Adapter eingesteckt? --iface manuell angeben."
        ip -o link show
        exit 1
    fi
else
    if ! ip link show "$IFACE" >/dev/null 2>&1; then
        fail "Interface '$IFACE' existiert nicht."
        exit 1
    fi
fi
echo "Verwende Adapter: $IFACE"

# --- 2. Physischer Link --------------------------------------------------------
step "Physischer Link"
OPERSTATE=$(cat "/sys/class/net/$IFACE/operstate" 2>/dev/null || echo "unknown")
if [[ "$OPERSTATE" == "up" ]]; then
    SPEED="unbekannt"
    DUPLEX="unbekannt"
    if command -v ethtool >/dev/null 2>&1; then
        SPEED=$(ethtool "$IFACE" 2>/dev/null | awk -F': ' '/Speed:/{print $2}')
        DUPLEX=$(ethtool "$IFACE" 2>/dev/null | awk -F': ' '/Duplex:/{print $2}')
    fi
    pass "Link up: Speed=$SPEED, Duplex=$DUPLEX"
    if [[ -n "$SPEED" && "$SPEED" != "100Mb/s" ]]; then
        warn "Automotive-Ethernet (100BASE-T1) sollte exakt 100Mb/s zeigen, nicht 1000Mb/s. Pruefe Master/Slave-Jumper."
    fi
    RESULTS["Link"]=1
else
    fail "Kein Link (operstate=$OPERSTATE). Kabel/Jumper/Medienkonverter-Stromversorgung pruefen."
    RESULTS["Link"]=0
fi

# --- 3. IP-Konfiguration -------------------------------------------------------
step "IP-Konfiguration ($LOCAL_IP/$PREFIX)"
if ip addr show dev "$IFACE" | grep -q "inet $LOCAL_IP/"; then
    echo "IP $LOCAL_IP bereits vorhanden."
else
    if ip addr add "$LOCAL_IP/$PREFIX" dev "$IFACE" 2>/tmp/ip_add_err; then
        echo "IP $LOCAL_IP/$PREFIX gesetzt."
    else
        fail "Konnte IP nicht setzen: $(cat /tmp/ip_add_err)"
        RESULTS["IP"]=0
    fi
fi
ip link set dev "$IFACE" up 2>/dev/null
pass "Lokale IP konfiguriert."
RESULTS["IP"]=1

# --- 4. UDP-Echo-Server (Port 49444) -------------------------------------------
step "UDP-Echo-Server Test ($BOARD_IP:49444)"
UDP_RESULT=$(python3 - "$LOCAL_IP" "$BOARD_IP" <<'PYEOF'
import socket, sys, time
local_ip, board_ip = sys.argv[1], sys.argv[2]
payload = f"hack-to-the-future-{int(time.time())}".encode()
try:
    s = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    s.bind((local_ip, 0))
    s.settimeout(3.0)
    s.sendto(payload, (board_ip, 49444))
    data, addr = s.recvfrom(256)
    print(f"OK|{addr[0]}|{data.decode(errors='replace')}")
except Exception as e:
    print(f"FAIL|{e}")
PYEOF
)
if [[ "$UDP_RESULT" == OK\|* ]]; then
    IFS='|' read -r _ remote resp <<< "$UDP_RESULT"
    pass "UDP-Echo von $remote erhalten: '$resp'"
    RESULTS["UDP"]=1
else
    fail "Keine UDP-Antwort: ${UDP_RESULT#FAIL|}"
    RESULTS["UDP"]=0
fi

# --- 5. TCP-Echo-Server (Port 49555) -------------------------------------------
step "TCP-Echo-Server Test ($BOARD_IP:49555)"
TCP_RESULT=$(python3 - "$LOCAL_IP" "$BOARD_IP" <<'PYEOF'
import socket, sys
local_ip, board_ip = sys.argv[1], sys.argv[2]
payload = b"hack-to-the-future-tcp"
try:
    s = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    s.bind((local_ip, 0))
    s.settimeout(3.0)
    s.connect((board_ip, 49555))
    s.sendall(payload)
    data = s.recv(256)
    print(f"OK|{data.decode(errors='replace')}")
except Exception as e:
    print(f"FAIL|{e}")
PYEOF
)
if [[ "$TCP_RESULT" == OK\|* ]]; then
    resp="${TCP_RESULT#OK|}"
    pass "TCP-Echo erhalten: '$resp'"
    RESULTS["TCP"]=1
else
    fail "Keine TCP-Antwort: ${TCP_RESULT#FAIL|}"
    RESULTS["TCP"]=0
fi

# --- Zusammenfassung ------------------------------------------------------------
step "Ergebnis"
ALL_OK=1
for key in "${!RESULTS[@]}"; do
    if [[ "${RESULTS[$key]}" == "1" ]]; then
        pass "$key"
    else
        fail "$key"
        ALL_OK=0
    fi
done

if [[ "$ALL_OK" == "1" ]]; then
    echo -e "\n${GREEN}ALLES GRUEN -- Automotive-Ethernet-Pfad funktioniert auf diesem Rechner einwandfrei.${NC}"
    exit 0
else
    echo -e "\n${RED}Mindestens ein Test fehlgeschlagen -- Details oben.${NC}"
    exit 1
fi
