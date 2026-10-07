# Bare-Metal-Mafia
Hack to the Future – Guardian Loop: portable child presence detection that moves from simulation to real hardware without changing the feature. Eclipse SDV Hackathon 2026.

## Branch status: concluded

This branch was the scratch space for the first S32K148EVB-Q176 automotive
Ethernet bring-up: getting a laptop to actually reach the board over
100BASE-T1 (`verify-automotive-ethernet.ps1`/`.sh`), a first pass at the
OpenSOVD CDA (`setup-run-sovd-cda.sh`), and a teammate's own working notes
on pinging the board via `nmcli` (`NOTES.md`).

**All of it is still here and still correct.** The work continued on
**[`S32_Hardware_Implement`](../../tree/S32_Hardware_Implement)**, which
contains everything from this branch plus: the actual `WindowPosition` UDS
DID added to the firmware (with patch), the DoIP bridge that reads it onto
the Guardian dashboard, the Raspberry Pi deployment script, and the full
bring-up writeup (`firmware/S32K148_HARDWARE_BRINGUP.md`).

Use `S32_Hardware_Implement` (or whatever branch follows it) going
forward. This branch is kept for history/reference only — **deliberately
not merged into `main`**, since the end-to-end DoIP integration it leads
to hasn't been verified on real hardware yet (see the open items listed in
`S32_Hardware_Implement`'s bring-up doc).
