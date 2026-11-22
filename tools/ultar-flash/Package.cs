// Ultar Flash – baut aus dem Original-Boot-Abbild des Geräts ein Ultar-Paket.
// Der Kernel des Herstellers (mit allen Treibern) bleibt, nur das Start-Ramdisk wird durch Ultar OS ersetzt.
// Quellen für das Original: Samsung-Firmware (AP_….tar.md5), eine boot.img oder der Sicherungsordner.
using System;
using System.Collections.Generic;
using System.IO;
using System.IO.Compression;
using System.Linq;
using System.Security.Cryptography;
using System.Text;

namespace UltarFlash
{
    public static class Lz4
    {
        /// LZ4-Frame entpacken (Format der *.img.lz4-Dateien in Samsung-Firmware).
        public static byte[] Decode(byte[] src)
        {
            if (src.Length < 7 || BitConverter.ToUInt32(src, 0) != 0x184D2204) throw new Exception("Kein LZ4-Frame.");
            int flg = src[4], p = 6;
            if ((flg & 0x08) != 0) p += 8;   // Inhaltsgröße
            if ((flg & 0x01) != 0) p += 4;   // Wörterbuch-ID
            p += 1;                          // Kopf-Prüfsumme
            bool blockSum = (flg & 0x10) != 0;
            // Die ganze Ausgabe bleibt im Speicher, daher funktionieren auch voneinander abhängige Blöcke
            byte[] outBuf = new byte[0]; int outLen = 0;
            Action<int> need = n => { if (outLen + n > outBuf.Length) Array.Resize(ref outBuf, Math.Max(outBuf.Length * 2, outLen + n + (1 << 20))); };
            while (p + 4 <= src.Length)
            {
                uint bs = BitConverter.ToUInt32(src, p); p += 4;
                if (bs == 0) break;
                int len = (int)(bs & 0x7FFFFFFF);
                if (p + len > src.Length) throw new Exception("LZ4: Datei abgeschnitten.");
                if ((bs & 0x80000000) != 0) { need(len); Buffer.BlockCopy(src, p, outBuf, outLen, len); outLen += len; }
                else
                {
                    int s = p, end = p + len;
                    while (s < end)
                    {
                        int token = src[s++];
                        int lit = token >> 4;
                        if (lit == 15) { int b; do { b = src[s++]; lit += b; } while (b == 255); }
                        need(lit); Buffer.BlockCopy(src, s, outBuf, outLen, lit); outLen += lit; s += lit;
                        if (s >= end) break; // letzter Abschnitt hat nur Literale
                        int off = src[s] | (src[s + 1] << 8); s += 2;
                        int ml = token & 15;
                        if (ml == 15) { int b; do { b = src[s++]; ml += b; } while (b == 255); }
                        ml += 4;
                        if (off == 0 || off > outLen) throw new Exception("LZ4: ungültiger Verweis.");
                        need(ml);
                        for (int i = 0; i < ml; i++) { outBuf[outLen] = outBuf[outLen - off]; outLen++; } // überlappend erlaubt
                    }
                }
                p += len;
                if (blockSum) p += 4;
            }
            Array.Resize(ref outBuf, outLen);
            return outBuf;
        }
    }

    public static class Tar
    {
        /// Sucht Einträge in einem tar-Archiv (auch *.tar.md5) und liefert Name -> Inhalt für die gewünschten Namen.
        public static Dictionary<string, byte[]> Extract(string file, Func<string, bool> want)
        {
            var res = new Dictionary<string, byte[]>();
            using (var f = File.OpenRead(file))
            {
                var h = new byte[512];
                while (f.Read(h, 0, 512) == 512)
                {
                    if (h[0] == 0) break;
                    string name = Encoding.ASCII.GetString(h, 0, 100).TrimEnd('\0');
                    string so = Encoding.ASCII.GetString(h, 124, 12).Trim('\0', ' ');
                    long size = so.Length == 0 ? 0 : Convert.ToInt64(so, 8);
                    if (want(name)) { var b = new byte[size]; int got = 0; while (got < size) { int n = f.Read(b, got, (int)size - got); if (n <= 0) throw new Exception("tar abgeschnitten"); got += n; } res[name] = b; f.Seek((512 - size % 512) % 512, SeekOrigin.Current); }
                    else f.Seek(size + (512 - size % 512) % 512, SeekOrigin.Current);
                }
            }
            return res;
        }

