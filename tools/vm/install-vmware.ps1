# Ultar OS v2 in VMware Workstation on this PC: fetch the machine built by tools/vm/make-vmware.sh from the
# build computer (scp), unpack it to "%USERPROFILE%\Documents\Virtual Machines\UltarOS-v2" and start it.
#
#   powershell -ExecutionPolicy Bypass -File install-vmware.ps1
#   powershell -ExecutionPolicy Bypass -File install-vmware.ps1 -Disk sata        (disk on SATA/AHCI instead of NVMe)
#   powershell -ExecutionPolicy Bypass -File install-vmware.ps1 -Ps2Mouse         (no USB pointer: PS/2 mouse)
#   powershell -ExecutionPolicy Bypass -File install-vmware.ps1 -Zip C:\x\UltarOS-v2.zip   (a zip that is already here)
#   powershell -ExecutionPolicy Bypass -File install-vmware.ps1 -Installer -Zip C:\x\UltarOS-v2-installer.zip
#                                   (the installation machine: medium on SATA, empty NVMe target, no account)
#
# Nothing is downloaded from the internet: the only source is the build computer in the local network (or -Zip).
# A machine of the same name that is already there is stopped and replaced (its disk is deleted).
# The kernel's log of every start is written to serial.log beside the .vmx.
# This file is plain ASCII on purpose (Windows PowerShell 5.1 reads scripts without a byte order mark as ANSI).
param(
    [string]$Server = "root@192.168.100.18",
    [string]$RemoteZip = "/root/ultar-test/ultar-os-v2/out/vmware/UltarOS-v2.zip",
    [string]$Zip = "",
    [ValidateSet("nvme", "sata")][string]$Disk = "nvme",
    [switch]$Ps2Mouse,
    [switch]$Installer,
    [switch]$NoStart
)
$ErrorActionPreference = "Stop"
$name = "UltarOS-v2"
if ($Installer) {
    $name = "UltarOS-v2-installer"
    $RemoteZip = $RemoteZip -replace "UltarOS-v2\.zip$", "UltarOS-v2-installer.zip"
    if ($Disk -ne "nvme") { throw "-Installer: the target stays on NVMe (the medium is the SATA disk)" }
}
$base = Join-Path $env:USERPROFILE "Documents\Virtual Machines"
$target = Join-Path $base $name
$vmx = Join-Path $target "$name.vmx"
$localZip = Join-Path $target "$name.zip"

$vmrun = "C:\Program Files\VMware\VMware Workstation\vmrun.exe"
if (-not (Test-Path $vmrun)) { $vmrun = "C:\Program Files (x86)\VMware\VMware Workstation\vmrun.exe" }
if (-not (Test-Path $vmrun)) { throw "vmrun.exe not found: is VMware Workstation installed?" }

# 1) stop and remove an earlier copy of this machine
if (Test-Path $vmx) {
    $running = & $vmrun -T ws list
    if ($running -contains $vmx) {
        Write-Host "stopping the running machine ..."
        & $vmrun -T ws stop $vmx hard | Out-Null
    }
}
if (Test-Path $target) {
    # only this machine's own folder below "Virtual Machines" is removed
    if (-not $target.StartsWith($base + "\")) { throw "refusing to remove $target" }
    Remove-Item -LiteralPath $target -Recurse -Force
}
New-Item -ItemType Directory -Force -Path $target | Out-Null

# 2) fetch the zip (and its checksum) from the build computer, or take the given one
if ($Zip -ne "") {
    Copy-Item -LiteralPath $Zip -Destination $localZip
} else {
    $scp = (Get-Command scp.exe -ErrorAction SilentlyContinue).Source
    if (-not $scp) { throw "scp.exe not found (Windows feature 'OpenSSH Client')" }
    Write-Host "fetching $RemoteZip from $Server ..."
    & $scp -o BatchMode=yes "${Server}:$RemoteZip" $localZip
    if ($LASTEXITCODE -ne 0) { throw "scp of the zip failed (exit $LASTEXITCODE)" }
    & $scp -o BatchMode=yes "${Server}:$RemoteZip.sha256" "$localZip.sha256"
    if ($LASTEXITCODE -ne 0) { throw "scp of the checksum failed (exit $LASTEXITCODE)" }
    $want = ((Get-Content -LiteralPath "$localZip.sha256" -TotalCount 1) -split "\s+")[0].ToLower()
    $have = (Get-FileHash -LiteralPath $localZip -Algorithm SHA256).Hash.ToLower()
    if ($want -ne $have) { throw "checksum of the zip differs: expected $want, got $have" }
    Write-Host "checksum ok ($have)"
    Remove-Item -LiteralPath "$localZip.sha256" -Force
}

# 3) unpack: the zip holds the folder $name with the .vmx and the .vmdk (installer: medium + target)
Expand-Archive -LiteralPath $localZip -DestinationPath $base -Force
Remove-Item -LiteralPath $localZip -Force
if (-not (Test-Path $vmx)) { throw "the zip does not hold $name\$name.vmx" }
$diskFile = if ($Installer) { "UltarOS-v2-target.vmdk" } else { "$name.vmdk" }
if (-not (Test-Path (Join-Path $target $diskFile))) { throw "the zip does not hold $name\$diskFile" }

# 4) options: the .vmx is UTF-8 without a byte order mark and must stay so
if ($Disk -eq "sata" -or $Ps2Mouse) {
    $utf8 = New-Object System.Text.UTF8Encoding($false)
    $text = [System.IO.File]::ReadAllText($vmx, $utf8)
    if ($Disk -eq "sata") {
        $text = $text.Replace('nvme0.present = "TRUE"', 'nvme0.present = "FALSE"')
        $text = $text.Replace('nvme0:0.present = "TRUE"', 'sata0:0.present = "TRUE"')
        $text = $text.Replace('nvme0:0.fileName', 'sata0:0.fileName')
        $text = $text.Replace('sata0.present = "FALSE"', 'sata0.present = "TRUE"')
    }
    if ($Ps2Mouse) {
        $text = $text.Replace('usb_xhci:4.present = "TRUE"', 'usb_xhci:4.present = "FALSE"')
    }
    [System.IO.File]::WriteAllText($vmx, $text, $utf8)
}

# 5) start
Write-Host "machine: $vmx (disk on $Disk)"
if (-not $NoStart) {
    & $vmrun -T ws start $vmx
    if ($LASTEXITCODE -ne 0) { throw "vmrun start failed (exit $LASTEXITCODE)" }
    Write-Host "started. Kernel log: $(Join-Path $target 'serial.log')"
}
