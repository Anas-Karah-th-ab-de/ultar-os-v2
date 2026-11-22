// Ultar Flash – Fensteroberfläche (Windows Forms) und Kommandozeile.
using System;
using System.Collections.Generic;
using System.Drawing;
using System.IO;
using System.Linq;
using System.Threading;
using System.Windows.Forms;

namespace UltarFlash
{
    static class T
    {
        public static int L = 0; // 0 Deutsch, 1 English, 2 العربية
        static readonly Dictionary<string, string[]> S = new Dictionary<string, string[]> {
            { "detect", new[] { "Gerät erkennen", "Detect device", "اكتشاف الجهاز" } },
            { "drivers", new[] { "Treiber && Infos sichern", "Back up drivers && info", "نسخ التعريفات والمعلومات" } },
            { "full", new[] { "Vollsicherung (Root/Recovery)", "Full backup (root/recovery)", "نسخ احتياطي كامل (روت/ريكفري)" } },
            { "flash", new[] { "Ultar OS flashen …", "Flash Ultar OS …", "تثبيت Ultar OS …" } },
            { "restore", new[] { "Original wiederherstellen …", "Restore original …", "استعادة النظام الأصلي …" } },
            { "folder", new[] { "Sicherungsordner", "Backup folder", "مجلد النسخ الاحتياطي" } },
            { "nodev", new[] { "Kein Gerät gefunden. USB-Debugging einschalten und Kabel prüfen.", "No device found. Enable USB debugging and check the cable.", "لم يُعثر على جهاز. فعّل تصحيح USB وافحص الكابل." } },
            { "unauth", new[] { "Am Handy die USB-Debugging-Anfrage bestätigen.", "Confirm the USB debugging prompt on the phone.", "وافق على طلب تصحيح USB على الهاتف." } },
            { "confirm", new[] { "Zum Bestätigen die Modellbezeichnung eintippen:", "Type the model name to confirm:", "اكتب اسم الطراز للتأكيد:" } },
            { "blocked", new[] { "Flashen ist gesperrt:", "Flashing is blocked:", "التثبيت محظور:" } },
            { "done", new[] { "Fertig.", "Done.", "تم." } },
            { "failed", new[] { "Fehlgeschlagen.", "Failed.", "فشل." } },
            { "update", new[] { "Nach Updates suchen", "Check for updates", "البحث عن تحديثات" } },
            { "token", new[] { "GitHub-Token (privates Projekt)", "GitHub token (private project)", "رمز GitHub (مشروع خاص)" } },
            { "uptodate", new[] { "Ultar Flash ist aktuell.", "Ultar Flash is up to date.", "Ultar Flash محدَّث." } },
            { "newver", new[] { "Neue Version verfügbar:", "New version available:", "إصدار جديد متوفر:" } },
            { "diag", new[] { "Diagnose vom Handy lesen", "Read diagnostics from phone", "قراءة التشخيص من الهاتف" } },
            { "guide", new[] { "Geführter Ablauf starten", "Start guided setup", "بدء الإعداد الموجَّه" } },
            { "cancel", new[] { "Abbrechen", "Cancel", "إلغاء" } },
            { "g_tools", new[] { "Werkzeuge prüfen …", "Checking tools …", "فحص الأدوات …" } },
            { "g_toolsdl", new[] { "adb fehlt. Lade die Android-Werkzeuge von Google (dl.google.com) …", "adb is missing. Downloading the Android tools from Google (dl.google.com) …", "adb غير موجود. جارٍ تنزيل أدوات أندرويد من Google …" } },
            { "g_toolsfail", new[] { "adb konnte nicht geladen werden. Bitte die „platform-tools“ von developer.android.com laden und nach Dokumente\\UltarFlash\\tools entpacken, dann erneut starten.", "adb could not be downloaded. Please get the platform-tools from developer.android.com, unzip them to Documents\\UltarFlash\\tools and start again.", "تعذّر تنزيل adb. نزّل platform-tools من developer.android.com وفك الضغط إلى Documents\\UltarFlash\\tools ثم أعد المحاولة." } },
            { "g_prepare", new[] { "Handy vorbereiten:\r\n1. Einstellungen → Telefoninfo → Softwareinformationen: 7× auf „Buildnummer“ tippen.\r\n2. Einstellungen → Entwickleroptionen: „USB-Debugging“ einschalten.\r\n3. Dort auch „OEM-Entsperrung“ einschalten (nötig fürs spätere Flashen).\r\n4. Handy per USB-Kabel anschließen.\r\n\r\nIch warte, bis das Handy erkannt wird …", "Prepare the phone:\r\n1. Settings → About phone → Software information: tap “Build number” 7 times.\r\n2. Settings → Developer options: turn on “USB debugging”.\r\n3. Also turn on “OEM unlocking” (needed for flashing later).\r\n4. Connect the phone with a USB cable.\r\n\r\nWaiting for the phone …", "جهّز الهاتف:\r\n1. الإعدادات ← حول الهاتف ← معلومات البرنامج: اضغط 7 مرات على «رقم الإصدار».\r\n2. الإعدادات ← خيارات المطوّر: فعّل «تصحيح USB».\r\n3. فعّل أيضًا «إلغاء قفل OEM» (لازم للتثبيت لاحقًا).\r\n4. صِل الهاتف بكابل USB.\r\n\r\nبانتظار اكتشاف الهاتف …" } },
            { "g_authorize", new[] { "Handy gefunden. Am Handy erscheint „USB-Debugging zulassen?“ – Haken bei „Von diesem Computer immer zulassen“ setzen und „Zulassen“ tippen.", "Phone found. The phone asks “Allow USB debugging?” – tick “Always allow from this computer” and tap “Allow”.", "تم العثور على الهاتف. يظهر على الهاتف «السماح بتصحيح USB؟» – فعّل «السماح دائمًا من هذا الحاسوب» ثم اضغط «سماح»." } },
            { "g_wrongmode", new[] { "Das Handy ist im Download-/Fastboot-Modus. Bitte normal starten (lange Ein/Aus + Leiser halten) und USB-Debugging einschalten.", "The phone is in download/fastboot mode. Please boot it normally and turn on USB debugging.", "الهاتف في وضع التنزيل/Fastboot. شغّله بشكل عادي وفعّل تصحيح USB." } },
            { "g_drivers", new[] { "Verbunden. Sichere Treiber, Firmware und Geräteinfos – bitte das Kabel nicht abziehen …", "Connected. Backing up drivers, firmware and device info – do not unplug the cable …", "تم الاتصال. جارٍ نسخ التعريفات والبرامج الثابتة ومعلومات الجهاز – لا تفصل الكابل …" } },
            { "g_full", new[] { "Root vorhanden. Sichere jetzt Boot, Recovery und die IMEI-/Funkdaten und prüfe jede Datei …", "Root available. Backing up boot, recovery and the IMEI/radio data, verifying every file …", "صلاحيات الروت متوفرة. جارٍ نسخ الإقلاع والريكفري وبيانات IMEI والتحقق من كل ملف …" } },
            { "g_next_unlock", new[] { "Treiber und Geräteinfos sind gesichert. Mehr kann ich vom Handy nicht lesen: Ohne Root gibt Android das Boot-Abbild nicht heraus.\r\n\r\nNächster Schritt – Bootloader entsperren (löscht alle Daten):\r\n1. Entwickleroptionen → „OEM-Entsperrung“ einschalten.\r\n2. Handy ausschalten. Lauter + Leiser gedrückt halten und das USB-Kabel anstecken.\r\n3. Lauter lange drücken, dann mit Lauter bestätigen.\r\n4. Android neu einrichten, einmal ins Internet, USB-Debugging wieder einschalten.\r\nDanach: „Ultar-Paket bauen …“.", "Drivers and device info are backed up. I cannot read more from the phone: without root Android does not hand out the boot image.\r\n\r\nNext – unlock the bootloader (erases all data):\r\n1. Developer options → turn on “OEM unlocking”.\r\n2. Power off. Hold Volume Up + Volume Down and plug in the USB cable.\r\n3. Long-press Volume Up, then confirm with Volume Up.\r\n4. Set up Android again, go online once, turn USB debugging back on.\r\nThen: “Build Ultar package …”.", "تم نسخ التعريفات ومعلومات الجهاز. لا يمكن قراءة أكثر من ذلك: دون روت لا يعطي أندرويد صورة الإقلاع.\r\n\r\nالتالي – فتح قفل محمّل الإقلاع (يمسح كل البيانات):\r\n1. خيارات المطوّر ← فعّل «إلغاء قفل OEM».\r\n2. أطفئ الهاتف. اضغط رفع الصوت + خفض الصوت معًا وصِل كابل USB.\r\n3. اضغط مطوّلًا على رفع الصوت ثم أكّد.\r\n4. أعد إعداد أندرويد، اتصل بالإنترنت مرة، وفعّل تصحيح USB.\r\nبعدها: «بناء حزمة Ultar …»." } },
            { "g_next_build", new[] { "Treiber und Geräteinfos sind gesichert, der Bootloader ist entsperrt.\r\n\r\nNächster Schritt: „Ultar-Paket bauen …“ und die Firmware-Datei des Handys wählen (AP_….tar.md5). Ohne Root gibt Android das Boot-Abbild nicht heraus – deshalb kommt es aus der Firmware.", "Drivers and device info are backed up, the bootloader is unlocked.\r\n\r\nNext: “Build Ultar package …” and choose the phone's firmware file (AP_….tar.md5). Without root Android does not hand out the boot image – so it comes from the firmware.", "تم نسخ التعريفات ومحمّل الإقلاع مفتوح.\r\n\r\nالتالي: «بناء حزمة Ultar …» واختر ملف برنامج الهاتف (AP_….tar.md5). دون روت لا يعطي أندرويد صورة الإقلاع، لذلك تؤخذ من ملف البرنامج." } },
            { "g_build", new[] { "Baue das Ultar-Paket aus der Firmware-Datei …", "Building the Ultar package from the firmware file …", "جارٍ بناء حزمة Ultar من ملف البرنامج …" } },
            { "g_check", new[] { "Paket ist fertig. Prüfe, ob geflasht werden darf …", "The package is ready. Checking whether flashing is allowed …", "الحزمة جاهزة. جارٍ التحقق من إمكانية التثبيت …" } },
            { "g_blocked", new[] { "Flashen ist gesperrt. Die Gründe stehen unten im Protokoll. Ist es ein Testgerät, links „Testgerät – darf verloren gehen“ anhaken und den geführten Ablauf erneut starten.", "Flashing is blocked. The reasons are in the log below. If this is a test device, tick “Test device – may be lost” on the left and start the guided setup again.", "التثبيت محظور. الأسباب في السجل أدناه. إن كان جهاز اختبار فعّل «جهاز اختبار – يمكن خسارته» ثم أعد بدء الإعداد الموجَّه." } },
            { "g_odin", new[] { "Das Paket ist fertig, aber heimdall.exe fehlt im tools-Ordner – damit kann ich ein Samsung nicht selbst flashen.\r\n\r\nZwei Wege:\r\n• heimdall.exe in den tools-Ordner legen und den geführten Ablauf erneut starten, oder\r\n• mit Odin flashen: Handy in den Download-Modus bringen, in Odin bei „AP“ diese Datei wählen und „Start“ drücken:\r\n{0}", "The package is ready, but heimdall.exe is missing in the tools folder – without it I cannot flash a Samsung myself.\r\n\r\nTwo ways:\r\n• put heimdall.exe into the tools folder and start the guided setup again, or\r\n• flash with Odin: put the phone into download mode, choose this file under “AP” in Odin and press “Start”:\r\n{0}", "الحزمة جاهزة، لكن heimdall.exe غير موجود في مجلد tools – دونه لا أستطيع تثبيت جهاز سامسونج.\r\n\r\nطريقتان:\r\n• ضع heimdall.exe في مجلد tools وأعد بدء الإعداد الموجَّه، أو\r\n• ثبّت عبر Odin: أدخل الهاتف وضع التنزيل، اختر هذا الملف في خانة «AP» ثم اضغط «Start»:\r\n{0}" } },
            { "g_flashing", new[] { "Flashe Ultar OS – Kabel nicht abziehen, Handy nicht anfassen …", "Flashing Ultar OS – do not unplug the cable or touch the phone …", "جارٍ تثبيت Ultar OS – لا تفصل الكابل ولا تلمس الهاتف …" } },
            { "g_flashed", new[] { "Geflasht. Das Handy startet jetzt neu.\r\nDreimal kurzes Vibrieren = Ultar OS wurde gestartet. Bleibt das Bild beim Samsung-Logo stehen: Kabel stecken lassen und „Diagnose vom Handy lesen“ drücken.\r\nZurück zu Android: Download-Modus und „Original wiederherstellen …“.", "Flashed. The phone is restarting now.\r\nThree short vibrations = Ultar OS was started. If the picture stays on the Samsung logo: keep the cable plugged in and press “Read diagnostics from phone”.\r\nBack to Android: download mode and “Restore original …”.", "تم التثبيت. الهاتف يعيد التشغيل الآن.\r\nثلاث اهتزازات قصيرة = تم تشغيل Ultar OS. إن بقيت الصورة على شعار سامسونج: أبقِ الكابل موصولًا واضغط «قراءة التشخيص من الهاتف».\r\nللعودة إلى أندرويد: وضع التنزيل ثم «استعادة النظام الأصلي …»." } },
            { "g_flashfail", new[] { "Das Flashen ist fehlgeschlagen. Details stehen im Protokoll. Am Handy wurde nur geschrieben, was dort als erfolgreich gemeldet ist.", "Flashing failed. Details are in the log. Only what is reported as successful there was written to the phone.", "فشل التثبيت. التفاصيل في السجل. لم يُكتب على الهاتف إلا ما ورد هناك أنه نجح." } },
            { "nopkg", new[] { "Es gibt noch kein Ultar-Paket für dieses Handy. Erst „Ultar-Paket bauen …“ ausführen (dafür die Firmware-Datei AP_….tar.md5 des Handys wählen).", "There is no Ultar package for this phone yet. Run “Build Ultar package …” first (choose the phone's firmware file AP_….tar.md5).", "لا توجد حزمة Ultar لهذا الهاتف بعد. نفّذ أولًا «بناء حزمة Ultar …» (اختر ملف البرنامج AP_….tar.md5)." } },
            { "whyfw", new[] { "Das Handy ist verbunden, aber ohne Root gibt Android das Boot-Abbild nicht heraus. Es wird deshalb aus der Firmware-Datei von Samsung genommen (AP_….tar.md5, Version siehe oben in der Statuszeile).", "The phone is connected, but without root Android does not hand out the boot image. It is therefore taken from Samsung's firmware file (AP_….tar.md5, version shown in the status line).", "الهاتف متصل، لكن دون روت لا يعطي أندرويد صورة الإقلاع. لذلك تؤخذ من ملف برنامج سامسونج (AP_….tar.md5)." } },
            { "g_noroot", new[] { "Treiber und Geräteinfos sind gesichert.\r\nFür die Vollsicherung (Boot, IMEI-Daten) braucht das Handy Root oder eine Custom-Recovery (z. B. TWRP). Ohne Vollsicherung flasht Ultar Flash nicht – so bleibt immer ein Weg zurück.", "Drivers and device info are backed up.\r\nThe full backup (boot, IMEI data) needs root or a custom recovery (e.g. TWRP). Without a full backup Ultar Flash will not flash – so there is always a way back.", "تم نسخ التعريفات ومعلومات الجهاز.\r\nالنسخ الكامل (الإقلاع وبيانات IMEI) يحتاج روت أو ريكفري مخصص (مثل TWRP). دون نسخة كاملة لن يثبّت Ultar Flash أي شيء – ليبقى طريق العودة متاحًا." } },
            { "g_fullfail", new[] { "Die Vollsicherung ist unvollständig oder nicht prüfbar. Details stehen im Protokoll. Es wird nicht geflasht.", "The full backup is incomplete or could not be verified. See the log. Nothing will be flashed.", "النسخة الكاملة ناقصة أو تعذّر التحقق منها. راجع السجل. لن يتم التثبيت." } },
            { "g_done", new[] { "Fertig: Treiber und Vollsicherung liegen geprüft im Sicherungsordner.\r\nNächster Schritt: „Ultar OS flashen …“ und den Ordner mit dem Ultar-Paket für dieses Gerät wählen.", "Done: drivers and full backup are verified in the backup folder.\r\nNext: “Flash Ultar OS …” and choose the Ultar package folder for this device.", "تم: التعريفات والنسخة الكاملة محفوظة ومتحقق منها.\r\nالتالي: «تثبيت Ultar OS …» واختر مجلد حزمة Ultar لهذا الجهاز." } },
            { "g_done_locked", new[] { "Fertig: Treiber und Vollsicherung liegen geprüft im Sicherungsordner.\r\nDer Bootloader ist noch gesperrt. Entsperren löscht alle Daten auf dem Handy – erst danach lässt sich flashen.", "Done: drivers and full backup are verified.\r\nThe bootloader is still locked. Unlocking erases all data on the phone – flashing is possible only after that.", "تم: التعريفات والنسخة الكاملة متحقق منها.\r\nمحمّل الإقلاع ما زال مقفلًا. فتح القفل يمسح كل بيانات الهاتف – وبعدها فقط يمكن التثبيت." } },
            { "build", new[] { "Ultar-Paket bauen …", "Build Ultar package …", "بناء حزمة Ultar …" } },
            { "testdev", new[] { "Testgerät – darf verloren gehen", "Test device – may be lost", "جهاز اختبار – يمكن خسارته" } },
            { "pickfw", new[] { "Samsung-Firmware (AP_….tar.md5) oder boot.img wählen", "Choose Samsung firmware (AP_….tar.md5) or boot.img", "اختر برنامج سامسونج (AP_….tar.md5) أو boot.img" } },
            { "pickrd", new[] { "Ultar-Kernsystem für das Handy wählen (ultar-initramfs-aarch64.cpio.gz)", "Choose the Ultar core system for the phone (ultar-initramfs-aarch64.cpio.gz)", "اختر نظام Ultar الأساسي للهاتف (ultar-initramfs-aarch64.cpio.gz)" } },
            { "st_battery", new[] { "Akku", "Battery", "البطارية" } },
            { "st_root", new[] { "Root", "Root", "روت" } },
            { "st_yes", new[] { "ja", "yes", "نعم" } },
            { "st_no", new[] { "nein", "no", "لا" } },
            { "st_boot", new[] { "Bootloader", "Bootloader", "محمّل الإقلاع" } },
            { "st_unlocked", new[] { "entsperrt", "unlocked", "مفتوح" } },
            { "st_locked", new[] { "gesperrt", "locked", "مقفل" } },
            { "st_kernel", new[] { "Kernel", "Kernel", "النواة" } },
            { "st_mode_adb", new[] { "verbunden", "connected", "متصل" } },
            { "st_mode_recovery", new[] { "Recovery", "recovery", "ريكفري" } },
            { "st_mode_fastboot", new[] { "Fastboot-Modus", "fastboot mode", "وضع Fastboot" } },
            { "st_mode_download", new[] { "Download-Modus", "download mode", "وضع التنزيل" } },
            { "askupdate", new[] { "Jetzt herunterladen und installieren?", "Download and install now?", "هل تريد التنزيل والتثبيت الآن؟" } },
            { "busy", new[] { "Läuft …", "Working …", "جارٍ التنفيذ …" } },
            { "pkg", new[] { "Ordner mit dem Ultar-Paket wählen", "Choose the Ultar package folder", "اختر مجلد حزمة Ultar" } },
            { "warn", new[] { "Flashen ersetzt das System auf dem Handy. Bootloader, Modem und IMEI-Daten bleiben unberührt; das Original lässt sich aus der Sicherung zurückschreiben.", "Flashing replaces the system on the phone. Bootloader, modem and IMEI data stay untouched; the original can be written back from the backup.", "التثبيت يستبدل نظام الهاتف. يبقى محمّل الإقلاع والمودم وبيانات IMEI دون تغيير، ويمكن إعادة النظام الأصلي من النسخة الاحتياطية." } },
        };
        public static string Get(string k) { return S[k][L]; }
    }