        /// Schreibt ein tar-Archiv (für Odin: AP-Datei mit boot.img und vbmeta.img).
        public static void Write(string file, Dictionary<string, string> entries)
        {
            using (var o = File.Create(file))
            {
                foreach (var kv in entries)
                {
                    var data = File.ReadAllBytes(kv.Value);
                    var h = new byte[512];
                    Encoding.ASCII.GetBytes(kv.Key).CopyTo(h, 0);
                    Encoding.ASCII.GetBytes("0000644\0").CopyTo(h, 100); Encoding.ASCII.GetBytes("0000000\0").CopyTo(h, 108); Encoding.ASCII.GetBytes("0000000\0").CopyTo(h, 116);
                    Encoding.ASCII.GetBytes(Convert.ToString(data.Length, 8).PadLeft(11, '0') + "\0").CopyTo(h, 124);
                    Encoding.ASCII.GetBytes("00000000000\0").CopyTo(h, 136);
                    for (int i = 148; i < 156; i++) h[i] = 32;
                    h[156] = (byte)'0'; Encoding.ASCII.GetBytes("ustar\0" + "00").CopyTo(h, 257);
                    int sum = h.Sum(b => (int)b);
                    Encoding.ASCII.GetBytes(Convert.ToString(sum, 8).PadLeft(6, '0') + "\0 ").CopyTo(h, 148);
                    o.Write(h, 0, 512); o.Write(data, 0, data.Length);
                    var pad = new byte[(512 - data.Length % 512) % 512]; o.Write(pad, 0, pad.Length);
                }
                o.Write(new byte[1024], 0, 1024);
            }
        }
    }

    public class BootImage
    {
        public byte[] Header, Kernel, Ramdisk, Second, RecoveryDtbo, Dtb;
        public int Page, Version;
        public uint RamdiskAddr; // gewünschte Ladeadresse, falls der Kopf 0 enthält (aus dem Recovery-Abbild übernommen)
        public uint HeaderRamdiskAddr { get { return U32(Header, 20); } }

        static uint U32(byte[] b, int o) { return BitConverter.ToUInt32(b, o); }
        static void W32(byte[] b, int o, uint v) { BitConverter.GetBytes(v).CopyTo(b, o); }
        static int Pad(int n, int page) { return (n + page - 1) / page * page; }

        public static BootImage Parse(byte[] img)
        {
            if (img.Length < 2048 || Encoding.ASCII.GetString(img, 0, 8) != "ANDROID!") throw new Exception("Kein Android-Boot-Abbild (Kennung ANDROID! fehlt).");
            var b = new BootImage { Page = (int)U32(img, 36), Version = (int)U32(img, 40) };
            if (b.Page < 2048 || b.Page > 65536 || (b.Page & (b.Page - 1)) != 0) throw new Exception("Boot-Abbild: ungültige Seitengröße " + b.Page);
            if (b.Version > 2) throw new Exception("Boot-Abbild Version " + b.Version + " wird nicht unterstützt (nur 0–2).");
            int ks = (int)U32(img, 8), rs = (int)U32(img, 16), ss = (int)U32(img, 24);
            int rds = b.Version >= 1 ? (int)U32(img, 1632) : 0, ds = b.Version >= 2 ? (int)U32(img, 1648) : 0;
            b.Header = img.Take(b.Page).ToArray();
            int off = b.Page;
            Func<int, byte[]> take = n => { if (off + n > img.Length) throw new Exception("Boot-Abbild abgeschnitten."); var r = new byte[n]; Buffer.BlockCopy(img, off, r, 0, n); off += Pad(n, b.Page); return r; };
            b.Kernel = take(ks); b.Ramdisk = take(rs); b.Second = take(ss); b.RecoveryDtbo = take(rds); b.Dtb = take(ds);
            if (ks == 0) throw new Exception("Boot-Abbild enthält keinen Kernel.");
            return b;
        }

