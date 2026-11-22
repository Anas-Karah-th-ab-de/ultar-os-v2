#!/usr/bin/env bash
# Baut UltarFlash.exe (läuft unter Windows mit .NET Framework 4.x, das Windows 10/11 mitbringen).
# Unter Linux: apt install mono-mcs mono-devel libmono-system-windows-forms4.0-cil libmono-system-web-extensions4.0-cil libmono-system-io-compression4.0-cil libmono-system-io-compression-filesystem4.0-cil wixl ; unter Windows genügt csc.exe aus dem .NET Framework.
set -e; cd "$(dirname "$0")"
# Gegen die Referenz-Bibliotheken des .NET Framework 4.5 bauen (Paket mono-devel), NICHT gegen Monos eigene:
# sonst bindet der Compiler an Methoden, die es unter Windows nicht gibt (z. B. String.Split(char, StringSplitOptions)).
API=${NETFX_API:-/usr/lib/mono/4.5-api}
[ -f "$API/mscorlib.dll" ] || { echo "Referenz-Bibliotheken fehlen: apt install mono-devel"; exit 1; }
mcs -nologo -target:winexe -nostdlib -noconfig -lib:"$API" -r:mscorlib.dll -r:System.dll -r:System.Core.dll \
  -r:System.Windows.Forms.dll -r:System.Drawing.dll -r:System.Web.Extensions.dll -r:System.IO.Compression.dll -r:System.IO.Compression.FileSystem.dll -r:System.Management.dll \
  -out:UltarFlash.exe Core.cs Update.cs Package.cs Loke.cs Diag.cs Lang.cs Program.cs
ls -la UltarFlash.exe

# MSI-Installationspaket (wixl aus msitools)
VER=$(grep -o 'Version = "[0-9.]*"' Update.cs | cut -d'"' -f2)
wixl -D Version="$VER" -o "UltarFlash-$VER.msi" UltarFlash.wxs
ls -la "UltarFlash-$VER.msi"
