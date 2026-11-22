// Ultar Flash – Kernlogik: Gerät erkennen, Treiber und Partitionen sichern, flashen, wiederherstellen.
// KM Code Craft. Nutzt die Programme adb, fastboot und heimdall (Ordner "tools" neben der .exe oder PATH).
using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.IO;
using System.Linq;
using System.Security.Cryptography;
using System.Text;

namespace UltarFlash
{
    public class Result { public int Code; public string Out = ""; public string Err = ""; public bool Ok { get { return Code == 0; } } }

    public class DeviceInfo
    {
        public string Mode = "none";      // none | adb | recovery | unauthorized | fastboot | download
        public string Model = "", Codename = "", Fingerprint = "", Kernel = "", Serial = "", Brand = "";
        public int Battery = -1;
        public bool Root, Unlocked, LockKnown;
        public bool Samsung { get { return Brand.ToLower().Contains("samsung") || Model.StartsWith("SM-"); } }
    }

    public class PartEntry { public string Part, File, Sha; public long Size; public bool Verified; }

    public class Core
    {
        public Action<string> Log = s => { };
        public Action<int> Progress = p => { };
        public string ToolDir;
        /// Testgerät, das verloren gehen darf: Flashen auch ohne EFS-Sicherung (die Root bräuchte). EFS wird trotzdem nie geschrieben.
        public bool TestDevice;
        /// Seriennummer des gewählten Geräts; adb-Befehle gehen gezielt dorthin (wichtig, wenn mehrere Geräte/Emulatoren gelistet sind).
        public string Serial = "";

        // Partitionen, die das Werkzeug schreiben darf. Bootloader, Partitionstabelle, Modem und EFS sind tabu:
        // solange sie unberührt bleiben, kommt man immer wieder in den Download-/Fastboot-Modus.
        public static readonly string[] Writable = { "boot", "recovery", "dtbo", "dtb", "vbmeta", "system", "vendor", "userdata", "cache", "super" };
        // Standardumfang der Vollsicherung (klein, aber alles Wichtige inkl. IMEI-/Kalibrierdaten)
        public static readonly string[] Critical = { "boot", "recovery", "dtb", "dtbo", "vbmeta", "vbmeta_samsung", "efs", "sec_efs", "cpefs", "param", "up_param", "radio", "modem", "persist", "nvram", "nvdata", "proinfo" };
        // Herstellertreiber und Firmware, ohne Root lesbar
        public static readonly string[] DriverDirs = { "/vendor/firmware", "/vendor/lib/modules", "/vendor/etc/wifi", "/vendor/etc/firmware", "/vendor/etc/bluetooth", "/system/etc/firmware", "/vendor/firmware_mnt/image", "/vendor/etc/init", "/vendor/lib/hw", "/vendor/lib64/hw" };

        public Core()
        {
            ToolDir = Path.Combine(AppDomain.CurrentDomain.BaseDirectory, "tools");
        }

