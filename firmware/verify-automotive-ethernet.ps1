<#
.SYNOPSIS
  Schnellcheck: S32K148EVB-Q176 Automotive-Ethernet-Link verifizieren.

.DESCRIPTION
  Prueft in einem Rutsch, ob die Verbindung Laptop <-> Medienkonverter <-> TJA1101 <-> S32K148
  funktioniert: IP-Konfiguration setzen, physischen Link pruefen, und den dokumentierten
  UDP-Echo-Server (Port 49444) sowie TCP-Echo-Server (Port 49555) der OpenBSW referenceApp
  direkt ansprechen (NICHT per Ping -- ICMP wird von der referenceApp evtl. nicht beantwortet,
  das ist kein Fehlerzeichen).

  Board muss bereits geflasht und per OpenSDA-USB oder eigenstaendig mit Strom versorgt
  laufen (TeraTerm o.ae. zeigt bei Erfolg: "link is UP by change" im Boot-Log).

.USAGE
  1. Dieses Skript auf den Test-Laptop kopieren (USB-Stick/Mail).
  2. USB-Ethernet-Adapter + Medienkonverter + TJA1101-Daughterboard + S32K148 wie gewohnt verkabeln.
     - Jumper auf TJA1101-Board entfernt (Master-Mode)
     - Medienkonverter-DIP-Schalter 1 auf OFF (Slave-Mode)
  3. PowerShell ALS ADMINISTRATOR oeffnen (fuer die IP-Konfiguration notwendig).
  4. .\verify-automotive-ethernet.ps1 ausfuehren.
     Optional: -AdapterName "Ethernet 11" wenn die Auto-Erkennung den falschen Adapter waehlt.

.NOTES
  Board-IP ist per Packet-Capture verifiziert: 192.168.0.200, MAC 10-11-22-77-77-77
  (siehe OpenBSW referenceApp Boot-Log, lwIP "netif: added interface" Eintraege).
#>

param(
    [string]$AdapterName = "",
    [string]$BoardIP = "192.168.0.200",
    [string]$LocalIP = "192.168.0.1",
    [int]$PrefixLength = 24
)

$ErrorActionPreference = "Stop"
$results = @{}

function Write-Step($msg) { Write-Host "`n=== $msg ===" -ForegroundColor Cyan }
function Write-Pass($msg) { Write-Host "[PASS] $msg" -ForegroundColor Green }
function Write-Fail($msg) { Write-Host "[FAIL] $msg" -ForegroundColor Red }

# --- 0. Admin-Check ---------------------------------------------------------
$isAdmin = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
if (-not $isAdmin) {
    Write-Fail "Dieses Skript braucht Administrator-Rechte (fuer die IP-Konfiguration). Bitte PowerShell als Admin neu starten."
    exit 1
}

# --- 1. Adapter finden -------------------------------------------------------
Write-Step "Adapter-Erkennung"
if ($AdapterName -eq "") {
    $candidates = Get-NetAdapter | Where-Object {
        $_.Status -eq "Up" -and
        $_.InterfaceDescription -notmatch "Wi-Fi|Virtual|Bluetooth|Hyper-V|VirtualBox|Loopback"
    }
    if ($candidates.Count -eq 0) {
        Write-Fail "Kein passender aktiver Ethernet-Adapter gefunden. USB-Adapter eingesteckt? -AdapterName manuell angeben."
        Get-NetAdapter | Select-Object Name, InterfaceDescription, Status | Format-Table -AutoSize
        exit 1
    }
    if ($candidates.Count -gt 1) {
        Write-Host "Mehrere Kandidaten gefunden, nehme den ersten. Falls falsch: -AdapterName angeben." -ForegroundColor Yellow
        $candidates | Select-Object Name, InterfaceDescription | Format-Table -AutoSize
    }
    $adapter = $candidates[0]
} else {
    $adapter = Get-NetAdapter -Name $AdapterName
}
Write-Host "Verwende Adapter: $($adapter.Name) ($($adapter.InterfaceDescription))"

# --- 2. Physischer Link ------------------------------------------------------
Write-Step "Physischer Link"
$adapter = Get-NetAdapter -Name $adapter.Name
if ($adapter.MediaConnectionState -eq "Connected") {
    Write-Pass "Link up: $($adapter.LinkSpeed), FullDuplex=$($adapter.FullDuplex)"
    if ($adapter.LinkSpeed -notmatch "100 Mbps") {
        Write-Host "  Hinweis: Automotive-Ethernet (100BASE-T1) sollte exakt 100 Mbps zeigen, nicht 1 Gbps. Pruefe Master/Slave-Jumper." -ForegroundColor Yellow
    }
    $results["Link"] = $true
} else {
    Write-Fail "Kein Link. Kabel/Jumper/Medienkonverter-Stromversorgung pruefen."
    $results["Link"] = $false
}