        public byte[] Build()
        {
            var h = (byte[])Header.Clone();
            W32(h, 8, (uint)Kernel.Length); W32(h, 16, (uint)Ramdisk.Length); W32(h, 24, (uint)Second.Length);
            // Abbilder ohne Ramdisk tragen oft die Ladeadresse 0. Mit Ramdisk würde der Bootloader es dann nach Adresse 0
            // laden und das Gerät sofort neu starten. Übliche Lage: 16 MiB über der Kernel-Basis (wie im Recovery-Abbild).
            if (Ramdisk.Length > 0 && U32(h, 20) == 0) W32(h, 20, RamdiskAddr != 0 ? RamdiskAddr : (U32(h, 12) & 0xFFF00000u) + 0x01000000u);
            int off = Page + Pad(Kernel.Length, Page) + Pad(Ramdisk.Length, Page) + Pad(Second.Length, Page);
            if (Version >= 1) { W32(h, 1632, (uint)RecoveryDtbo.Length); BitConverter.GetBytes((ulong)(RecoveryDtbo.Length > 0 ? off : 0)).CopyTo(h, 1636); }
            if (Version >= 2) W32(h, 1648, (uint)Dtb.Length);
            // Kennung (SHA-1 über die Teile samt Längen) wie mkbootimg neu berechnen
            using (var sha = SHA1.Create())
            {
                var ms = new MemoryStream();
                Action<byte[]> add = p => { ms.Write(p, 0, p.Length); ms.Write(BitConverter.GetBytes((uint)p.Length), 0, 4); };
                add(Kernel); add(Ramdisk); add(Second);
                if (Version >= 1) add(RecoveryDtbo);
                if (Version >= 2) add(Dtb);
                var id = sha.ComputeHash(ms.ToArray());
                Array.Clear(h, 576, 32); id.CopyTo(h, 576);
            }
            var o = new MemoryStream();
            Action<byte[]> put = p => { o.Write(p, 0, p.Length); var pad = new byte[Pad(p.Length, Page) - p.Length]; o.Write(pad, 0, pad.Length); };
            o.Write(h, 0, h.Length); put(Kernel); put(Ramdisk); put(Second); put(RecoveryDtbo); put(Dtb);
            var tail = Encoding.ASCII.GetBytes("SEANDROIDENFORCE"); // Samsung erwartet diese Endmarke
            o.Write(tail, 0, tail.Length);
            return o.ToArray();
        }
    }

    public static class PackageBuilder
    {
        static int Find(byte[] hay, byte[] needle)
        {
            for (int i = 0; i <= hay.Length - needle.Length; i++)
            {
                int j = 0; while (j < needle.Length && hay[i + j] == needle[j]) j++;
                if (j == needle.Length) return i;
            }
            return -1;
        }

        /// Ältere Geräte (System-als-Root) übergehen das Ramdisk, wenn der Bootloader "skip_initramfs" übergibt.
        /// Wie bei Magisk: das Wort im Kernel umbenennen, damit unser Ramdisk gestartet wird.
        public static byte[] PatchKernel(byte[] k, Action<string> log)
        {
            byte[] a = Encoding.ASCII.GetBytes("skip_initramfs"), b = Encoding.ASCII.GetBytes("want_initramfs");
            bool gz = k.Length > 2 && k[0] == 0x1f && k[1] == 0x8b;
            byte[] raw = k;
            if (gz)
            {
                // gzip-Kernel (evtl. mit angehängtem Gerätebaum): entpacken, ändern, neu packen
                var ms = new MemoryStream(k);
                using (var z = new GZipStream(ms, CompressionMode.Decompress, true)) { var o = new MemoryStream(); z.CopyTo(o); raw = o.ToArray(); }
            }
            int n = 0, i;
            while ((i = Find(raw, a)) >= 0) { b.CopyTo(raw, i); n++; }
            if (n == 0) { log("Kernel enthält kein \"skip_initramfs\" – keine Änderung am Kernel nötig" + (k.Length > 4 && k[0] == 0x02 && k[1] == 0x21 ? " (oder LZ4-gepackter Kernel: wird nicht unterstützt)" : "") + "."); return k; }
            log("Kernel: \"skip_initramfs\" an " + n + " Stelle(n) umbenannt, damit das Ultar-Ramdisk gestartet wird.");
            if (!gz) return raw;
            log("Hinweis: gzip-gepackter Kernel wurde neu gepackt; ein angehängter Gerätebaum ginge dabei verloren.");
            var outp = new MemoryStream();
            using (var z = new GZipStream(outp, CompressionMode.Compress, true)) z.Write(raw, 0, raw.Length);
            return outp.ToArray();
        }

