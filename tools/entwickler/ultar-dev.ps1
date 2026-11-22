# Ultar OS – Entwickler-Zugang vom Windows-PC (PowerShell 5+, eingebautes OpenSSH).
# Damit steuert der PC – und Claude auf dem PC – ein Ultar-Gerät während der Entwicklung.
#
#   .\ultar-dev.ps1 einrichten -Adresse 192.168.1.50      Schlüssel + SSH-Eintrag "ultar" anlegen, Schlüssel anzeigen
#   .\ultar-dev.ps1 einrichten -Name ultar-qemu -Adresse 127.0.0.1   QEMU in WSL (tools/entwickler/qemu-dev.sh)
#   .\ultar-dev.ps1 info | log | bild | hilfe
#   .\ultar-dev.ps1 tippen 640 400 | text "hallo" | taste enter | oeffne settings
#   .\ultar-dev.ps1 hin <datei> [ziel] | her <pfad> [ziel]
#   .\ultar-dev.ps1 <beliebiger befehl auf dem Gerät>      z. B.  .\ultar-dev.ps1 ultar-ctl status
#   -Name <eintrag>  wählt das Gerät (Standard: $env:ULTAR_GERAET oder "ultar")
param(
    [Parameter(Position = 0)][string]$Befehl = "hilfe",
    [Parameter(Position = 1, ValueFromRemainingArguments = $true)][string[]]$Rest = @(),
    [string]$Name = $(if ($env:ULTAR_GERAET) { $env:ULTAR_GERAET } else { "ultar" }),
    [string]$Adresse = "",
    [int]$Port = 2222
)
$ErrorActionPreference = "Stop"
$SshDir = Join-Path $HOME ".ssh"
$Key = Join-Path $SshDir "ultar_dev_ed25519"
$Config = Join-Path $SshDir "config"

function Schreibe-OhneBom([string]$Pfad, [string]$Text) {
    [IO.File]::WriteAllText($Pfad, $Text, (New-Object Text.UTF8Encoding($false)))
}

function Einrichten {
    if (-not $Adresse) { throw "Adresse fehlt: -Adresse <IP des Geräts> (Einstellungen → System → Entwickleroptionen zeigt sie)" }
    New-Item -ItemType Directory -Force -Path $SshDir | Out-Null
    if (-not (Test-Path $Key)) {
        & ssh-keygen -q -t ed25519 -N '""' -C "ultar-dev@$env:COMPUTERNAME" -f $Key
        if (-not (Test-Path $Key)) { & ssh-keygen -q -t ed25519 -N "" -C "ultar-dev@$env:COMPUTERNAME" -f $Key }
    }
    $kh = (Join-Path $SshDir "ultar_known_hosts") -replace '\\', '/'
    $id = $Key -replace '\\', '/'
    $block = @"
# >>> ultar-dev $Name
Host $Name
    HostName $Adresse
    Port $Port
    User root
    IdentityFile "$id"
    IdentitiesOnly yes
    UserKnownHostsFile "$kh"
    StrictHostKeyChecking accept-new
    ServerAliveInterval 15
    ConnectTimeout 10
# <<< ultar-dev $Name
"@
    $alt = if (Test-Path $Config) { [IO.File]::ReadAllText($Config) } else { "" }
    $muster = "(?s)# >>> ultar-dev $([regex]::Escape($Name))\r?\n.*?# <<< ultar-dev $([regex]::Escape($Name))\r?\n?"
    $neu = [regex]::Replace($alt, $muster, "")
    if ($neu.Length -gt 0 -and -not $neu.EndsWith("`n")) { $neu += "`n" }
    # eigener Eintrag vorne: SSH nimmt den ersten passenden Wert
    Schreibe-OhneBom $Config ($block + "`n" + $neu)
    Write-Host "SSH-Eintrag '$Name' → ${Adresse}:$Port angelegt ($Config)."
    Write-Host ""
    Write-Host "Öffentlicher Schlüssel dieses PCs – auf dem Gerät erlauben (Einstellungen → System → Entwickleroptionen →"
    Write-Host "Entwickler-Zugang → einfügen) oder beim Bauen: ENTWICKLER=1 ENTWICKLER_SCHLUESSEL=<datei.pub>:"
    Write-Host ""
    Get-Content "$Key.pub"
    Write-Host ""
    Write-Host "Prüfen:  .\ultar-dev.ps1 -Name $Name info"
}

function Geraet([string[]]$a) {
    & ssh $Name @a
    exit $LASTEXITCODE
}

switch ($Befehl) {
    "einrichten" { Einrichten }
    "schluessel" { Get-Content "$Key.pub" }
    "log" { Geraet @("kernlog") }
    "bild" {
        $ziel = if ($Rest.Count -gt 0) { $Rest[0] } else { "ultar-bildschirm.png" }
        $r = & ssh $Name bildschirm
        if ($LASTEXITCODE -ne 0) { Write-Error "$r"; exit 1 }
        $pfad = ($r -split ' ')[0]
        & scp -q "${Name}:$pfad" $ziel
        if ($LASTEXITCODE -eq 0) { Write-Host "$ziel ($(($r -split ' ')[1]))" }
        exit $LASTEXITCODE
    }
    "hin" {
        if ($Rest.Count -lt 1) { throw "hin <datei> [ziel auf dem Gerät]" }
        $ziel = if ($Rest.Count -gt 1) { $Rest[1] } else { "/root/" }
        & scp -q $Rest[0] "${Name}:$ziel"; exit $LASTEXITCODE
    }
    "her" {
        if ($Rest.Count -lt 1) { throw "her <pfad auf dem Gerät> [ziel]" }
        $ziel = if ($Rest.Count -gt 1) { $Rest[1] } else { "." }
        & scp -q "${Name}:$($Rest[0])" $ziel; exit $LASTEXITCODE
    }
    "weiterleiten" {
        # Gerätedienst auf den PC holen, z. B. Sprachmodell: weiterleiten 8080  → http://127.0.0.1:8080
        $p = if ($Rest.Count -gt 0) { $Rest[0] } else { "8080" }
        Write-Host "127.0.0.1:$p → Gerät 127.0.0.1:$p (Strg+C beendet)"
        & ssh -N -L "${p}:127.0.0.1:$p" $Name; exit $LASTEXITCODE
    }
    "hilfe" {
        Get-Content $PSCommandPath | Select-Object -Skip 1 -First 10 | ForEach-Object { $_ -replace '^# ?', '' }
        Write-Host ""
        Write-Host "Auf dem Gerät eingebaut:"
        & ssh $Name ultar-hilfe 2>$null
    }
    default { Geraet (@($Befehl) + $Rest) }
}