    static class Program
    {
        [STAThread]
        static int Main(string[] a)
        {
            if (a.Length > 0 && a[0] == "--cli") return Cli(a.Skip(1).ToArray());
            Application.EnableVisualStyles();
            Application.Run(new MainForm());
            return 0;
        }

        // Kommandozeile: --cli detect | drivers <ordner> | full <ordner> [--large] | check <paket> <sicherung> | flash <paket> <sicherung> --yes <modell> | restore <sicherung> --yes <modell>
        static int Cli(string[] a)
        {
            try { Console.OutputEncoding = new System.Text.UTF8Encoding(false); Console.InputEncoding = new System.Text.UTF8Encoding(false); } catch { }
            var c = new Core { Log = Console.WriteLine };
            var tools = Environment.GetEnvironmentVariable("ULTARFLASH_TOOLS");
            if (tools != null) c.ToolDir = tools;
            var d = c.Detect();
            Console.WriteLine("mode=" + d.Mode + " model=" + d.Model + " device=" + d.Codename + " battery=" + d.Battery + " root=" + d.Root + " unlocked=" + (d.LockKnown ? d.Unlocked.ToString() : "?"));
            if (a.Length >= 2 && a[0] == "diag") return Diag.Read(a[1], 5, Console.WriteLine) != null ? 0 : 1;
            if (a.Length > 0 && a[0] == "update")
            {
                try
                {
                    var r = Updater.Check();
                    Console.WriteLine("current=" + Updater.Version + " latest=" + r.Tag + " newer=" + r.Newer);
                    if (r.Newer && a.Contains("--download")) Console.WriteLine("msi=" + Updater.Download(r, Console.WriteLine));
                    return 0;
                }
                catch (Exception e) { Console.WriteLine("FEHLER: " + e.Message); return 1; }
            }
            if (a.Length == 0 || a[0] == "detect") return d.Mode == "none" ? 1 : 0;
            c.TestDevice = a.Contains("--testdevice");
            if (a.Length >= 5 && a[0] == "build")
            {
                // build <quelle> <initramfs> <paketordner> <sicherungsordner>
                try { PackageBuilder.Build(a[1], a[2], a[3], a[4], d, Console.WriteLine); return 0; }
                catch (Exception e) { Console.WriteLine("FEHLER: " + e.Message); return 1; }
            }
            if (a.Length >= 2 && a[0] == "translate")
            {
                // Prüfhilfe: translate <en|ar> – liest Meldungen von der Standardeingabe und gibt sie übersetzt aus
                T.L = a[1] == "ar" ? 2 : 1; string ln;
                while ((ln = Console.ReadLine()) != null) Console.WriteLine(Tr.Line(ln));
                return 0;
            }
            string yes = null; int yi = Array.IndexOf(a, "--yes"); if (yi >= 0 && yi + 1 < a.Length) yes = a[yi + 1];
            switch (a[0])
            {
                case "guide":
                    DeviceInfo gd; int waited = 0;
                    var gdir = c.RunGuide(a[1], (n, k) => Console.WriteLine("SCHRITT " + n + " " + k), () => waited++ > 400, out gd);
                    Console.WriteLine("ordner=" + gdir);
                    return gdir != null ? 0 : 1;
                case "drivers": Console.WriteLine("files=" + c.BackupDrivers(d, a[1])); return 0;
                case "full":
                    c.BackupDrivers(d, a[1]);
                    var l = c.BackupPartitions(d, a[1], a.Contains("--large"));
                    return l.Count > 0 && l.All(e => e.Verified) ? 0 : 1;
                case "check":
                case "flash":
                    int ri = Array.IndexOf(a, "--ramdisk"); if (ri >= 0 && ri + 1 < a.Length) PackageBuilder.Refresh(a[1], a[ri + 1], a[2], d, Console.WriteLine);
                    Dictionary<string, string> images;
                    var pr = c.CheckFlash(d, a[1], a[2], out images);
                    foreach (var p in pr) Console.WriteLine("BLOCK: " + p);
                    if (pr.Count > 0) return 2;
                    if (a[0] == "check") { Console.WriteLine("OK"); return 0; }
                    if (yes != d.Model) { Console.WriteLine("BLOCK: Bestätigung fehlt (--yes " + d.Model + ")"); return 2; }
                    return c.WriteImages(d, images) ? 0 : 1;
                case "restore":
                    var probs = new List<string>();
                    var bdev = Core.ReadKv(Path.Combine(a[1], "device.txt"));
                    if (yes == null || !bdev.ContainsKey("model") || yes != bdev["model"]) { Console.WriteLine("BLOCK: Bestätigung fehlt (--yes <modell>)"); return 2; }
                    bool ok = c.Restore(d, a[1], probs);
                    foreach (var p in probs) Console.WriteLine("HINWEIS: " + p);
                    return ok ? 0 : 1;
            }
            return 64;
        }
    }