# --- 3. IP-Konfiguration ------------------------------------------------------
Write-Step "IP-Konfiguration ($LocalIP/$PrefixLength)"
$existing = Get-NetIPAddress -InterfaceIndex $adapter.ifIndex -AddressFamily IPv4 -ErrorAction SilentlyContinue |
            Where-Object { $_.IPAddress -eq $LocalIP }
if (-not $existing) {
    New-NetIPAddress -InterfaceIndex $adapter.ifIndex -IPAddress $LocalIP -PrefixLength $PrefixLength | Out-Null
    Write-Host "IP $LocalIP/$PrefixLength gesetzt."
} else {
    Write-Host "IP $LocalIP bereits vorhanden."
}
Write-Pass "Lokale IP konfiguriert."
$results["IP"] = $true

# --- 4. UDP-Echo-Server (Port 49444) -----------------------------------------
Write-Step "UDP-Echo-Server Test ($BoardIP`:49444)"
try {
    $localEp = New-Object System.Net.IPEndPoint ([System.Net.IPAddress]::Parse($LocalIP)), 0
    $udp = New-Object System.Net.Sockets.UdpClient
    $udp.Client.Bind($localEp)
    $udp.Client.ReceiveTimeout = 3000
    $target = New-Object System.Net.IPEndPoint ([System.Net.IPAddress]::Parse($BoardIP)), 49444
    $payload = "hack-to-the-future-$(Get-Date -Format HHmmss)"
    $msg = [System.Text.Encoding]::ASCII.GetBytes($payload)
    $udp.Send($msg, $msg.Length, $target) | Out-Null

    $remoteEp = New-Object System.Net.IPEndPoint ([System.Net.IPAddress]::Any), 0
    $resp = $udp.Receive([ref]$remoteEp)
    $respText = [System.Text.Encoding]::ASCII.GetString($resp)
    $udp.Close()

    if ($respText -eq $payload) {
        Write-Pass "UDP-Echo von $remoteEp erhalten: '$respText'"
        $results["UDP"] = $true
    } else {
        Write-Host "[WARN] Antwort erhalten, aber Inhalt weicht ab: '$respText'" -ForegroundColor Yellow
        $results["UDP"] = $true
    }
} catch {
    Write-Fail "Keine UDP-Antwort: $($_.Exception.Message)"
    $results["UDP"] = $false
}

# --- 5. TCP-Echo-Server (Port 49555) -----------------------------------------
Write-Step "TCP-Echo-Server Test ($BoardIP`:49555)"
try {
    $tcp = New-Object System.Net.Sockets.TcpClient
    $tcp.Client.Bind((New-Object System.Net.IPEndPoint ([System.Net.IPAddress]::Parse($LocalIP)), 0))
    $connectTask = $tcp.ConnectAsync($BoardIP, 49555)
    if (-not $connectTask.Wait(3000)) {
        throw "Connect-Timeout nach 3s"
    }
    $stream = $tcp.GetStream()
    $payload = "hack-to-the-future-tcp"
    $bytes = [System.Text.Encoding]::ASCII.GetBytes($payload)
    $stream.Write($bytes, 0, $bytes.Length)
    $stream.ReadTimeout = 3000
    $buf = New-Object byte[] 256
    $n = $stream.Read($buf, 0, $buf.Length)
    $respText = [System.Text.Encoding]::ASCII.GetString($buf, 0, $n)
    $tcp.Close()

    Write-Pass "TCP-Echo erhalten: '$respText'"
    $results["TCP"] = $true
} catch {
    Write-Fail "Keine TCP-Antwort: $($_.Exception.Message)"
    $results["TCP"] = $false
}

# --- Zusammenfassung ----------------------------------------------------------
Write-Step "Ergebnis"
$results.GetEnumerator() | ForEach-Object {
    if ($_.Value) { Write-Pass $_.Key } else { Write-Fail $_.Key }
}
if (($results.Values | Where-Object { -not $_ }).Count -eq 0) {
    Write-Host "`nALLES GRUEN -- Automotive-Ethernet-Pfad funktioniert auf diesem Laptop einwandfrei." -ForegroundColor Green
} else {
    Write-Host "`nMindestens ein Test fehlgeschlagen -- Details oben." -ForegroundColor Red
}