        /// Sucht adb/fastboot/heimdall: Ordner "tools" neben der .exe, dann Dokumente\UltarFlash\tools, sonst PATH.
        string Tool(string name)
        {
            var dirs = new[] { ToolDir, Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.MyDocuments), "UltarFlash", "tools") };
            foreach (var d in dirs)
                foreach (var ext in new[] { ".exe", "" })
                {
                    var p = Path.Combine(d, name + ext);
                    if (File.Exists(p)) return p;
                }
            return name; // aus PATH
        }

        public bool HasTool(string name)
        {
            try { return Run(name, name == "heimdall" ? "version" : "version", null, 8000).Code >= 0; } catch { return false; }
        }

        /// Programm ausführen. Mit outFile wird die Standardausgabe binär in die Datei geschrieben.
        public Result Run(string tool, string args, string outFile = null, int timeoutMs = 0)
        {
            var r = new Result();
            if (tool == "adb" && Serial.Length > 0 && !args.StartsWith("devices") && !args.StartsWith("start-server") && !args.StartsWith("version")) args = "-s " + Serial + " " + args;
            var psi = new ProcessStartInfo(Tool(tool), args) { UseShellExecute = false, RedirectStandardOutput = true, RedirectStandardError = true, CreateNoWindow = true };
            using (var p = new Process { StartInfo = psi })
            {
                var err = new StringBuilder();
                p.ErrorDataReceived += (s, e) => { if (e.Data != null) lock (err) err.AppendLine(e.Data); };
                try { p.Start(); }
                catch (Exception e) { r.Code = -1; r.Err = tool + ": " + e.Message; return r; }
                p.BeginErrorReadLine();
                if (outFile != null)
                {
                    using (var fs = File.Create(outFile)) p.StandardOutput.BaseStream.CopyTo(fs, 1 << 20);
                }
                else r.Out = p.StandardOutput.ReadToEnd();
                if (timeoutMs > 0) { if (!p.WaitForExit(timeoutMs)) { try { p.Kill(); } catch { } r.Code = -2; return r; } }
                p.WaitForExit();
                r.Code = p.ExitCode;
                lock (err) r.Err = err.ToString();
            }
            return r;
        }

        static string Q(string s) { return "\"" + s.Replace("\"", "\\\"") + "\""; }

        /// Befehl auf dem Gerät ausführen; root=true nutzt su (bzw. in der Recovery direkt root).
        public Result Sh(string cmd, bool root, DeviceInfo d, string outFile = null)
        {
            string verb = outFile != null ? "exec-out" : "shell";
            if (root && d.Mode != "recovery") cmd = "su -c " + "'" + cmd.Replace("'", "'\\''") + "'";
            return Run("adb", verb + " " + Q(cmd), outFile);
        }

        // ---------------- Geführter Ablauf ----------------

        public string UserToolDir { get { return Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.MyDocuments), "UltarFlash", "tools"); } }

        public bool HasAdb() { return Run("adb", "version", null, 8000).Code == 0; }
        public bool HasHeimdall() { return Run("heimdall", "version", null, 8000).Code == 0; }

        /// Lädt die Android-„platform-tools“ (adb, fastboot) von Google und entpackt sie nach Dokumente\UltarFlash\tools.
        public void DownloadPlatformTools()
        {
            var url = Environment.GetEnvironmentVariable("ULTARFLASH_PLATFORM_TOOLS_URL") ?? "https://dl.google.com/android/repository/platform-tools-latest-windows.zip";
            var zip = Path.Combine(Path.GetTempPath(), "platform-tools.zip");
            System.Net.ServicePointManager.SecurityProtocol |= (System.Net.SecurityProtocolType)3072;
            using (var wc = new System.Net.WebClient()) wc.DownloadFile(url, zip);
            Directory.CreateDirectory(UserToolDir);
            using (var z = System.IO.Compression.ZipFile.OpenRead(zip))
                foreach (var e in z.Entries)
                {
                    if (e.Name.Length == 0) continue; // Ordner
                    using (var src = e.Open()) using (var dst = File.Create(Path.Combine(UserToolDir, e.Name))) src.CopyTo(dst);
                }
            File.Delete(zip);
        }

        /// Führt Schritt für Schritt durch: Werkzeuge -> Handy vorbereiten -> verbinden -> freigeben -> Treiber sichern -> Vollsicherung.
        /// step(nr, schlüssel) meldet den aktuellen Schritt an die Oberfläche. Liefert den Sicherungsordner oder null bei Abbruch.
        public string RunGuide(string baseDir, Action<int, string> step, Func<bool> cancelled, out DeviceInfo dev)
        {
            dev = new DeviceInfo();
            step(1, "g_tools");
            if (!HasAdb())
            {
                step(1, "g_toolsdl");
                try { DownloadPlatformTools(); } catch (Exception e) { Log("Download fehlgeschlagen: " + e.Message); }
                if (!HasAdb()) { step(1, "g_toolsfail"); return null; }
            }
            Run("adb", "start-server", null, 20000);
            // Warten, bis das Handy angeschlossen und USB-Debugging eingeschaltet ist
            step(2, "g_prepare");
            while (true)
            {
                if (cancelled()) return null;
                dev = Detect();
                if (dev.Mode == "unauthorized") step(3, "g_authorize");
                else if (dev.Mode == "adb" || dev.Mode == "recovery") break;
                else if (dev.Mode == "download" || dev.Mode == "fastboot") { step(2, "g_wrongmode"); }
                else step(2, "g_prepare");
                System.Threading.Thread.Sleep(1500);
            }
            var dir = Path.Combine(baseDir, (dev.Codename.Length > 0 ? dev.Codename : "geraet") + "-" + (dev.Serial.Length > 0 ? dev.Serial : "x"));
            step(4, "g_drivers");
            int n = BackupDrivers(dev, dir);
            Log(n + " Treiberdateien -> " + dir);
            if (cancelled()) return null;
            if (!dev.Root) { step(5, dev.LockKnown && !dev.Unlocked ? "g_next_unlock" : "g_next_build"); return dir; }
            step(5, "g_full");
            var l = BackupPartitions(dev, dir, false);
            bool ok = l.Count > 0 && l.All(e => e.Verified);
            step(6, ok ? (dev.LockKnown && !dev.Unlocked ? "g_done_locked" : "g_done") : "g_fullfail");
            return dir;
        }

        public DeviceInfo Detect()
        {
            var d = new DeviceInfo();
            var a = Run("adb", "devices", null, 15000);
            // Erstes nutzbares Gerät nehmen; abgeschaltete Emulatoren ("offline") überspringen, echte Geräte vor Emulatoren
            var rows = a.Out.Split(new[] { '\n' }).Skip(1).Select(l => l.Trim().Split(new[] { '\t', ' ' }, StringSplitOptions.RemoveEmptyEntries)).Where(t => t.Length >= 2 && (t[1] == "device" || t[1] == "recovery" || t[1] == "unauthorized")).OrderBy(t => t[0].StartsWith("emulator-") ? 1 : 0).ToList();
            Serial = "";
            if (rows.Count > 0)
            {
                d.Serial = rows[0][0];
                d.Mode = rows[0][1] == "device" ? "adb" : rows[0][1];
                Serial = d.Serial;
            }
            if (d.Mode == "adb" || d.Mode == "recovery")
            {
                Func<string, string> prop = k => Run("adb", "shell getprop " + k).Out.Trim();
                d.Model = prop("ro.product.model"); d.Codename = prop("ro.product.device"); d.Brand = prop("ro.product.brand");
                d.Fingerprint = prop("ro.build.fingerprint");
                d.Kernel = Run("adb", "shell uname -r").Out.Trim();
                var bat = Run("adb", "shell dumpsys battery").Out;
                foreach (var l in bat.Split(new[] { '\n' })) { var t = l.Trim(); if (t.StartsWith("level:")) int.TryParse(t.Substring(6).Trim(), out d.Battery); }
                if (d.Battery < 0) { int b; if (int.TryParse(Run("adb", "shell cat /sys/class/power_supply/battery/capacity").Out.Trim(), out b)) d.Battery = b; }
                d.Root = d.Mode == "recovery" ? Run("adb", "shell id").Out.Contains("uid=0") : Run("adb", "shell " + Q("su -c id"), null, 15000).Out.Contains("uid=0");
                // Sperrzustand des Bootloaders, soweit das Gerät ihn meldet
                string locked = prop("ro.boot.flash.locked"), vbs = prop("ro.boot.verifiedbootstate"), oem = prop("ro.boot.other.locked");
                if (locked == "0" || vbs == "orange") { d.Unlocked = true; d.LockKnown = true; }
                else if (locked == "1" || vbs == "green" || oem == "1") { d.Unlocked = false; d.LockKnown = true; }
                return d;
            }
            if (d.Mode == "unauthorized") return d;
            var f = Run("fastboot", "devices", null, 15000);
            if (f.Ok && f.Out.Trim().Length > 0)
            {
                d.Mode = "fastboot"; d.Serial = f.Out.Trim().Split(new[] { '\t', ' ' })[0];
                var v = Run("fastboot", "getvar all", null, 15000); var all = v.Out + v.Err;
                foreach (var l in all.Split(new[] { '\n' }))
                {
                    var t = l.Replace("(bootloader)", "").Trim();
                    if (t.StartsWith("product:")) d.Codename = t.Substring(8).Trim();
                    if (t.StartsWith("unlocked:")) { d.Unlocked = t.Contains("yes"); d.LockKnown = true; }
                }
                return d;
            }
            var h = Run("heimdall", "detect", null, 15000);
            if (h.Ok) { d.Mode = "download"; d.Brand = "samsung"; }
            return d;
        }

        public static string Sha256(string file)
        {
            using (var s = SHA256.Create()) using (var f = File.OpenRead(file))
                return BitConverter.ToString(s.ComputeHash(f)).Replace("-", "").ToLower();
        }

        // ---------------- Sicherung ----------------

        string ByName(DeviceInfo d)
        {
            var r = Sh("ls -d /dev/block/by-name /dev/block/platform/*/by-name /dev/block/platform/*/*/by-name /dev/block/bootdevice/by-name 2>/dev/null", true, d);
            return r.Out.Split(new[] { '\n' }).Select(x => x.Trim()).FirstOrDefault(x => x.Length > 0);
        }

        /// Geräteinfos und Herstellertreiber sichern (ohne Root möglich). Liefert die Zahl der gesicherten Dateien.
        public int BackupDrivers(DeviceInfo d, string dir)
        {
            // Schon gesichert (gleiches Gerät, gleicher Softwarestand)? Dann nicht noch einmal alles kopieren.
            var prev = ReadKv(Path.Combine(dir, "device.txt"));
            int had;
            if (prev.ContainsKey("driver_files") && int.TryParse(prev["driver_files"], out had) && had > 0 && prev.ContainsKey("fingerprint") && prev["fingerprint"] == d.Fingerprint)
            {
                Log("Treiber und Geräteinfos sind bereits gesichert (" + had + " Dateien).");
                return had;
            }
            Directory.CreateDirectory(dir);
            var info = new StringBuilder();
            info.AppendLine("model=" + d.Model); info.AppendLine("device=" + d.Codename); info.AppendLine("brand=" + d.Brand);
            info.AppendLine("fingerprint=" + d.Fingerprint); info.AppendLine("kernel=" + d.Kernel); info.AppendLine("serial=" + d.Serial);
            info.AppendLine("date=" + DateTime.UtcNow.ToString("yyyy-MM-ddTHH:mm:ssZ"));
            File.WriteAllText(Path.Combine(dir, "device.txt"), info.ToString());
            var dumps = new Dictionary<string, string> {
                { "getprop.txt", "getprop" }, { "lsmod.txt", "lsmod" }, { "cmdline.txt", "cat /proc/cmdline" }, { "partitions.txt", "cat /proc/partitions" },
                { "cpuinfo.txt", "cat /proc/cpuinfo" }, { "power_supply.txt", "ls /sys/class/power_supply/" }, { "input.txt", "cat /proc/bus/input/devices" },
                { "net.txt", "ls /sys/class/net/" }, { "modalias.txt", "cat /sys/bus/*/devices/*/modalias" }, { "dmesg.txt", "dmesg" } };
            foreach (var kv in dumps)
            {
                var r = Sh(kv.Value + " 2>/dev/null", d.Root, d);
                if (r.Out.Trim().Length > 0) File.WriteAllText(Path.Combine(dir, kv.Key), r.Out);
            }
            Run("adb", "pull /proc/config.gz " + Q(Path.Combine(dir, "kernel-config.gz")));
            int n = 0, i = 0;
            foreach (var src in DriverDirs)
            {
                Progress(100 * i++ / DriverDirs.Length);
                if (!Sh("ls -d " + src + " 2>/dev/null", false, d).Out.Contains(src)) continue;
                var dst = Path.Combine(dir, "drivers", src.Trim('/').Replace('/', '_'));
                Directory.CreateDirectory(dst);
                Log("Sichere " + src + " …");
                var r = Run("adb", "pull " + src + " " + Q(dst));
                int c = Directory.GetFiles(dst, "*", SearchOption.AllDirectories).Length;
                n += c;
                Log("  " + c + " Dateien" + (r.Ok ? "" : " (teilweise – einige Dateien sind ohne Root nicht lesbar)"));
            }
            Progress(100);
            File.AppendAllText(Path.Combine(dir, "device.txt"), "driver_files=" + n + "\n");
            return n;
        }

        /// Partitionen sichern (braucht Root oder Recovery). Jede Datei wird gegen die Prüfsumme auf dem Gerät geprüft.
        public List<PartEntry> BackupPartitions(DeviceInfo d, string dir, bool includeLarge)
        {
            var list = new List<PartEntry>();
            if (!d.Root) { Log("Keine Root-Rechte: Partitionen können nicht gelesen werden."); return list; }
            var bn = ByName(d);
            if (bn == null) { Log("Partitionsverzeichnis (by-name) nicht gefunden."); return list; }
            var names = Sh("ls " + bn, true, d).Out.Split(new[] { '\n' }).Select(x => x.Trim()).Where(x => x.Length > 0).ToList();
            File.WriteAllLines(Path.Combine(dir, "by-name.txt"), names.ToArray());
            var want = names.Where(n => Critical.Contains(n.ToLower()) || (includeLarge && (n.ToLower() == "vendor" || n.ToLower() == "system" || n.ToLower() == "super"))).ToList();
            var pdir = Path.Combine(dir, "partitions"); Directory.CreateDirectory(pdir);
            int i = 0;
            foreach (var n in want)
            {
                Progress(100 * i++ / Math.Max(1, want.Count));
                var e = new PartEntry { Part = n, File = n + ".img" };
                var local = Path.Combine(pdir, e.File);
                Log("Lese Partition " + n + " …");
                var r = Sh("dd if=" + bn + "/" + n + " bs=4194304 2>/dev/null", true, d, local);
                e.Size = File.Exists(local) ? new FileInfo(local).Length : 0;
                if (!r.Ok || e.Size == 0) { Log("  FEHLER: " + n + " nicht lesbar"); try { File.Delete(local); } catch { } continue; }
                e.Sha = Sha256(local);
                var remote = Sh("sha256sum " + bn + "/" + n, true, d).Out.Trim().Split(new[] { ' ' })[0].ToLower();
                e.Verified = remote == e.Sha;
                Log("  " + (e.Size >> 10) + " KiB, Prüfsumme " + (e.Verified ? "stimmt" : "STIMMT NICHT – Sicherung dieser Partition unbrauchbar"));
                list.Add(e);
            }
            Progress(100);
            WriteManifest(dir, list);
            return list;
        }

        public static void WriteManifest(string dir, List<PartEntry> l)
        {
            File.WriteAllLines(Path.Combine(dir, "manifest.txt"), l.Select(e => e.Part + "|" + e.File + "|" + e.Size + "|" + e.Sha + "|" + (e.Verified ? "1" : "0")).ToArray());
        }

        public static List<PartEntry> ReadManifest(string dir)
        {
            var p = Path.Combine(dir, "manifest.txt");
            var l = new List<PartEntry>();
            if (!File.Exists(p)) return l;
            foreach (var line in File.ReadAllLines(p))
            {
                var t = line.Split(new[] { '|' });
                if (t.Length == 5) l.Add(new PartEntry { Part = t[0], File = t[1], Size = long.Parse(t[2]), Sha = t[3], Verified = t[4] == "1" });
            }
            return l;
        }

        public static Dictionary<string, string> ReadKv(string file)
        {
            var d = new Dictionary<string, string>();
            if (!File.Exists(file)) return d;
            foreach (var l in File.ReadAllLines(file)) { int i = l.IndexOf('='); if (i > 0 && !l.TrimStart().StartsWith("#")) d[l.Substring(0, i).Trim()] = l.Substring(i + 1).Trim(); }
            return d;
        }

        // ---------------- Flashen ----------------

        /// Prüft alle Bedingungen. Leere Liste = Flashen erlaubt.
        public List<string> CheckFlash(DeviceInfo d, string pkgDir, string backupDir, out Dictionary<string, string> images)
        {
            var problems = new List<string>();
            images = new Dictionary<string, string>();
            var pkg = ReadKv(Path.Combine(pkgDir, "ultar-package.txt"));
            if (pkg.Count == 0) { problems.Add("Kein Ultar-Paket: ultar-package.txt fehlt in " + pkgDir); return problems; }
            var devs = (pkg.ContainsKey("device") ? pkg["device"] : "").Split(new[] { ',' }).Select(x => x.Trim().ToLower()).Where(x => x.Length > 0).ToList();
            var bd0 = ReadKv(Path.Combine(backupDir, "device.txt"));
            if (d.Codename.Length == 0 && TestDevice && bd0.ContainsKey("device") && bd0.ContainsKey("model"))
            {
                // Handy ist nicht über adb erreichbar (z. B. schon im Download-Modus): Gerät aus dem Sicherungsordner übernehmen.
                // Nur für Testgeräte; die Partitionstabelle des Handys wird beim Schreiben trotzdem geprüft.
                d.Codename = bd0["device"]; d.Model = bd0["model"]; if (bd0.ContainsKey("brand")) d.Brand = bd0["brand"]; if (d.Mode == "none") d.Mode = "download";
                Log("WARNUNG: Handy nicht über adb erkannt (Download-Modus?). Gerät laut Sicherungsordner: " + d.Model + ". Als Testgerät erlaubt – Akku und Bootloader-Zustand sind ungeprüft.");
            }
            if (d.Codename.Length == 0 || !devs.Contains(d.Codename.ToLower()))
                problems.Add("Paket ist für [" + string.Join(", ", devs.ToArray()) + "] gebaut, angeschlossen ist \"" + d.Codename + "\" (" + d.Model + ").");
            foreach (var kv in pkg.Where(k => k.Key.StartsWith("image.")))
            {
                var part = kv.Key.Substring(6).ToLower();
                var file = Path.Combine(pkgDir, kv.Value);
                if (!Writable.Contains(part)) problems.Add("Partition \"" + part + "\" darf nicht geschrieben werden (nur: " + string.Join(", ", Writable) + ").");
                else if (!File.Exists(file)) problems.Add("Abbild fehlt: " + kv.Value);
                else
                {
                    string want;
                    if (pkg.TryGetValue("sha256." + part, out want) && want.ToLower() != Sha256(file)) problems.Add("Abbild " + kv.Value + " ist beschädigt (Prüfsumme stimmt nicht).");
                    images[part] = file;
                }
            }
            if (images.Count == 0) problems.Add("Paket enthält keine Abbilder.");
            var man = ReadManifest(backupDir);
            var bdev = ReadKv(Path.Combine(backupDir, "device.txt"));
            if (man.Count == 0) problems.Add("Keine Vollsicherung gefunden. Erst sichern, dann flashen.");
            else
            {
                if (bdev.ContainsKey("serial") && d.Serial.Length > 0 && bdev["serial"] != d.Serial) problems.Add("Die Sicherung stammt von einem anderen Gerät (Seriennummer " + bdev["serial"] + ").");
                foreach (var part in images.Keys.Where(p => p != "userdata" && p != "cache"))
                {
                    var e = man.FirstOrDefault(m => m.Part.ToLower() == part);
                    if (e == null || !e.Verified) problems.Add("Für \"" + part + "\" gibt es keine geprüfte Sicherung – ohne sie ist kein Zurück möglich.");
                    else if (!File.Exists(Path.Combine(backupDir, "partitions", e.File)) || Sha256(Path.Combine(backupDir, "partitions", e.File)) != e.Sha) problems.Add("Sicherung von \"" + part + "\" ist beschädigt.");
                }
                if (d.Samsung && !man.Any(m => m.Part.ToLower() == "efs" && m.Verified))
                {
                    if (TestDevice) Log("WARNUNG: keine Sicherung der EFS-Partition (IMEI/Funkdaten) – als Testgerät trotzdem erlaubt. EFS wird nicht geschrieben.");
                    else problems.Add("Keine geprüfte Sicherung der EFS-Partition (IMEI/Funkdaten). Nur als Testgerät übergehbar.");
                }
            }
            if (d.Battery >= 0 && d.Battery < 50) problems.Add("Akku bei " + d.Battery + " % – mindestens 50 % nötig.");
            if (d.LockKnown && !d.Unlocked) problems.Add("Bootloader ist gesperrt. Erst in den Entwickleroptionen OEM-Entsperrung aktivieren und entsperren (löscht alle Daten).");
            return problems;
        }

        bool WaitMode(string mode, int seconds)
        {
            for (int i = 0; i < seconds; i++)
            {
                if (Detect().Mode == mode) return true;
                System.Threading.Thread.Sleep(1000);
            }
            return false;
        }

        /// Abbilder schreiben. Samsung: Download-Modus über heimdall, sonst fastboot.
        public bool WriteImages(DeviceInfo d, Dictionary<string, string> images)
        {
            foreach (var p in images.Keys) if (!Writable.Contains(p)) { Log("Abbruch: " + p + " ist gesperrt."); return false; }
            if (d.Samsung)
            {
                // Bevorzugt der eigene Schreibweg über den COM-Port des Download-Modus (kein heimdall, kein Treibertausch).
                // Mit ULTARFLASH_USE_HEIMDALL=1 oder wenn kein COM-Port gefunden wird: heimdall, falls vorhanden.
                bool useHeimdall = Environment.GetEnvironmentVariable("ULTARFLASH_USE_HEIMDALL") == "1";
                if (!useHeimdall)
                {
                    if (d.Mode != "download") { Log("Starte in den Download-Modus …"); Run("adb", "reboot download"); }
                    int wait; if (!int.TryParse(Environment.GetEnvironmentVariable("ULTARFLASH_LOKE_WAIT"), out wait)) wait = 60;
                    if (Loke.Flash(images, wait, Log, Progress)) return true;
                    Log("Download-Modus verlassen: Leiser + Ein/Aus etwa 7 Sekunden halten. Ausweg: mit Odin flashen (ultar-…-odin-AP.tar bei „AP“).");
                    return false;
                }
                if (!HasHeimdall())
                {
                    Log("heimdall.exe fehlt im tools-Ordner. Ohne heimdall kann Ultar Flash ein Samsung nicht selbst flashen.");
                    return false;
                }
                if (d.Mode != "download")
                {
                    Log("Starte in den Download-Modus …"); Run("adb", "reboot download");
                    if (!WaitMode("download", 60)) { Log("Gerät ist nicht im Download-Modus angekommen."); return false; }
                }
                // Namen so verwenden, wie sie in der Partitionstabelle (PIT) des Geräts stehen
                var pit = Run("heimdall", "print-pit --no-reboot");
                var pitNames = (pit.Out + pit.Err).Split(new[] { '\n' }).Select(l => l.Trim()).Where(l => l.StartsWith("Partition Name:")).Select(l => l.Substring(15).Trim()).ToList();
                var args = new StringBuilder("flash --resume");
                foreach (var kv in images)
                {
                    var name = pitNames.FirstOrDefault(n => n.ToLower() == kv.Key);
                    if (name == null) { Log("Abbruch: Partition " + kv.Key + " steht nicht in der Partitionstabelle des Geräts."); return false; }
                    args.Append(" --" + name + " " + Q(kv.Value));
                }
                Log("heimdall " + args);
                var r = Run("heimdall", args.ToString());
                Log((r.Out + r.Err).Trim());
                return r.Ok;
            }
            if (d.Mode != "fastboot") { Log("Starte in den Fastboot-Modus …"); Run("adb", "reboot bootloader"); if (!WaitMode("fastboot", 60)) { Log("Gerät ist nicht im Fastboot-Modus angekommen."); return false; } }
            foreach (var kv in images)
            {
                Log("fastboot flash " + kv.Key);
                var r = Run("fastboot", "flash " + kv.Key + " " + Q(kv.Value));
                Log((r.Out + r.Err).Trim());
                if (!r.Ok) return false;
            }
            Run("fastboot", "reboot");
            return true;
        }

        /// Ursprungszustand wiederherstellen: gesicherte, schreibbare Partitionen zurückschreiben.
        public bool Restore(DeviceInfo d, string backupDir, List<string> problems)
        {
            var man = ReadManifest(backupDir);
            var images = new Dictionary<string, string>();
            foreach (var e in man.Where(m => Writable.Contains(m.Part.ToLower()) && m.Part.ToLower() != "userdata"))
            {
                var f = Path.Combine(backupDir, "partitions", e.File);
                if (!e.Verified) { problems.Add(e.Part + ": Sicherung war nie geprüft, wird ausgelassen."); continue; }
                if (!File.Exists(f) || Sha256(f) != e.Sha) { problems.Add(e.Part + ": Sicherungsdatei beschädigt – Abbruch."); return false; }
                images[e.Part.ToLower()] = f;
            }
            if (images.Count == 0) { problems.Add("Die Sicherung enthält keine zurückschreibbaren Partitionen."); return false; }
            var bdev = ReadKv(Path.Combine(backupDir, "device.txt"));
            if (d.Codename.Length > 0 && bdev.ContainsKey("device") && bdev["device"].ToLower() != d.Codename.ToLower()) { problems.Add("Sicherung gehört zu \"" + bdev["device"] + "\", angeschlossen ist \"" + d.Codename + "\"."); return false; }
            if (d.Samsung || bdev.ContainsKey("brand") && bdev["brand"].ToLower().Contains("samsung")) d.Brand = "samsung";
            return WriteImages(d, images);
        }
    }
}