        /// vbmeta: Prüfung der Partitionen abschalten (Flags = 3), sonst lehnt der Bootloader das geänderte Boot-Abbild ab.
        public static byte[] PatchVbmeta(byte[] v)
        {
            if (v.Length < 256 || Encoding.ASCII.GetString(v, 0, 4) != "AVB0") throw new Exception("vbmeta.img hat nicht das erwartete Format (AVB0).");
            // Ein leeres, unsigniertes vbmeta mit „Prüfung aus“ (Flags = 2), wie es auch Magisk für Samsung erzeugt.
            // Nur die Flags im signierten Original zu ändern genügt nicht: die Signatur passt dann nicht mehr und der
            // Samsung-Bootloader startet das Gerät in einer Schleife neu (so beim Galaxy A30 beobachtet).
            var o = new byte[256];
            Encoding.ASCII.GetBytes("AVB0").CopyTo(o, 0);
            o[7] = 1;      // Version 1.0
            o[123] = 2;    // AVB_VBMETA_IMAGE_FLAGS_VERIFICATION_DISABLED
            Encoding.ASCII.GetBytes("avbtool 1.1.0").CopyTo(o, 128);
            return o;
        }

        static byte[] Unpack(string name, byte[] data) { return name.EndsWith(".lz4") ? Lz4.Decode(data) : data; }

        /// Baut das Paket. source: AP_….tar(.md5), boot.img(.lz4) oder Sicherungsordner mit partitions/boot.img.
        /// Die unveränderten Originale werden als geprüfte Wiederherstellungs-Sicherung nach backupDir gelegt.
        /// true, wenn das Paket mit einem anderen Ultar-Kernsystem gebaut wurde als dem vorliegenden.
        public static bool Stale(string pkgDir, string initramfs)
        {
            if (initramfs == null || !File.Exists(initramfs)) return false;
            var f = Path.Combine(pkgDir, "ramdisk.sha256");
            return !File.Exists(f) || File.ReadAllText(f).Trim() != Core.Sha256(initramfs);
        }

        /// Paket neu bauen, wenn ein neueres Kernsystem vorliegt und das Original-Boot-Abbild in der Sicherung liegt.
        public static void Refresh(string pkgDir, string initramfs, string backupDir, DeviceInfo dev, Action<string> log)
        {
            if (!File.Exists(Path.Combine(pkgDir, "ultar-package.txt")) || !Stale(pkgDir, initramfs)) return;
            if (dev == null || dev.Mode != "adb" || string.IsNullOrEmpty(dev.Codename)) return; // nur mit erkanntem Handy neu bauen
            if (!File.Exists(Path.Combine(backupDir, "partitions", "boot.img")) || (File.ReadAllText(Path.Combine(pkgDir, "ultar-package.txt")).Contains("start=recovery") && !File.Exists(Path.Combine(backupDir, "partitions", "recovery.img")))) { log("Hinweis: Es liegt ein neueres Ultar-Kernsystem vor. Bitte „Ultar-Paket bauen …“ erneut ausführen."); return; }
            log("Neues Ultar-Kernsystem gefunden – baue das Paket neu …");
            Build(backupDir, initramfs, pkgDir, backupDir, dev, log);
        }

