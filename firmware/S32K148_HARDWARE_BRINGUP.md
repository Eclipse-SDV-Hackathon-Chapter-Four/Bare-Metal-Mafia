# S32K148 Hardware Bring-up — Summary

> **⚠️ KI-Hinweis / AI disclosure:** Dieses Dokument wurde von **Claude (Anthropic)**
> im Rahmen einer interaktiven Pair-Programming-Session erstellt — Claude hat die
> Recherche, Diagnose, Code-Änderungen und dieses Protokoll verfasst; physische
> Hardware-Handgriffe (Kabel stecken, Jumper umstellen, Treiber-Installation
> bestätigen) wurden vom menschlichen Teammitglied ausgeführt, von Claude
> angeleitet. Alle Fakten unten (Fehlercodes, Paket-Captures, Firmware-Diffs)
> sind reale Messergebnisse aus dieser Session, keine Erfindungen — aber wie bei
> jeder KI-Ausgabe: vor sicherheitsrelevanter Nutzung gegenprüfen.
>
> Jeder Abschnitt ist mit 🤖 (von Claude recherchiert/geschrieben/ausgeführt)
> oder 🧑 (von Menschen physisch ausgeführt, von Claude nur angeleitet)
> gekennzeichnet.

## 1. Ziel

Den NXP S32K148EVB-Q176 als reale OpenBSW-ECU in Betrieb nehmen: Toolchain
aufsetzen, Stock-`referenceApp` bauen und flashen, Automotive-Ethernet-Link
zum Laptop verifizieren, eine Guardian-Loop-relevante UDS-DID (`WindowPosition`)
ergänzen, und den Pfad SOVD→DoIP→UDS über das echte Eclipse OpenSOVD CDA
nachweisen — als Ersatz für `window_controller_sim`/`cda_sim` im bestehenden
Guardian-Loop-Stack (`guardian.rs` bleibt dabei unverändert).

## 2. Hardware-Inventar 🧑

| Teil | Rolle |
|---|---|
| NXP S32K148EVB-Q176 | Ziel-ECU, läuft OpenBSW |
| NXP ADTJA1101-RMII (TJA1101-PHY-Daughterboard) | 100BASE-T1 Automotive-Ethernet-Adapter fürs Board |
| Technica Engineering 100/1000BASE-T1 Medienkonverter | Automotive-Ethernet ↔ Standard-Ethernet |
| USB-Ethernet-Adapter (ASIX AX88179) | Laptop-seitiger Netzwerk-Port |

**Kritische Konfiguration (musste iterativ gefunden werden):**
- TJA1101-Board-Jumper **entfernt** = Master
- Medienkonverter-DIP-Schalter 1 **OFF** = Slave (Gegenstück zum Master — zwei
  gleiche Rollen auf der Leitung verhindern die Link-Synchronisation komplett)
- Nach Transport zwischen Laptops: Master/Slave-Einstellung unbedingt erneut
  prüfen, das verstellt sich leicht.

## 3. Toolchain-Setup 🤖 (angeleitet, Treiber-Installation 🧑 ausgeführt)

- **PEMicro OpenSDA-USB-Treiber** (offiziell: pemicro.com/opensda) — ohne den
  zeigt Windows Device-Manager-Fehlercode 28 ("Treiber nicht installiert") für
  die Debug-/Serial-Interfaces, auch wenn das Board selbst (Massenspeicher-Modus)
  bereits erkannt wird.
- **ARM GNU Toolchain 14.3.rel1** (`arm-none-eabi-gcc`) — nach WSL geladen.
- **P&E GDB-Server-Paket** (`com.pemicro.debug.gdbjtag.pne.updatesite`) —
  enthält `pegdbserver_console.exe` + `arm-none-eabi-gdb.exe`, musste aus einem
  Eclipse-p2-Update-Site-Jar entpackt werden; `supportFiles_ARM/` muss als
  **Geschwisterordner**, nicht Unterordner, neben der Exe liegen (sonst
  "Illegal Device Type"-Fehler).
- Build: `eclipse-openbsw/openbsw`, CMake-Preset `s32k148-freertos`
  (**kein Bazel**, trotz anderslautender älterer Docs-Fundstellen — aktuelle
  Doku nutzt CMake + Ninja).
- Flash/Debug: `pegdbserver_console -startserver -device=NXP_S32K1xx_S32K148F2M0M11`,
  dann `arm-none-eabi-gdb -batch -x flash.gdb <elf>` (Port 7224).

## 4. Automotive-Ethernet-Diagnose 🤖

Zusammenfassung einer längeren Fehlersuche (Details: Session-Verlauf), damit
niemand dieselbe Sackgasse nochmal durchläuft:

1. Physischer Link kam zunächst mit **1 Gbps** statt **100 Mbps** hoch →
   Master/Slave-Mismatch (s. Abschnitt 2).
