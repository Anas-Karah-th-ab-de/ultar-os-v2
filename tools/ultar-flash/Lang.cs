// Ultar Flash – Übersetzung der Meldungen. Der Kern erzeugt deutsche Texte; hier werden sie beim Anzeigen
// anhand von Mustern ({1}, {2} = veränderliche Teile) ins Englische oder Arabische übertragen.
using System;
using System.Collections.Generic;
using System.Text.RegularExpressions;

namespace UltarFlash
{
    static class Tr
    {
        // Deutsch (Muster) | English | العربية
        static readonly string[][] M = {
            new[] { "Sichere {1} …", "Backing up {1} …", "جارٍ نسخ {1} …" },
            new[] { "{1} Dateien (teilweise – einige Dateien sind ohne Root nicht lesbar)", "{1} files (partial – some files cannot be read without root)", "{1} ملفًا (جزئي – بعض الملفات لا تُقرأ دون روت)" },
            new[] { "{1} Dateien", "{1} files", "{1} ملفًا" },
            new[] { "{1} Treiberdateien -> {2}", "{1} driver files -> {2}", "{1} ملف تعريف -> {2}" },
            new[] { "{1} Dateien -> {2}", "{1} files -> {2}", "{1} ملفًا -> {2}" },
            new[] { "Treiber und Geräteinfos sind bereits gesichert ({1} Dateien).", "Drivers and device info are already backed up ({1} files).", "التعريفات ومعلومات الجهاز منسوخة مسبقًا ({1} ملفًا)." },
            new[] { "Keine Root-Rechte: Partitionen können nicht gelesen werden.", "No root access: partitions cannot be read.", "لا توجد صلاحيات روت: لا يمكن قراءة الأقسام." },
            new[] { "Partitionsverzeichnis (by-name) nicht gefunden.", "Partition directory (by-name) not found.", "لم يُعثر على دليل الأقسام (by-name)." },
            new[] { "Lese Partition {1} …", "Reading partition {1} …", "جارٍ قراءة القسم {1} …" },
            new[] { "FEHLER: {1} nicht lesbar", "ERROR: {1} cannot be read", "خطأ: تعذّرت قراءة {1}" },
            new[] { "{1} KiB, Prüfsumme stimmt", "{1} KiB, checksum matches", "{1} ك.بايت، المجموع الاختباري مطابق" },
            new[] { "{1} KiB, Prüfsumme STIMMT NICHT – Sicherung dieser Partition unbrauchbar", "{1} KiB, checksum DOES NOT MATCH – backup of this partition is unusable", "{1} ك.بايت، المجموع الاختباري غير مطابق – نسخة هذا القسم غير صالحة" },
            new[] { "{1}/{2} Partitionen geprüft gesichert -> {3}", "{1}/{2} partitions backed up and verified -> {3}", "تم نسخ {1}/{2} قسمًا والتحقق منها -> {3}" },
            new[] { "Sicherungsordner: {1}", "Backup folder: {1}", "مجلد النسخ الاحتياطي: {1}" },
            new[] { "Download fehlgeschlagen: {1}", "Download failed: {1}", "فشل التنزيل: {1}" },
            new[] { "Kein Ultar-Paket: ultar-package.txt fehlt in {1}", "No Ultar package: ultar-package.txt is missing in {1}", "لا توجد حزمة Ultar: الملف ultar-package.txt مفقود في {1}" },
            new[] { "Paket ist für [{1}] gebaut, angeschlossen ist \"{2}\" ({3}).", "The package is built for [{1}], connected is \"{2}\" ({3}).", "الحزمة مبنية لـ [{1}]، والجهاز المتصل هو \"{2}\" ({3})." },
            new[] { "Partition \"{1}\" darf nicht geschrieben werden (nur: {2}).", "Partition \"{1}\" must not be written (only: {2}).", "لا يُسمح بالكتابة على القسم \"{1}\" (فقط: {2})." },
            new[] { "Abbild fehlt: {1}", "Image missing: {1}", "الصورة مفقودة: {1}" },
            new[] { "Abbild {1} ist beschädigt (Prüfsumme stimmt nicht).", "Image {1} is damaged (checksum does not match).", "الصورة {1} تالفة (المجموع الاختباري غير مطابق)." },
            new[] { "Paket enthält keine Abbilder.", "The package contains no images.", "الحزمة لا تحتوي على صور." },
            new[] { "Keine Vollsicherung gefunden. Erst sichern, dann flashen.", "No full backup found. Back up first, then flash.", "لا توجد نسخة احتياطية كاملة. انسخ أولًا ثم ثبّت." },
            new[] { "Die Sicherung stammt von einem anderen Gerät (Seriennummer {1}).", "The backup is from a different device (serial {1}).", "النسخة الاحتياطية من جهاز آخر (الرقم التسلسلي {1})." },
            new[] { "Für \"{1}\" gibt es keine geprüfte Sicherung – ohne sie ist kein Zurück möglich.", "There is no verified backup of \"{1}\" – without it there is no way back.", "لا توجد نسخة متحقق منها للقسم \"{1}\" – دونها لا يمكن الرجوع." },
            new[] { "Sicherung von \"{1}\" ist beschädigt.", "The backup of \"{1}\" is damaged.", "نسخة القسم \"{1}\" تالفة." },
            new[] { "WARNUNG: keine Sicherung der EFS-Partition (IMEI/Funkdaten) – als Testgerät trotzdem erlaubt. EFS wird nicht geschrieben.", "WARNING: no backup of the EFS partition (IMEI/radio data) – allowed anyway as a test device. EFS is not written.", "تحذير: لا توجد نسخة لقسم EFS (بيانات IMEI والراديو) – مسموح لأنه جهاز اختبار. لن يُكتب على EFS." },
            new[] { "Keine geprüfte Sicherung der EFS-Partition (IMEI/Funkdaten). Nur als Testgerät übergehbar.", "No verified backup of the EFS partition (IMEI/radio data). Can only be skipped for a test device.", "لا توجد نسخة متحقق منها لقسم EFS (بيانات IMEI والراديو). يمكن التجاوز لجهاز اختبار فقط." },
            new[] { "Akku bei {1} % – mindestens 50 % nötig.", "Battery at {1} % – at least 50 % required.", "البطارية {1} % – المطلوب 50 % على الأقل." },
            new[] { "Bootloader ist gesperrt. Erst in den Entwickleroptionen OEM-Entsperrung aktivieren und entsperren (löscht alle Daten).", "The bootloader is locked. First enable OEM unlocking in the developer options and unlock it (erases all data).", "محمّل الإقلاع مقفل. فعّل أولًا «إلغاء قفل OEM» في خيارات المطوّر ثم افتح القفل (يمسح كل البيانات)." },
            new[] { "Abbruch: {1} ist gesperrt.", "Aborted: {1} is protected.", "أُلغي: القسم {1} محمي." },
            new[] { "Starte in den Download-Modus …", "Rebooting into download mode …", "جارٍ الإقلاع إلى وضع التنزيل …" },
            new[] { "Gerät ist nicht im Download-Modus angekommen.", "The device did not reach download mode.", "لم يصل الجهاز إلى وضع التنزيل." },
            new[] { "Starte in den Fastboot-Modus …", "Rebooting into fastboot mode …", "جارٍ الإقلاع إلى وضع Fastboot …" },
            new[] { "Gerät ist nicht im Fastboot-Modus angekommen.", "The device did not reach fastboot mode.", "لم يصل الجهاز إلى وضع Fastboot." },
            new[] { "Abbruch: Partition {1} steht nicht in der Partitionstabelle des Geräts.", "Aborted: partition {1} is not in the device's partition table.", "أُلغي: القسم {1} غير موجود في جدول أقسام الجهاز." },
            new[] { "{1}: Sicherung war nie geprüft, wird ausgelassen.", "{1}: backup was never verified, skipped.", "{1}: النسخة لم يُتحقق منها، تم التخطي." },
            new[] { "{1}: Sicherungsdatei beschädigt – Abbruch.", "{1}: backup file damaged – aborted.", "{1}: ملف النسخة تالف – أُلغي." },
            new[] { "Die Sicherung enthält keine zurückschreibbaren Partitionen.", "The backup contains no partitions that can be written back.", "النسخة لا تحتوي على أقسام يمكن إعادتها." },
            new[] { "Sicherung gehört zu \"{1}\", angeschlossen ist \"{2}\".", "The backup belongs to \"{1}\", connected is \"{2}\".", "النسخة تخص \"{1}\"، والجهاز المتصل هو \"{2}\"." },
            new[] { "Keine Sicherung in {1}", "No backup in {1}", "لا توجد نسخة احتياطية في {1}" },
            new[] { "Fehler: {1}", "Error: {1}", "خطأ: {1}" },
            new[] { "heimdall.exe fehlt im tools-Ordner. Ohne heimdall kann Ultar Flash ein Samsung nicht selbst flashen.", "heimdall.exe is missing in the tools folder. Without heimdall Ultar Flash cannot flash a Samsung itself.", "الملف heimdall.exe غير موجود في مجلد tools. دونه لا يستطيع Ultar Flash تثبيت جهاز سامسونج بنفسه." },
            new[] { "Ausweg: mit Odin flashen – die Datei ultar-…-odin-AP.tar aus dem Paketordner in Odin bei „AP“ wählen.", "Alternative: flash with Odin – choose the file ultar-…-odin-AP.tar from the package folder under “AP” in Odin.", "البديل: التثبيت عبر Odin – اختر الملف ultar-…-odin-AP.tar من مجلد الحزمة في خانة «AP»." },
            new[] { "Zeigt das Handy den Download-Bildschirm, fehlt heimdall der USB-Treiber: mit Zadig für das Gerät „WinUSB“ installieren, oder mit Odin flashen (ultar-…-odin-AP.tar bei „AP“).", "If the phone shows the download screen, heimdall lacks the USB driver: install “WinUSB” for the device with Zadig, or flash with Odin (ultar-…-odin-AP.tar under “AP”).", "إن ظهرت شاشة التنزيل على الهاتف فتعريف USB الخاص بـ heimdall مفقود: ثبّت «WinUSB» للجهاز عبر Zadig، أو ثبّت عبر Odin (الملف ultar-…-odin-AP.tar في خانة «AP»)." },
            new[] { "Es wurde nichts geschrieben. Download-Modus verlassen: Leiser + Ein/Aus etwa 7 Sekunden halten.", "Nothing was written. To leave download mode: hold Volume Down + Power for about 7 seconds.", "لم يُكتب أي شيء. للخروج من وضع التنزيل: اضغط خفض الصوت + زر التشغيل نحو 7 ثوانٍ." },
            new[] { "Download-Modus gefunden an {1}.", "Download mode found on {1}.", "تم العثور على وضع التنزيل على {1}." },
            new[] { "Kein Samsung-Gerät im Download-Modus gefunden (kein passender COM-Port).", "No Samsung device found in download mode (no matching COM port).", "لم يُعثر على جهاز سامسونج في وضع التنزيل (لا يوجد منفذ COM مناسب)." },
            new[] { "Alle Abbilder wurden vom Handy bestätigt. Nur der Abschluss der Sitzung schlug fehl: {1}", "All images were confirmed by the phone. Only closing the session failed: {1}", "أكّد الهاتف جميع الصور. فشل فقط إنهاء الجلسة: {1}" },
            new[] { "Handy bitte von Hand neu starten: Leiser + Ein/Aus etwa 7 Sekunden halten.", "Please restart the phone by hand: hold Volume Down + Power for about 7 seconds.", "أعد تشغيل الهاتف يدويًا: اضغط خفض الصوت + زر التشغيل نحو 7 ثوانٍ." },
            new[] { "Suche Ultar OS am USB (serielle Schnittstelle „Ultar OS“) …", "Looking for Ultar OS on USB (serial port “Ultar OS”) …", "جارٍ البحث عن Ultar OS على USB (المنفذ التسلسلي «Ultar OS») …" },
            new[] { "Ultar OS gefunden an {1}. Lese Diagnose …", "Ultar OS found on {1}. Reading diagnostics …", "تم العثور على Ultar OS على {1}. جارٍ قراءة التشخيص …" },
            new[] { "Ultar OS meldet sich nicht am USB.", "Ultar OS does not answer on USB.", "Ultar OS لا يستجيب عبر USB." },
            new[] { "Das heißt: Ultar OS läuft auf dem Handy nicht (oder dessen USB-Anschluss wurde nicht eingeschaltet).", "This means: Ultar OS is not running on the phone (or its USB port was not switched on).", "هذا يعني: Ultar OS لا يعمل على الهاتف (أو لم يُفعَّل منفذ USB فيه)." },
            new[] { "Ultar OS läuft auf dem Handy. Diagnose gespeichert: {1}", "Ultar OS is running on the phone. Diagnostics saved: {1}", "Ultar OS يعمل على الهاتف. حُفظ التشخيص: {1}" },
            new[] { "Neues Ultar-Kernsystem gefunden – baue das Paket neu …", "New Ultar core system found – rebuilding the package …", "تم العثور على نظام Ultar أساسي جديد – جارٍ إعادة بناء الحزمة …" },
            new[] { "Hinweis: Es liegt ein neueres Ultar-Kernsystem vor. Bitte „Ultar-Paket bauen …“ erneut ausführen.", "Note: a newer Ultar core system is available. Please run “Build Ultar package …” again.", "ملاحظة: يوجد نظام Ultar أساسي أحدث. نفّذ «بناء حزمة Ultar …» مرة أخرى." },
            new[] { "Das Handy hat die Abbilder beim Abschluss abgelehnt: {1}", "The phone rejected the images at the end of the session: {1}", "رفض الهاتف الصور عند إنهاء الجلسة: {1}" },
            new[] { "Steht am Handy oben „Only official released binaries are allowed to be flashed“, sperrt Samsung eigene Abbilder noch (KG STATE: Prenormal).", "If the phone shows “Only official released binaries are allowed to be flashed”, Samsung still blocks custom images (KG STATE: Prenormal).", "إذا ظهر على الهاتف «Only official released binaries are allowed to be flashed» فسامسونج ما زالت تمنع الصور المخصصة (KG STATE: Prenormal)." },
            new[] { "Abhilfe: Android starten, mit dem Internet verbinden und warten, bis in den Entwickleroptionen „OEM-Entsperrung“ erscheint; im Download-Modus muss „KG STATE: Checking“ stehen.", "Fix: boot Android, connect to the internet and wait until “OEM unlocking” appears in developer options; download mode must show “KG STATE: Checking”.", "الحل: شغّل أندرويد واتصل بالإنترنت وانتظر حتى يظهر «إلغاء قفل OEM» في خيارات المطوّر؛ ويجب أن يظهر في وضع التنزيل «KG STATE: Checking»." },
            new[] { "Handy jetzt von Hand neu starten: Leiser + Ein/Aus etwa 7 Sekunden halten.", "Restart the phone by hand now: hold Volume Down + Power for about 7 seconds.", "أعد تشغيل الهاتف يدويًا الآن: اضغط خفض الصوت + زر التشغيل نحو 7 ثوانٍ." },
            new[] { "WARNUNG: Handy nicht über adb erkannt (Download-Modus?). Gerät laut Sicherungsordner: {1}. Als Testgerät erlaubt – Akku und Bootloader-Zustand sind ungeprüft.", "WARNING: phone not detected via adb (download mode?). Device according to the backup folder: {1}. Allowed as a test device – battery and bootloader state are unchecked.", "تحذير: لم يُكتشف الهاتف عبر adb (وضع التنزيل؟). الجهاز حسب مجلد النسخ: {1}. مسموح كجهاز اختبار – البطارية وحالة محمّل الإقلاع غير مفحوصتين." },
            new[] { "Boot-Abbild: Ramdisk-Ladeadresse {1}.", "Boot image: ramdisk load address {1}.", "صورة الإقلاع: عنوان تحميل الـ ramdisk هو {1}." },
            new[] { "Versuchte Anschlüsse: {1}", "Ports tried: {1}", "المنافذ التي جُرّبت: {1}" },
            new[] { "Partitionstabelle gelesen: {1} Einträge.", "Partition table read: {1} entries.", "تمت قراءة جدول الأقسام: {1} مدخلًا." },
            new[] { "Schreibe {1} …", "Writing {1} …", "جارٍ كتابة {1} …" },
            new[] { "Es wurde nichts geschrieben.", "Nothing was written.", "لم يُكتب أي شيء." },
            new[] { "Der Schreibvorgang wurde unterbrochen. Das Handy kann jetzt möglicherweise nicht starten – über den Download-Modus wiederherstellen.", "Writing was interrupted. The phone may not boot now – restore it via download mode.", "انقطعت الكتابة. قد لا يقلع الهاتف الآن – استعده عبر وضع التنزيل." },
            new[] { "Download-Modus verlassen: Leiser + Ein/Aus etwa 7 Sekunden halten. Ausweg: mit Odin flashen (ultar-…-odin-AP.tar bei „AP“).", "To leave download mode: hold Volume Down + Power for about 7 seconds. Alternative: flash with Odin (ultar-…-odin-AP.tar under “AP”).", "للخروج من وضع التنزيل: اضغط خفض الصوت + زر التشغيل نحو 7 ثوانٍ. البديل: التثبيت عبر Odin (الملف ultar-…-odin-AP.tar في خانة «AP»)." },
            new[] { "Das Handy antwortet nicht mehr (Verbindung im Download-Modus abgebrochen).", "The phone no longer responds (connection in download mode lost).", "الهاتف لم يعد يستجيب (انقطع الاتصال في وضع التنزيل)." },
            new[] { "Unerwartete Antwort des Handys bei \"{1}\" (Typ {2} statt {3}).", "Unexpected reply from the phone at \"{1}\" (type {2} instead of {3}).", "رد غير متوقع من الهاتف عند \"{1}\" (النوع {2} بدل {3})." },
            new[] { "Das Handy hat die Begrüßung nicht erwidert (\"{1}\" statt LOKE).", "The phone did not return the greeting (\"{1}\" instead of LOKE).", "الهاتف لم يرد التحية (\"{1}\" بدل LOKE)." },
            new[] { "Das Handy meldet einen Fehler beim Schreiben von {1}.", "The phone reports an error while writing {1}.", "الهاتف يبلّغ عن خطأ أثناء كتابة {1}." },
            new[] { "Das Handy hat Teil {1} von {2} nicht bestätigt.", "The phone did not confirm part {1} of {2}.", "الهاتف لم يؤكد الجزء {1} من {2}." },
            // Paketbau
            new[] { "Lese Firmware {1} …", "Reading firmware {1} …", "جارٍ قراءة ملف البرنامج {1} …" },
            new[] { "Original-Boot-Abbild: Version {1}, Kernel {2} KiB, Ramdisk {3} KiB, Seitengröße {4}", "Original boot image: version {1}, kernel {2} KiB, ramdisk {3} KiB, page size {4}", "صورة الإقلاع الأصلية: الإصدار {1}، النواة {2} ك.بايت، القرص المؤقت {3} ك.بايت، حجم الصفحة {4}" },
            new[] { "Kernel: \"skip_initramfs\" an {1} Stelle(n) umbenannt, damit das Ultar-Ramdisk gestartet wird.", "Kernel: \"skip_initramfs\" renamed in {1} place(s) so that the Ultar ramdisk is started.", "النواة: أُعيدت تسمية \"skip_initramfs\" في {1} موضع ليُشغَّل نظام Ultar." },
            new[] { "Kernel enthält kein \"skip_initramfs\" – keine Änderung am Kernel nötig{1}.", "The kernel contains no \"skip_initramfs\" – no kernel change needed{1}.", "النواة لا تحتوي على \"skip_initramfs\" – لا حاجة لتعديل النواة{1}." },
            new[] { "Hinweis: gzip-gepackter Kernel wurde neu gepackt; ein angehängter Gerätebaum ginge dabei verloren.", "Note: the gzip-compressed kernel was repacked; an appended device tree would be lost.", "ملاحظة: أُعيد ضغط النواة (gzip)؛ قد تُفقد شجرة الأجهزة الملحقة." },
            new[] { "vbmeta: durch leeres vbmeta ersetzt (Prüfung aus, Flags = 2).", "vbmeta: replaced by an empty vbmeta (verification off, flags = 2).", "vbmeta: تم تعطيل التحقق (Flags = 3)." },
            new[] { "Hinweis: keine vbmeta.img in der Quelle. Prüft das Gerät Abbilder (AVB), startet das Paket ohne geänderte vbmeta nicht.", "Note: no vbmeta.img in the source. If the device verifies images (AVB), the package will not boot without a modified vbmeta.", "ملاحظة: لا يوجد vbmeta.img في المصدر. إن كان الجهاز يتحقق من الصور (AVB) فلن تقلع الحزمة دون vbmeta معدّل." },
            new[] { "Originale ({1}) für die Wiederherstellung abgelegt: {2}", "Originals ({1}) stored for restoring: {2}", "حُفظت النسخ الأصلية ({1}) للاستعادة: {2}" },
            new[] { "Paket fertig: {1}", "Package ready: {1}", "الحزمة جاهزة: {1}" },
            new[] { "WARNUNG: Firmware-Datei passt dem Namen nach nicht zum Stand auf dem Handy ({1}). Für die Wiederherstellung sollte es dieselbe Version sein.", "WARNING: by its name the firmware file does not match the version on the phone ({1}). For restoring it should be the same version.", "تحذير: اسم ملف البرنامج لا يطابق إصدار الهاتف ({1}). للاستعادة يجب أن يكون الإصدار نفسه." },
            new[] { "Kein Android-Boot-Abbild (Kennung ANDROID! fehlt).", "Not an Android boot image (ANDROID! signature missing).", "ليست صورة إقلاع أندرويد (التوقيع ANDROID! مفقود)." },
            new[] { "Boot-Abbild Version {1} wird nicht unterstützt (nur 0–2).", "Boot image version {1} is not supported (only 0–2).", "إصدار صورة الإقلاع {1} غير مدعوم (فقط 0–2)." },
            new[] { "Boot-Abbild abgeschnitten.", "Boot image is truncated.", "صورة الإقلاع مبتورة." },
            new[] { "Boot-Abbild enthält keinen Kernel.", "The boot image contains no kernel.", "صورة الإقلاع لا تحتوي على نواة." },
            new[] { "vbmeta.img hat nicht das erwartete Format (AVB0).", "vbmeta.img does not have the expected format (AVB0).", "الملف vbmeta.img ليس بالصيغة المتوقعة (AVB0)." },
            new[] { "Im Sicherungsordner fehlt partitions/boot.img (Vollsicherung braucht Root). Stattdessen die Firmware-Datei AP_….tar.md5 wählen.", "partitions/boot.img is missing in the backup folder (a full backup needs root). Choose the firmware file AP_….tar.md5 instead.", "الملف partitions/boot.img مفقود في مجلد النسخ (النسخ الكامل يحتاج روت). اختر ملف البرنامج AP_….tar.md5 بدلًا منه." },
            new[] { "In der Firmware-Datei ist keine boot.img. Bitte die AP-Datei wählen (AP_….tar.md5).", "There is no boot.img in the firmware file. Please choose the AP file (AP_….tar.md5).", "لا يوجد boot.img في ملف البرنامج. اختر ملف AP (‏AP_….tar.md5)." },
            new[] { "Das neue Boot-Abbild ({1} KiB) ist größer als das Original ({2} KiB) und passt nicht in die Partition.", "The new boot image ({1} KiB) is larger than the original ({2} KiB) and does not fit into the partition.", "صورة الإقلاع الجديدة ({1} ك.بايت) أكبر من الأصلية ({2} ك.بايت) ولا تتسع في القسم." },
            new[] { "Kein LZ4-Frame.", "Not an LZ4 frame.", "ليس ملف LZ4 صالحًا." },
            new[] { "LZ4: Datei abgeschnitten.", "LZ4: file is truncated.", "LZ4: الملف مبتور." },
            new[] { "tar abgeschnitten", "tar archive is truncated", "أرشيف tar مبتور" },
            // Updates
            new[] { "Lade {1} …", "Downloading {1} …", "جارٍ تنزيل {1} …" },
            new[] { "Prüfsumme stimmt.", "Checksum matches.", "المجموع الاختباري مطابق." },
            new[] { "Das Release enthält keine MSI-Datei.", "The release contains no MSI file.", "الإصدار لا يحتوي على ملف MSI." },
            new[] { "Das Release enthält keine SHA256SUMS – Installation abgelehnt.", "The release contains no SHA256SUMS – installation refused.", "الإصدار لا يحتوي على SHA256SUMS – رُفض التثبيت." },
            new[] { "SHA256SUMS enthält keinen Eintrag für {1}.", "SHA256SUMS has no entry for {1}.", "الملف SHA256SUMS لا يحتوي على {1}." },
            new[] { "Prüfsumme der MSI-Datei stimmt nicht – Installation abgelehnt.", "The checksum of the MSI file does not match – installation refused.", "المجموع الاختباري لملف MSI غير مطابق – رُفض التثبيت." },
            new[] { "Update: {1}  (Projekt ist privat – GitHub-Token eintragen)", "Update: {1}  (the project is private – enter a GitHub token)", "التحديث: {1}  (المشروع خاص – أدخل رمز GitHub)" },
            new[] { "Update: {1}", "Update: {1}", "التحديث: {1}" },
        };