        public static string Build(string source, string initramfs, string outDir, string backupDir, DeviceInfo dev, Action<string> log)
        {
            byte[] boot = null, vbmeta = null, recovery = null;
            if (Directory.Exists(source))
            {
                var bp = Path.Combine(source, "partitions", "boot.img"); var vp = Path.Combine(source, "partitions", "vbmeta.img");
                if (!File.Exists(bp)) throw new Exception("Im Sicherungsordner fehlt partitions/boot.img (Vollsicherung braucht Root). Stattdessen die Firmware-Datei AP_….tar.md5 wählen.");
                boot = File.ReadAllBytes(bp); if (File.Exists(vp)) vbmeta = File.ReadAllBytes(vp);
                var rp = Path.Combine(source, "partitions", "recovery.img"); if (File.Exists(rp)) recovery = File.ReadAllBytes(rp);
            }
            else if (source.ToLower().Contains(".tar"))
            {
                log("Lese Firmware " + Path.GetFileName(source) + " …");
                var e = Tar.Extract(source, n => n == "boot.img" || n == "boot.img.lz4" || n == "vbmeta.img" || n == "vbmeta.img.lz4" || n == "recovery.img" || n == "recovery.img.lz4");
                foreach (var kv in e) { if (kv.Key.StartsWith("boot.img")) boot = Unpack(kv.Key, kv.Value); else if (kv.Key.StartsWith("recovery.img")) recovery = Unpack(kv.Key, kv.Value); else vbmeta = Unpack(kv.Key, kv.Value); }
                if (boot == null) throw new Exception("In der Firmware-Datei ist keine boot.img. Bitte die AP-Datei wählen (AP_….tar.md5).");
                string pda = dev != null && dev.Fingerprint.Contains("/") ? dev.Fingerprint.Split(new[] { '/' })[4].Split(new[] { ':' })[0] : "";
                if (pda.Length > 0 && !Path.GetFileName(source).Contains(pda)) log("WARNUNG: Firmware-Datei passt dem Namen nach nicht zum Stand auf dem Handy (" + pda + "). Für die Wiederherstellung sollte es dieselbe Version sein.");
            }
            else boot = Unpack(source, File.ReadAllBytes(source));

            var img = BootImage.Parse(boot);
            log("Original-Boot-Abbild: Version " + img.Version + ", Kernel " + (img.Kernel.Length >> 10) + " KiB, Ramdisk " + (img.Ramdisk.Length >> 10) + " KiB, Seitengröße " + img.Page);
            // Geräte, deren Boot-Abbild kein Ramdisk hat (Samsung mit System-als-Root, z. B. Galaxy A30): der Bootloader
            // lädt dort kein Ramdisk aus der Boot-Partition – Android startet einfach weiter. Ultar OS kommt deshalb in die
            // Recovery-Partition (deren Ramdisk wird immer geladen); Android bleibt unverändert daneben bestehen.
            bool viaRecovery = img.Ramdisk.Length == 0 && recovery != null;
            byte[] nr = null;
            if (viaRecovery)
            {
                var rimg = BootImage.Parse(recovery);
                log("Boot-Abbild ohne Ramdisk: Ultar OS wird in die Recovery-Partition gebaut (Recovery: Kernel " + (rimg.Kernel.Length >> 10) + " KiB, Ramdisk " + (rimg.Ramdisk.Length >> 10) + " KiB).");
                rimg.Ramdisk = File.ReadAllBytes(initramfs);
                nr = rimg.Build();
                img.RamdiskAddr = rimg.HeaderRamdiskAddr;
                if (nr.Length > recovery.Length) throw new Exception("Das neue Recovery-Abbild ist größer als das Original und passt nicht in die Partition.");
            }
            {
                if (img.Ramdisk.Length == 0 && !viaRecovery) log("WARNUNG: Boot-Abbild ohne Ramdisk und keine recovery.img in der Quelle. Manche Geräte (Samsung) starten dann weiter Android. Besser die Firmware-Datei AP_….tar.md5 als Quelle wählen.");
                img.Kernel = PatchKernel(img.Kernel, log);
                img.Ramdisk = File.ReadAllBytes(initramfs);
            }
            var nb = img.Build();
            log("Boot-Abbild: Ramdisk-Ladeadresse 0x" + BitConverter.ToUInt32(nb, 20).ToString("X") + "."); // Ultar OS auch beim normalen Start (Android würde sonst die Recovery-Partition zurückschreiben)
            if (nb.Length > boot.Length) throw new Exception("Das neue Boot-Abbild (" + (nb.Length >> 10) + " KiB) ist größer als das Original (" + (boot.Length >> 10) + " KiB) und passt nicht in die Partition.");
            Array.Resize(ref nb, Math.Max(nb.Length, 0));

            Directory.CreateDirectory(outDir);
            var pkg = new StringBuilder();
            string code = dev != null && dev.Codename.Length > 0 ? dev.Codename : "unbekannt";
            if (code == "unbekannt") { var bk = Core.ReadKv(Path.Combine(backupDir, "device.txt")); if (bk.ContainsKey("device")) code = bk["device"]; } // Handy gerade nicht über adb erreichbar
            pkg.AppendLine("# Ultar-Paket, erzeugt von Ultar Flash am " + DateTime.UtcNow.ToString("yyyy-MM-dd HH:mm") + " UTC");
            pkg.AppendLine("device=" + code);
            File.WriteAllBytes(Path.Combine(outDir, "boot.img"), nb);
            pkg.AppendLine("image.boot=boot.img"); pkg.AppendLine("sha256.boot=" + Core.Sha256(Path.Combine(outDir, "boot.img")));
            var odin = new Dictionary<string, string> { { "boot.img", Path.Combine(outDir, "boot.img") } };
            if (viaRecovery)
            {
                File.WriteAllBytes(Path.Combine(outDir, "recovery.img"), nr);
                pkg.AppendLine("image.recovery=recovery.img"); pkg.AppendLine("sha256.recovery=" + Core.Sha256(Path.Combine(outDir, "recovery.img")));
                pkg.AppendLine("start=recovery");
                odin["recovery.img"] = Path.Combine(outDir, "recovery.img");
            }
            if (vbmeta != null)
            {
                File.WriteAllBytes(Path.Combine(outDir, "vbmeta.img"), PatchVbmeta(vbmeta));
                pkg.AppendLine("image.vbmeta=vbmeta.img"); pkg.AppendLine("sha256.vbmeta=" + Core.Sha256(Path.Combine(outDir, "vbmeta.img")));
                odin["vbmeta.img"] = Path.Combine(outDir, "vbmeta.img");
                log("vbmeta: durch leeres vbmeta ersetzt (Prüfung aus, Flags = 2).");
            }
            else log("Hinweis: keine vbmeta.img in der Quelle. Prüft das Gerät Abbilder (AVB), startet das Paket ohne geänderte vbmeta nicht.");
            File.WriteAllText(Path.Combine(outDir, "ultar-package.txt"), pkg.ToString());
            File.WriteAllText(Path.Combine(outDir, "ramdisk.sha256"), Core.Sha256(initramfs)); // damit ein neueres Kernsystem erkannt wird
            Tar.Write(Path.Combine(outDir, "ultar-" + code + "-odin-AP.tar"), odin); // Ausweg, falls heimdall das Gerät nicht bedienen kann

            // Originale als Wiederherstellungs-Sicherung ablegen (identisch mit dem, was auf dem Gerät liegt, wenn die Firmware-Version passt)
            if (!Directory.Exists(source))
            {
                var pdir = Path.Combine(backupDir, "partitions"); Directory.CreateDirectory(pdir);
                var man = Core.ReadManifest(backupDir);
                Action<string, byte[]> keep = (part, data) =>
                {
                    if (data == null || man.Any(m => m.Part.ToLower() == part && m.Verified)) return;
                    var f = Path.Combine(pdir, part + ".img"); File.WriteAllBytes(f, data);
                    man.RemoveAll(m => m.Part.ToLower() == part);
                    man.Add(new PartEntry { Part = part, File = part + ".img", Size = data.Length, Sha = Core.Sha256(f), Verified = true });
                };
                keep("boot", boot); keep("vbmeta", vbmeta); keep("recovery", recovery);
                Core.WriteManifest(backupDir, man);
                if (!File.Exists(Path.Combine(backupDir, "device.txt")) && dev != null) File.WriteAllText(Path.Combine(backupDir, "device.txt"), "model=" + dev.Model + "\ndevice=" + dev.Codename + "\nbrand=" + dev.Brand + "\nserial=" + dev.Serial + "\n");
                log("Originale (boot" + (vbmeta != null ? ", vbmeta" : "") + ") für die Wiederherstellung abgelegt: " + pdir);
            }
            log("Paket fertig: " + outDir);
            return outDir;
        }
    }
}