2. Nach Korrektur: Link bei 100 Mbps, Board sendet nachweislich (Packet-Capture
   via `pktmon` bestätigte: Quelle `10-11-22-77-77-77`, IP `192.168.0.200`,
   SOME/IP-Multicast alle 1s) — aber **kein Unicast** (ARP/Ping/UDP/TCP) kam
   beim Laptop an.
3. Statischer ARP-Eintrag (MAC aus dem Capture fest eingetragen, ARP komplett
   umgangen) → **immer noch keine Antwort**. Das schloss ARP als Ursache aus.
4. Verdacht: Corporate-Security (Palo Alto GlobalProtect, auf dem Firmenlaptop
   aktiv) blockiert ausgehenden Traffic zu privaten Nicht-Firmen-Subnetzen,
   auch im "disconnected"-Zustand.
5. **Verifiziert auf einem zweiten, nicht-firmenverwalteten Laptop**: dort
   funktioniert derselbe Hardware-Aufbau — bestätigt, dass es sich um eine
   Policy des Firmenlaptops handelt, nicht um einen Hardware-Defekt.

→ Zwei Skripte (`verify-automotive-ethernet.ps1`/`.sh`, bereits auf Branch
`NXP-Test`) automatisieren genau diesen Check für neue Maschinen.

## 5. Firmware-Änderung: WindowPosition-DID 🤖

Neue UDS-DID `0xCF20` (lesbar via `0x22`, schreibbar via `0x2E`, 1 Byte,
0–100 %) in `eclipse-openbsw/openbsw`, nach exakt demselben Muster wie die
vorhandenen Demo-DIDs (`ReadIdentifierFromMemory`/`WriteIdentifierToMemory`,
generische Jobs aus `libs/bsw/uds/include/uds/jobs/`). Kein neuer Code nötig,
nur Registrierung — siehe Patch unten.

Gebaut (`cmake --preset s32k148-freertos`), geflasht, kein Compile-Fehler,
+112 Bytes Flash-Verbrauch.

**Patch:** [`0001-windowposition-did-0xCF20.patch`](0001-windowposition-did-0xCF20.patch)
— anwendbar auf einen frischen `eclipse-openbsw/openbsw`-Checkout mit
`git apply 0001-windowposition-did-0xCF20.patch`.

**Noch offen:** Die DID schreibt aktuell nur in einen RAM-Puffer (kein echter
Aktor dran) — Guardian-Loop-Beweis ("Schreiben kommt an") funktioniert, ein
sichtbarer physischer Effekt (LED/PWM/Relais) ist der nächste Ausbauschritt.

## 6. SOVD/DoIP-Anbindung (CDA) 🤖

`real-sovd-cda` aus `Eclipse-SDV-HackFest-Esslingen-2026/OpenBSW-Playground`
als echtes OpenSOVD-CDA vorgesehen (ersetzt den `cda_sim`-Stub im
Guardian-Stack). Muss auf der Maschine laufen, die physisch am
Automotive-Ethernet hängt (direkter L2/L3-Zugriff nötig, kein WSL/Container
ohne Adapter-Durchreichung).

→ Setup-Skript `setup-run-sovd-cda.sh` (bereits auf Branch `NXP-Test`)
automatisiert Klonen (inkl. Submodule), Rust-Toolchain-Install, Build, Start.

**Bereits ohne Zusatzarbeit nutzbar:** die mitgelieferte `OpenBSW.mdd`-Datenbank
kennt u. a. `ADC_Value` — das **echte Onboard-Potentiometer** des Boards, live
über DoIP/SOVD-REST abrufbar. Guter sofortiger Nachweis "echter Sensor im
Loop", ganz ohne die neue `WindowPosition`-DID anzufassen.

**Noch offen:** `WindowPosition` (`0xCF20`) ist der generierten MDD-Datenbank
noch nicht bekannt — dafür `odx-gen/openbsw_ecu.json` erweitern und
`generate_mdd.py` neu laufen lassen (nicht Teil dieser Session).

## 7. Stand / nächste Schritte

- [x] Board erkannt, geflasht, läuft stabil (Stock-`referenceApp` + WindowPosition-DID)
- [x] Automotive-Ethernet-Link verifiziert funktionsfähig (auf unkorrumpiertem Laptop)
- [x] Root Cause für Firmenlaptop-Problem identifiziert (Security-Policy, nicht Hardware)
- [ ] CDA real gegen das Board laufen lassen und per REST/Swagger verifizieren
- [ ] `WindowPosition`-DID in die CDA-Datenbank (MDD) aufnehmen
- [ ] `cda_sim` im `docker-compose.yml`-Stack durch das echte CDA ersetzen
      (Guardian-Code bleibt dabei unverändert — das ist der eigentliche Beweis)
- [ ] Sichtbarer physischer Aktor für `WindowPosition` (LED/PWM) statt reinem RAM-Puffer
- [ ] openDuT-Topologiewechsel verdrahten (Vagrantfile bringt vcan/can-gw-Grundlage schon mit)