    class MainForm : Form
    {
        readonly Core core = new Core();
        DeviceInfo dev = new DeviceInfo();
        readonly TextBox log = new TextBox(), folder = new TextBox();
        readonly Label status = new Label(), title = new Label(), folderLabel = new Label();
        readonly ProgressBar bar = new ProgressBar();
        readonly Button bDetect = new Button(), bDrivers = new Button(), bFull = new Button(), bFlash = new Button(), bRestore = new Button(), bBrowse = new Button(), bUpdate = new Button(), bDiag = new Button();
        readonly Button bGuide = new Button(), bBuild = new Button();
        readonly CheckBox testDev = new CheckBox();
        readonly Label stepLabel = new Label(), stepText = new Label();
        volatile bool cancelGuide;
        bool guiding;
        string lastStep = "";
        readonly TextBox token = new TextBox();
        readonly Label tokenLabel = new Label();
        readonly ComboBox lang = new ComboBox();
        readonly Color gold = Color.FromArgb(184, 144, 47), bg = Color.FromArgb(15, 17, 21), card = Color.FromArgb(27, 30, 37), fg = Color.FromArgb(242, 243, 245);

        public MainForm()
        {
            Text = "Ultar Flash – KM Code Craft"; ClientSize = new Size(860, 690); MinimumSize = new Size(780, 680); BackColor = bg; ForeColor = fg; Font = new Font("Segoe UI", 10f);
            title.Text = "Ultar Flash"; title.Font = new Font("Segoe UI", 20f, FontStyle.Bold); title.ForeColor = gold; title.SetBounds(20, 12, 400, 44);
            lang.Items.AddRange(new object[] { "Deutsch", "English", "العربية" }); lang.DropDownStyle = ComboBoxStyle.DropDownList; lang.SetBounds(720, 20, 120, 28); lang.Anchor = AnchorStyles.Top | AnchorStyles.Right;
            lang.SelectedIndexChanged += (s, e) => { T.L = lang.SelectedIndex; Relabel(); };
            status.SetBounds(20, 62, 820, 66); status.BackColor = card; status.Padding = new Padding(10); status.Anchor = AnchorStyles.Top | AnchorStyles.Left | AnchorStyles.Right;
            int y = 140;
            foreach (var b in new[] { bDetect, bDrivers, bFull, bBuild, bFlash, bRestore, bDiag })
            {
                b.SetBounds(20, y, 270, 40); b.FlatStyle = FlatStyle.Flat; b.BackColor = b == bFlash ? gold : card; b.ForeColor = b == bFlash ? Color.Black : fg; b.FlatAppearance.BorderColor = gold; y += 46; Controls.Add(b);
            }
            testDev.SetBounds(20, y, 270, 24); testDev.ForeColor = fg; testDev.CheckedChanged += (s, e) => core.TestDevice = testDev.Checked; Controls.Add(testDev);
            y += 22;
            bBuild.Click += (s, e) => BuildPackage();
            folderLabel.SetBounds(20, y + 6, 270, 22);
            folder.SetBounds(20, y + 30, 230, 28); folder.BackColor = card; folder.ForeColor = fg; folder.BorderStyle = BorderStyle.FixedSingle;
            folder.Text = Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.MyDocuments), "UltarFlash");
            bBrowse.Text = "…"; bBrowse.SetBounds(254, y + 29, 36, 28); bBrowse.FlatStyle = FlatStyle.Flat; bBrowse.BackColor = card;
            bBrowse.Click += (s, e) => { using (var f = new FolderBrowserDialog()) if (f.ShowDialog() == DialogResult.OK) folder.Text = f.SelectedPath; };
            log.Multiline = true; log.ReadOnly = true; log.ScrollBars = ScrollBars.Vertical; log.BackColor = card; log.ForeColor = fg; log.BorderStyle = BorderStyle.None; log.Font = new Font("Consolas", 9.5f);
            // Geführter Ablauf: großer Knopf, Schrittanzeige und Anleitung über dem Protokoll
            bGuide.SetBounds(310, 140, 530, 44); bGuide.FlatStyle = FlatStyle.Flat; bGuide.BackColor = gold; bGuide.ForeColor = Color.Black; bGuide.Font = new Font(Font, FontStyle.Bold); bGuide.Anchor = AnchorStyles.Top | AnchorStyles.Left | AnchorStyles.Right;
            bGuide.Click += (s, e) => { if (guiding) cancelGuide = true; else Guide(); };
            stepLabel.SetBounds(310, 192, 530, 24); stepLabel.ForeColor = gold; stepLabel.Font = new Font(Font, FontStyle.Bold); stepLabel.Anchor = AnchorStyles.Top | AnchorStyles.Left | AnchorStyles.Right;
            stepText.SetBounds(310, 218, 530, 150); stepText.BackColor = card; stepText.Padding = new Padding(10); stepText.Anchor = AnchorStyles.Top | AnchorStyles.Left | AnchorStyles.Right;
            Controls.AddRange(new Control[] { bGuide, stepLabel, stepText });
            log.SetBounds(310, 378, 530, 262); log.Anchor = AnchorStyles.Top | AnchorStyles.Bottom | AnchorStyles.Left | AnchorStyles.Right;
            bar.SetBounds(20, 660, 820, 14); bar.Anchor = AnchorStyles.Bottom | AnchorStyles.Left | AnchorStyles.Right;
            tokenLabel.SetBounds(20, y + 66, 270, 22);
            token.SetBounds(20, y + 90, 270, 28); token.BackColor = card; token.ForeColor = fg; token.BorderStyle = BorderStyle.FixedSingle; token.UseSystemPasswordChar = true; token.Text = Updater.Token;
            token.Leave += (s, e) => { try { Updater.Token = token.Text; } catch { } };
            bUpdate.SetBounds(20, y + 128, 270, 36); bUpdate.FlatStyle = FlatStyle.Flat; bUpdate.BackColor = card; bUpdate.ForeColor = fg; bUpdate.FlatAppearance.BorderColor = gold;
            bUpdate.Click += (s, e) => CheckUpdate(true);
            title.Text = "Ultar Flash " + Updater.Version;
            Controls.AddRange(new Control[] { title, lang, status, folderLabel, folder, bBrowse, tokenLabel, token, bUpdate, log, bar });
            // Standardsprache nach der Windows-Sprache: Arabisch, Deutsch, sonst Englisch
            var ui = System.Globalization.CultureInfo.CurrentUICulture.TwoLetterISOLanguageName;
            T.L = ui == "ar" ? 2 : ui == "de" ? 0 : 1;
            lang.SelectedIndex = T.L;
            // Alle Meldungen laufen durch die Übersetzung
            core.Log = s => BeginInvoke((Action)(() => log.AppendText(Tr.Line(s) + Environment.NewLine)));
            core.Progress = p => BeginInvoke((Action)(() => bar.Value = Math.Max(0, Math.Min(100, p))));
            bDetect.Click += (s, e) => Work(() => { dev = core.Detect(); });
            bDrivers.Click += (s, e) => Work(() => { dev = core.Detect(); if (Ready()) core.Log(core.BackupDrivers(dev, BackupDir()) + " Dateien -> " + BackupDir()); });
            bFull.Click += (s, e) => Work(() => { dev = core.Detect(); if (Ready()) { core.BackupDrivers(dev, BackupDir()); var l = core.BackupPartitions(dev, BackupDir(), false); core.Log(l.Count(x => x.Verified) + "/" + l.Count + " Partitionen geprüft gesichert -> " + BackupDir()); } });
            bFlash.Click += (s, e) => Flash();
            bRestore.Click += (s, e) => RestoreClick();
            bDiag.Click += (s, e) => Work(() => Diag.Read(Path.Combine(folder.Text, "diagnose"), 60, core.Log));
            Relabel(); ShowStatus();
            Shown += (s, e) => { Work(() => { dev = core.Detect(); }); CheckUpdate(false); };
        }

        string BackupDir() { return Path.Combine(folder.Text, (dev.Codename.Length > 0 ? dev.Codename : "geraet") + "-" + (dev.Serial.Length > 0 ? dev.Serial : "x")); }

        bool Ready()
        {
            if (dev.Mode == "unauthorized") { core.Log(T.Get("unauth")); return false; }
            if (dev.Mode != "adb" && dev.Mode != "recovery") { core.Log(T.Get("nodev")); return false; }
            return true;
        }

        /// Im Hintergrund nach einer neuen Version sehen; verbose=true meldet auch "aktuell" und Fehler.
        void CheckUpdate(bool verbose)
        {
            try { Updater.Token = token.Text; } catch { }
            new Thread(() =>
            {
                try
                {
                    var r = Updater.Check();
                    if (!r.Newer) { if (verbose) core.Log(T.Get("uptodate") + " (" + Updater.Version + ")"); return; }
                    core.Log(T.Get("newver") + " " + r.Tag);
                    bool go = false;
                    Invoke((Action)(() => go = MessageBox.Show(this, T.Get("newver") + " " + r.Tag + "\r\n\r\n" + T.Get("askupdate"), "Ultar Flash", MessageBoxButtons.YesNo) == DialogResult.Yes));
                    if (!go) return;
                    var msi = Updater.Download(r, core.Log);
                    Updater.Install(msi);
                    Invoke((Action)Close);
                }
                catch (Exception ex) { if (verbose) core.Log("Update: " + ex.Message + (Updater.Token.Length == 0 ? "  (Projekt ist privat – GitHub-Token eintragen)" : "")); }
            }) { IsBackground = true }.Start();
        }

        /// Geführter Ablauf: erkennt selbst, wann der Nutzer den nächsten Handgriff erledigt hat.
        void Guide()
        {
            guiding = true; cancelGuide = false; bGuide.Text = T.Get("cancel");
            Work(() =>
            {
                DeviceInfo d;
                var dir = core.RunGuide(folder.Text, (n, key) => BeginInvoke((Action)(() =>
                {
                    lastStep = key;
                    stepLabel.Text = n + " / 6";
                    stepText.Text = T.Get(key);
                    bar.Value = Math.Min(100, n * 100 / 6);
                })), () => cancelGuide, out d);
                dev = d;
                if (dir != null) core.Log("Sicherungsordner: " + dir);
                // Ohne Root, aber entsperrt: gleich weiter mit Paketbau und Flashen
                if (dir != null && !cancelGuide && !d.Root && d.LockKnown && d.Unlocked) GuideBuildAndFlash();
                BeginInvoke((Action)(() => { guiding = false; bGuide.Text = T.Get("guide"); bGuide.Enabled = true; }));
            });
            bGuide.Enabled = true; // bleibt als „Abbrechen“ bedienbar
        }

        string PackageDir() { return Path.Combine(BackupDir(), "paket"); }

        void SetStep(int n, string key, string arg = "")
        {
            BeginInvoke((Action)(() => { lastStep = key; stepLabel.Text = n + " / 6"; stepText.Text = T.Get(key).Replace("{0}", arg); bar.Value = Math.Min(100, n * 100 / 6); }));
        }

        /// Ultar-Kernsystem fürs Handy suchen: neben der .exe oder im Sicherungsordner, sonst nachfragen.
        string LocalRamdisk()
        {
            foreach (var d in new[] { AppDomain.CurrentDomain.BaseDirectory, folder.Text })
            {
                var c = Path.Combine(d, "ultar-initramfs-aarch64.cpio.gz");
                if (File.Exists(c)) return c;
            }
            return null;
        }

        string FindRamdisk()
        {
            foreach (var d in new[] { AppDomain.CurrentDomain.BaseDirectory, folder.Text })
            {
                var c = Path.Combine(d, "ultar-initramfs-aarch64.cpio.gz");
                if (File.Exists(c)) return c;
            }
            string rd = null;
            Invoke((Action)(() => { using (var f = new OpenFileDialog { Title = T.Get("pickrd"), Filter = "Ultar-Kernsystem|*.cpio.gz;*.cpio.zst|Alle Dateien|*.*" }) if (f.ShowDialog(this) == DialogResult.OK) rd = f.FileName; }));
            return rd;
        }

        /// Fortsetzung des geführten Ablaufs (läuft im Arbeits-Thread): Paket bauen, prüfen, flashen.
        void GuideBuildAndFlash()
        {
            string pkg = PackageDir();
            if (!File.Exists(Path.Combine(pkg, "ultar-package.txt")))
            {
                SetStep(5, "whyfw");
                string src = null;
                Invoke((Action)(() => { using (var f = new OpenFileDialog { Title = T.Get("pickfw"), Filter = "Firmware / Boot-Abbild|*.tar;*.md5;*.img;*.lz4|Alle Dateien|*.*" }) if (f.ShowDialog(this) == DialogResult.OK) src = f.FileName; }));
                if (src == null) { SetStep(5, "g_next_build"); return; }
                var rd = FindRamdisk();
                if (rd == null) { SetStep(5, "g_next_build"); return; }
                SetStep(5, "g_build");
                PackageBuilder.Build(src, rd, pkg, BackupDir(), dev, core.Log);
            }
            else PackageBuilder.Refresh(pkg, LocalRamdisk(), BackupDir(), dev, core.Log);
            SetStep(6, "g_check");
            Dictionary<string, string> images;
            var pr = core.CheckFlash(dev, pkg, BackupDir(), out images);
            if (pr.Count > 0) { core.Log(T.Get("blocked")); foreach (var p in pr) core.Log("  • " + p); SetStep(6, "g_blocked"); return; }
            bool go = false;
            Invoke((Action)(() => go = Confirm(dev.Model)));
            if (!go) { SetStep(6, "g_check"); return; }
            SetStep(6, "g_flashing");
            SetStep(6, core.WriteImages(dev, images) ? "g_flashed" : "g_flashfail");
        }

        /// Paket aus der Samsung-Firmware (oder boot.img / Sicherung) und dem Ultar-Kernsystem bauen.
        void BuildPackage()
        {
            string src;
            // Mit Root-Vollsicherung braucht es keine Datei: das Boot-Abbild liegt dann schon im Sicherungsordner
            if (File.Exists(Path.Combine(BackupDir(), "partitions", "boot.img")) && Core.ReadManifest(BackupDir()).Any(m => m.Part.ToLower() == "boot" && m.Verified)) src = BackupDir();
            else
            {
                stepText.Text = T.Get("whyfw"); core.Log(T.Get("whyfw"));
                using (var f = new OpenFileDialog { Title = T.Get("pickfw"), Filter = "Firmware / Boot-Abbild|*.tar;*.md5;*.img;*.lz4|Alle Dateien|*.*" }) { if (f.ShowDialog() != DialogResult.OK) return; src = f.FileName; }
            }
            Work(() =>
            {
                dev = core.Detect();
                if (!Ready()) return;
                var rd = FindRamdisk();
                if (rd == null) return;
                PackageBuilder.Build(src, rd, PackageDir(), BackupDir(), dev, core.Log);
            });
        }

        void Relabel()
        {
            bBuild.Text = T.Get("build"); testDev.Text = T.Get("testdev");
            bGuide.Text = guiding ? T.Get("cancel") : T.Get("guide");
            if (lastStep.Length > 0) stepText.Text = T.Get(lastStep);
            bUpdate.Text = T.Get("update"); tokenLabel.Text = T.Get("token");
            bDetect.Text = T.Get("detect"); bDrivers.Text = T.Get("drivers"); bFull.Text = T.Get("full"); bFlash.Text = T.Get("flash"); bRestore.Text = T.Get("restore"); bDiag.Text = T.Get("diag"); folderLabel.Text = T.Get("folder");
            RightToLeft = T.L == 2 ? RightToLeft.Yes : RightToLeft.No;
            RightToLeftLayout = T.L == 2; // ganze Anordnung spiegeln, nicht nur die Schrift
            ShowStatus();
        }

        void ShowStatus()
        {
            if (dev.Mode == "none") { status.Text = T.Get("nodev"); return; }
            if (dev.Mode == "unauthorized") { status.Text = T.Get("unauth"); return; }
            status.Text = dev.Model + "  (" + dev.Codename + ")   ·   " + T.Get("st_mode_" + dev.Mode) + "   ·   " + T.Get("st_battery") + " " + (dev.Battery >= 0 ? dev.Battery + " %" : "?") + "   ·   " + T.Get("st_root") + ": " + T.Get(dev.Root ? "st_yes" : "st_no") + "   ·   " + T.Get("st_boot") + ": " + (!dev.LockKnown ? "?" : T.Get(dev.Unlocked ? "st_unlocked" : "st_locked")) + "\r\n" + T.Get("st_kernel") + " " + dev.Kernel + "   ·   " + dev.Fingerprint;
        }

        void Work(Action a)
        {
            foreach (var b in new[] { bDetect, bDrivers, bFull, bBuild, bFlash, bRestore, bDiag }) b.Enabled = false;
            bar.Value = 0;
            new Thread(() =>
            {
                try { a(); } catch (Exception ex) { core.Log("Fehler: " + ex.Message); }
                BeginInvoke((Action)(() => { ShowStatus(); foreach (var b in new[] { bDetect, bDrivers, bFull, bBuild, bFlash, bRestore, bDiag }) b.Enabled = true; }));
            }) { IsBackground = true }.Start();
        }

        bool Confirm(string model)
        {
            using (var f = new Form { Text = "Ultar Flash", ClientSize = new Size(460, 190), StartPosition = FormStartPosition.CenterParent, FormBorderStyle = FormBorderStyle.FixedDialog, BackColor = bg, ForeColor = fg, Font = Font })
            {
                var l = new Label { Text = T.Get("warn") + "\r\n\r\n" + T.Get("confirm") + "  " + model }; l.SetBounds(16, 12, 428, 110);
                var t = new TextBox(); t.SetBounds(16, 126, 280, 28);
                var ok = new Button { Text = "OK", DialogResult = DialogResult.OK, BackColor = gold, ForeColor = Color.Black, FlatStyle = FlatStyle.Flat }; ok.SetBounds(310, 124, 134, 32);
                f.Controls.AddRange(new Control[] { l, t, ok }); f.AcceptButton = ok;
                return f.ShowDialog(this) == DialogResult.OK && t.Text.Trim() == model && model.Length > 0;
            }
        }

        void Flash()
        {
            Work(() =>
            {
                dev = core.Detect();
                if (!Ready()) return;
                // Das zu diesem Handy gebaute Paket verwenden – keine Ordnerfrage
                string pkg = PackageDir();
                if (!File.Exists(Path.Combine(pkg, "ultar-package.txt"))) { core.Log(T.Get("nopkg")); BeginInvoke((Action)(() => stepText.Text = T.Get("nopkg"))); return; }
                PackageBuilder.Refresh(pkg, LocalRamdisk(), BackupDir(), dev, core.Log);
                Dictionary<string, string> images;
                var pr = core.CheckFlash(dev, pkg, BackupDir(), out images);
                if (pr.Count > 0) { core.Log(T.Get("blocked")); foreach (var p in pr) core.Log("  • " + p); return; }
                bool go = false;
                Invoke((Action)(() => go = Confirm(dev.Model)));
                if (!go) return;
                core.Log(core.WriteImages(dev, images) ? T.Get("done") : T.Get("failed"));
            });
        }

        void RestoreClick()
        {
            string dir;
            using (var f = new FolderBrowserDialog { Description = T.Get("folder"), SelectedPath = folder.Text }) { if (f.ShowDialog() != DialogResult.OK) return; dir = f.SelectedPath; }
            var bdev = Core.ReadKv(Path.Combine(dir, "device.txt"));
            if (!bdev.ContainsKey("model")) { core.Log("Keine Sicherung in " + dir); return; }
            if (!Confirm(bdev["model"])) return;
            Work(() =>
            {
                dev = core.Detect();
                var probs = new List<string>();
                bool ok = core.Restore(dev, dir, probs);
                foreach (var p in probs) core.Log("  • " + p);
                core.Log(ok ? T.Get("done") : T.Get("failed"));
            });
        }
    }
}