        static List<KeyValuePair<Regex, string[]>> rules;

        static void Init()
        {
            rules = new List<KeyValuePair<Regex, string[]>>();
            foreach (var m in M)
            {
                // Muster in einen regulären Ausdruck wandeln: Text wörtlich, {n} = beliebiger Teil
                var pat = Regex.Replace(Regex.Escape(m[0]), @"\\\{(\d)}", "(?<g$1>.+?)");
                rules.Add(new KeyValuePair<Regex, string[]>(new Regex("^" + pat + "$", RegexOptions.Singleline), m));
            }
        }

        /// Eine Meldung in die gewählte Sprache übertragen (unbekannte Texte bleiben unverändert, z. B. Ausgaben von adb/heimdall).
        public static string Line(string s)
        {
            if (T.L == 0 || string.IsNullOrEmpty(s)) return s;
            if (rules == null) Init();
            // Einrückung und Aufzählungspunkt behalten
            int i = 0; while (i < s.Length && (s[i] == ' ' || s[i] == '•')) i++;
            string pre = s.Substring(0, i), body = s.Substring(i);
            foreach (var r in rules)
            {
                var m = r.Key.Match(body);
                if (!m.Success) continue;
                string outp = r.Value[T.L];
                for (int g = 1; g <= 4; g++)
                {
                    var grp = m.Groups["g" + g];
                    if (grp.Success) outp = outp.Replace("{" + g + "}", Line(grp.Value)); // innere Meldung (z. B. Fehlertext) ebenfalls übertragen
                }
                return pre + outp;
            }
            return s;
        }
    }
}
