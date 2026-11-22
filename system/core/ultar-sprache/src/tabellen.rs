//! Übersetzungstabellen: (Deutsch, Englisch, Arabisch). Längste Wendung gewinnt; Platzhalter-Werte bleiben stehen.

pub static ALLE: &[&[(&str, &str, &str)]] = &[HILFEN, ZELLE, VERBUND, ALLGEMEIN];

/// Befehlswörter: Deutsch (intern), Englisch, Arabisch
pub static BEFEHLE: &[(&str, &str, &str)] = &[
    ("neu", "new", "جديد"),
    ("liste", "list", "قائمة"),
    ("zeige", "show", "عرض"),
    ("loeschen", "delete", "حذف"),
    ("setze", "set", "ضبط"),
    ("start", "run", "تشغيل"),
    ("stopp", "stop", "إيقاف"),
    ("verknuepfen", "link", "ربط"),
    ("trennen", "unlink", "فصل"),
    ("verknuepfen-container", "link-container", "ربط-حاوية"),
    ("protokoll", "log", "سجل"),
    ("systemprotokoll", "syslog", "سجل-النظام"),
    ("empfehlung", "suggest", "اقتراح"),
    ("modus", "mode", "وضع"),
    ("einrichten", "setup", "إعداد"),
    ("netz", "net", "شبكة"),
    ("erlauben", "allow", "سماح"),
    ("verbieten", "deny", "منع"),
    ("vorgabe", "default", "افتراضي"),
    ("geraet", "device", "الجهاز"),
    ("standort", "location", "الموقع"),
    ("an", "on", "تفعيل"),
    ("aus", "off", "تعطيل"),
    ("frei", "open", "مفتوح"),
    ("hilfe", "help", "مساعدة"),
    // Verbund
    ("geraete", "devices", "الأجهزة"),
    ("koppeln", "pair", "اقتران"),
    ("entkoppeln", "unpair", "إلغاء-الاقتران"),
    ("ausfuehren", "exec", "تنفيذ"),
    ("verteilt", "distribute", "توزيع"),
    ("anzeigen", "view", "مشاهدة"),
    ("bildschirm", "screen", "الشاشة"),
    ("fern", "remote", "تحكم"),
    ("senden", "send", "إرسال"),
    ("weitergeben", "handoff", "تمرير"),
    ("hoeren", "listen", "استماع"),
    ("sprechen", "speak", "تحدث"),
    ("konto", "account", "الحساب"),
    ("kerne", "cores", "الأنوية"),
    ("relais", "relay", "المرحّل"),
    ("dienst", "service", "خدمة"),
];

pub const ZELLE_HILFE_DE: &str = "zelle – Apps in abgeschotteten Zellen (eigener Speicher, Netz/VPN, Privatsphäre, Protokoll)

  zelle neu <name> [--app ID | -- befehl …] [Optionen]
      --netz frei|aus|ueber:<vpn-zelle>|wireguard   --vpn-zelle <schnittstelle>   (diese Zelle IST das VPN)
      --wg-conf DATEI --wg-adresse IP/NN --wg-endpunkt IP:PORT
      --protokoll  --uploads   --gpu auto|alle|aus|N   --ram MB (0 = dynamisch)  --ram-garantiert MB  --max-gb N
      --standort --geraeteinfo --kamera --mikrofon --usb   (erlauben; Standard: alles aus)   --wurzel /compat
      --ort BREITE,LAENGE  --zeitzone Europe/Berlin  --sprache ar   (vorgegebene Identität der App)
  zelle liste [--json] | zeige <name> | loeschen <name>
  zelle setze <name> <feld> <wert>     z. B. netz.art ueber · netz.ueber vpn · protokoll.uploads true · gpu 1
  zelle start <name> [-- argumente]    zelle stopp <name>
  zelle verknuepfen <a> <b> | trennen <a> <b>      gemeinsamer Ordner + gegenseitig erreichbar
  zelle verknuepfen-container <a> <b>              Kiri-/Docker-Container: gemeinsames Netz + Volume
  zelle protokoll <name> [--n 50] [--json]         Verbindungen, DNS, Bytes, Uploads
  zelle systemprotokoll [--runden N]               alle Verbindungen aller Programme
  zelle empfehlung <datei|paket|abbild> [--json]   gibt es das schon? verknüpfen statt doppelt
  zelle gpu [--json] | gpu modus auto|alle|aus|N   Grafikkarten
  zelle kiri status|einrichten|container           Kiri (Docker-Ersatz)
  zelle netz [--json]                              Internet-Regeln anzeigen
  zelle netz geraet an|aus                         Internet für das Gerät selbst (Weiterleitung für Zellen bleibt)
  zelle netz vorgabe frei|aus                      Netz neuer Zellen
  zelle netz erlauben|verbieten <dienst>           Systemdienst trotz „Gerät aus“ ins Internet lassen

Befehle gehen auch englisch (new, list, run, stop, link, log, net …) und arabisch (جديد، قائمة، تشغيل …).";

pub const ZELLE_HILFE_EN: &str = "zelle – apps in sealed cells (own storage, network/VPN, privacy, log)

  zelle new <name> [--app ID | -- command …] [options]
      --netz frei|aus|ueber:<vpn-cell>|wireguard   --vpn-zelle <interface>   (this cell IS the VPN)
      --wg-conf FILE --wg-adresse IP/NN --wg-endpunkt IP:PORT
      --protokoll  --uploads   --gpu auto|alle|aus|N   --ram MB (0 = dynamic)  --ram-garantiert MB  --max-gb N
      --standort --geraeteinfo --kamera --mikrofon --usb   (allow; default: all off)   --wurzel /compat
      --ort LAT,LON  --zeitzone Europe/Berlin  --sprache ar   (identity the app is given)
  zelle list [--json] | show <name> | delete <name>
  zelle set <name> <field> <value>     e.g. netz.art ueber · netz.ueber vpn · protokoll.uploads true · gpu 1
  zelle run <name> [-- arguments]      zelle stop <name>
  zelle link <a> <b> | unlink <a> <b>              shared folder + reachable by name
  zelle link-container <a> <b>                     Kiri/Docker containers: shared network + volume
  zelle log <name> [--n 50] [--json]               connections, DNS, bytes, uploads
  zelle syslog [--runden N]                        all connections of all programs
  zelle suggest <file|package|image> [--json]      already there? link instead of installing twice
  zelle gpu [--json] | gpu mode auto|alle|aus|N    graphics cards
  zelle kiri status|setup|container                Kiri (Docker replacement)
  zelle net [--json]                               show internet rules
  zelle net device on|off                          internet for the device itself (forwarding for cells stays)
  zelle net default open|off                       network of new cells
  zelle net allow|deny <service>                   let a system service online while the device is off

Commands also work in German (neu, liste, start …) and Arabic (جديد، قائمة، تشغيل …).";

pub const ZELLE_HILFE_AR: &str = "zelle – تطبيقات في خلايا معزولة (تخزين خاص، شبكة/VPN، خصوصية، سجل)

  zelle جديد <الاسم> [--app المعرّف | -- أمر …] [خيارات]
      --netz frei|aus|ueber:<خلية-vpn>|wireguard   --vpn-zelle <الواجهة>   (هذه الخلية هي الـ VPN)
      --wg-conf ملف --wg-adresse IP/NN --wg-endpunkt IP:PORT
      --protokoll  --uploads   --gpu auto|alle|aus|N   --ram MB (0 = ديناميكي)  --ram-garantiert MB  --max-gb N
      --standort --geraeteinfo --kamera --mikrofon --usb   (سماح؛ الافتراضي: الكل مغلق)   --wurzel /compat
      --ort خط_العرض,خط_الطول  --zeitzone Asia/Damascus  --sprache ar   (هوية تُعطى للتطبيق)
  zelle قائمة [--json] | عرض <الاسم> | حذف <الاسم>
  zelle ضبط <الاسم> <الحقل> <القيمة>     مثلاً netz.art ueber · netz.ueber vpn · protokoll.uploads true · gpu 1
  zelle تشغيل <الاسم> [-- وسائط]        zelle إيقاف <الاسم>
  zelle ربط <أ> <ب> | فصل <أ> <ب>              مجلد مشترك + الوصول المتبادل بالاسم
  zelle ربط-حاوية <أ> <ب>                     حاويات Kiri/Docker: شبكة مشتركة + وحدة تخزين
  zelle سجل <الاسم> [--n 50] [--json]           الاتصالات، DNS، البايتات، الرفع
  zelle سجل-النظام [--runden N]                كل اتصالات كل البرامج
  zelle اقتراح <ملف|حزمة|صورة> [--json]         هل هو موجود؟ اربط بدل التثبيت مرتين
  zelle gpu [--json] | gpu وضع auto|alle|aus|N   بطاقات الرسوميات
  zelle kiri status|إعداد|container            Kiri (بديل Docker)
  zelle شبكة [--json]                          عرض قواعد الإنترنت
  zelle شبكة الجهاز تفعيل|تعطيل            إنترنت الجهاز نفسه (تمرير الخلايا يبقى)
  zelle شبكة افتراضي مفتوح|تعطيل               شبكة الخلايا الجديدة
  zelle شبكة سماح|منع <خدمة>                   السماح لخدمة نظام بالإنترنت رغم إطفاء الجهاز

الأوامر تعمل أيضاً بالألمانية (neu, liste …) وبالإنجليزية (new, list, run …).";

pub const VERBUND_HILFE_DE: &str = "ultar-verbund – Ultar-Geräte koppeln, steuern und Leistung bündeln

  ultar-verbund an | aus               Verbund ein-/ausschalten
  ultar-verbund status                 eigenes Gerät, Relais, Zähler
  ultar-verbund geraete                gefundene Geräte (Netz + Relais)
  ultar-verbund koppeln [--rolle rechnen|voll]
                                       Code anzeigen, anderes Gerät gibt ihn ein
  ultar-verbund koppeln <gerät> <code> mit einem Gerät koppeln (Kennung, Name oder ip:port)
  ultar-verbund entkoppeln <gerät>     Vertrauen entziehen
  ultar-verbund konto anmelden <benutzer>   (Passwort aus ULTAR_PASSWORT oder Eingabe)
  ultar-verbund konto token <token> | abgleichen | abmelden
  ultar-verbund name <name> | relais <url> | kerne <n> | ziel <ip[:port]>
  ultar-verbund erlauben steuern|rechnen|bildschirm|ton|dateien an|aus
  ultar-verbund ausfuehren <gerät> -- befehl …   Befehl auf einem Gerät ausführen (steuern)
  ultar-verbund verteilt [-n N] [--zeilen] [--ohne-lokal] [--json] -- befehl … {}
                                       Aufgaben über alle Geräte verteilen ({} = Nummer/Zeile)
  ultar-verbund bench [-n N] [-a ITER] Leistung: nur hier vs. ganzer Verbund
  ultar-verbund anzeigen <gerät> [--web ip:port] [--oeffnen]
                                       Bildschirm eines Geräts im Browser sehen und bedienen
  ultar-verbund foto <gerät> <bild.bmp>       Bildschirmfoto eines Geräts
  ultar-verbund bildschirm <gerät> hell N | drehen 0|90|180|270 | an | aus | oeffne NAME | helfer TEXT
  ultar-verbund fern <gerät> <zeile>…  Eingaben schicken (ab x y | zieh x y | auf | zeichen T | enter …)
  ultar-verbund senden <gerät> <datei> [--nicht-oeffnen]   Datei/Bild weitergeben (dort öffnen)
  ultar-verbund weitergeben <gerät> [datei]  Datei dort öffnen – ohne Datei: dieses Gerät dort anzeigen
  ultar-verbund hoeren <gerät> [sek]   Mikrofon des Geräts hier abspielen
  ultar-verbund sprechen <gerät> [sek] hier sprechen, dort abspielen (Durchsage)
  ultar-verbund mining status|an|aus|einrichten|wallet ADR|pool HOST:PORT|kerne PROZENT
  ultar-verbund mining alle            Mining-Einstellungen auf alle gekoppelten Geräte bringen und einrichten
  ultar-verbund arbeit <i> <iter>      Rechenarbeit (SHA-256-Kette) für Tests
  ultar-verbund dienst                 Dienst starten (ultar-init / systemd)";

pub const VERBUND_HILFE_EN: &str = "ultar-verbund – pair Ultar devices, control them and pool their power

  ultar-verbund on | off               switch the cluster on/off
  ultar-verbund status                 own device, relay, counters
  ultar-verbund devices                devices found (network + relay)
  ultar-verbund pair [--rolle rechnen|voll]
                                       show a code, the other device enters it
  ultar-verbund pair <device> <code>   pair with a device (ID, name or ip:port)
  ultar-verbund unpair <device>        revoke trust
  ultar-verbund account anmelden <user>   (password from ULTAR_PASSWORT or prompt)
  ultar-verbund account token <token> | abgleichen | abmelden
  ultar-verbund name <name> | relay <url> | cores <n> | ziel <ip[:port]>
  ultar-verbund allow steuern|rechnen|bildschirm|ton|dateien on|off
  ultar-verbund exec <device> -- command …   run a command on a device (control)
  ultar-verbund distribute [-n N] [--zeilen] [--ohne-lokal] [--json] -- command … {}
                                       spread tasks over all devices ({} = number/line)
  ultar-verbund bench [-n N] [-a ITER] performance: this device only vs. whole cluster
  ultar-verbund view <device> [--web ip:port] [--oeffnen]
                                       see and operate a device's screen in the browser
  ultar-verbund foto <device> <image.bmp>     screenshot of a device
  ultar-verbund screen <device> hell N | drehen 0|90|180|270 | an | aus | oeffne NAME | helfer TEXT
  ultar-verbund remote <device> <line>…  send input (ab x y | zieh x y | auf | zeichen T | enter …)
  ultar-verbund send <device> <file> [--nicht-oeffnen]   hand over a file/image (opens there)
  ultar-verbund handoff <device> [file]  open the file there – without a file: show this device there
  ultar-verbund listen <device> [sec]  play the device's microphone here
  ultar-verbund speak <device> [sec]   speak here, play there (announcement)
  ultar-verbund mining status|an|aus|einrichten|wallet ADR|pool HOST:PORT|kerne PERCENT
  ultar-verbund mining alle            bring mining settings to all paired devices and set them up
  ultar-verbund arbeit <i> <iter>      computing work (SHA-256 chain) for tests
  ultar-verbund service                start the service (ultar-init / systemd)";

pub const VERBUND_HILFE_AR: &str = "ultar-verbund – اقتران أجهزة Ultar والتحكم بها وجمع قوتها

  ultar-verbund تفعيل | تعطيل             تشغيل/إطفاء الاتحاد
  ultar-verbund status                 الجهاز الحالي، المرحّل، العدّادات
  ultar-verbund الأجهزة                 الأجهزة المكتشفة (الشبكة + المرحّل)
  ultar-verbund اقتران [--rolle rechnen|voll]
                                       عرض رمز يُدخله الجهاز الآخر
  ultar-verbund اقتران <الجهاز> <الرمز>  الاقتران مع جهاز (المعرّف أو الاسم أو ip:port)
  ultar-verbund إلغاء-الاقتران <الجهاز>  سحب الثقة
  ultar-verbund الحساب anmelden <المستخدم>   (كلمة المرور من ULTAR_PASSWORT أو إدخالها)
  ultar-verbund الحساب token <الرمز> | abgleichen | abmelden
  ultar-verbund name <الاسم> | المرحّل <url> | الأنوية <n> | ziel <ip[:port]>
  ultar-verbund سماح steuern|rechnen|bildschirm|ton|dateien an|aus
  ultar-verbund تنفيذ <الجهاز> -- أمر …   تنفيذ أمر على جهاز (تحكم)
  ultar-verbund توزيع [-n N] [--zeilen] [--ohne-lokal] [--json] -- أمر … {}
                                       توزيع المهام على كل الأجهزة ({} = الرقم/السطر)
  ultar-verbund bench [-n N] [-a ITER] الأداء: هذا الجهاز فقط مقابل الاتحاد كله
  ultar-verbund مشاهدة <الجهاز> [--web ip:port] [--oeffnen]
                                       رؤية شاشة جهاز والتحكم بها في المتصفح
  ultar-verbund foto <الجهاز> <صورة.bmp>      لقطة شاشة لجهاز
  ultar-verbund الشاشة <الجهاز> hell N | drehen 0|90|180|270 | an | aus | oeffne الاسم | helfer نص
  ultar-verbund تحكم <الجهاز> <سطر>…    إرسال مدخلات (ab x y | zieh x y | auf | zeichen T | enter …)
  ultar-verbund إرسال <الجهاز> <ملف> [--nicht-oeffnen]   تمرير ملف/صورة (يُفتح هناك)
  ultar-verbund تمرير <الجهاز> [ملف]   فتح الملف هناك – بدون ملف: عرض هذا الجهاز هناك
  ultar-verbund استماع <الجهاز> [ثانية] تشغيل ميكروفون الجهاز هنا
  ultar-verbund تحدث <الجهاز> [ثانية]  التحدث هنا والتشغيل هناك (إعلان)
  ultar-verbund mining status|an|aus|einrichten|wallet عنوان|pool HOST:PORT|kerne نسبة
  ultar-verbund mining alle            نقل إعدادات التعدين لكل الأجهزة المقترنة وإعدادها
  ultar-verbund arbeit <i> <iter>      عمل حسابي (سلسلة SHA-256) للاختبار
  ultar-verbund خدمة                    تشغيل الخدمة (ultar-init / systemd)";

static HILFEN: &[(&str, &str, &str)] = &[
    (ZELLE_HILFE_DE, ZELLE_HILFE_EN, ZELLE_HILFE_AR),
    (VERBUND_HILFE_DE, VERBUND_HILFE_EN, VERBUND_HILFE_AR),
];

static ZELLE: &[(&str, &str, &str)] = &[
    ("Zelle „", "Cell „", "الخلية „"),
    (
        "“ angelegt: Netz ",
        "“ created: network ",
        "“ أُنشئت: الشبكة ",
    ),
    (", Protokoll ", ", log ", "، السجل "),
    (", Standort ", ", location ", "، الموقع "),
    (", Geräteinfo ", ", device info ", "، معلومات الجهاز "),
    ("“ gibt es nicht", "“ does not exist", "“ غير موجودة"),
    ("“ gibt es schon", "“ already exists", "“ موجودة مسبقاً"),
    ("[zelle] ", "[cell] ", "[خلية] "),
    (" läuft: Netz ", " running: network ", " تعمل: الشبكة "),
    (", Grafik: ", ", graphics: ", "، الرسوميات: "),
    (
        "keine Grafikkarte gefunden",
        "no graphics card found",
        "لم يُعثر على بطاقة رسوميات",
    ),
    (
        "keine Grafikkarte",
        "no graphics card",
        "لا توجد بطاقة رسوميات",
    ),
    ("Grafik aus", "graphics off", "الرسوميات مطفأة"),
    (
        "freiere Karte",
        "card with more free memory",
        "البطاقة الأكثر فراغاً",
    ),
    ("Karte ", "Card ", "البطاقة "),
    (" MB frei", " MB free", " MB متاحة"),
    (" % belegt", " % busy", " % مشغولة"),
    ("alle ", "all ", "كل الـ "),
    (
        " Karten gemeinsam (Aufteilung ",
        " cards together (split ",
        " بطاقات معاً (التقسيم ",
    ),
    ("fest Karte ", "fixed card ", "بطاقة ثابتة "),
    ("Vorgabe: ", "Default: ", "الافتراضي: "),
    (" gestoppt", " stopped", " أُوقفت"),
    (
        ": gemeinsamer Ordner /data/gemeinsam/<andere> (",
        ": shared folder /data/gemeinsam/<other> (",
        ": مجلد مشترك /data/gemeinsam/<الأخرى> (",
    ),
    (
        "), erreichbar als „",
        "), reachable as „",
        ")، يمكن الوصول إليها باسم „",
    ),
    ("“ bzw. „", "“ and „", "“ و„"),
    (
        " getrennt (gemeinsamer Ordner bleibt erhalten: ",
        " unlinked (shared folder is kept: ",
        " فُصلت (المجلد المشترك يبقى: ",
    ),
    (
        " Einträge, gesendet zusammen ",
        " entries, sent in total ",
        " إدخال، المُرسل إجمالاً ",
    ),
    (" Bytes", " bytes", " بايت"),
    ("kein Protokoll für ", "no log for ", "لا يوجد سجل لـ "),
    (" gibt es schon:", " is already there:", " موجود مسبقاً:"),
    ("Empfehlung: ", "Suggestion: ", "اقتراح: "),
    (
        "Doppelt installieren bleibt möglich.",
        "Installing twice is still possible.",
        "يبقى التثبيت مرتين ممكناً.",
    ),
    ("Die Zelle „", "The cell „", "الخلية „"),
    (
        "“ hat das schon. Verknüpfen (gemeinsamer Ordner, gegenseitig erreichbar): ",
        "“ already has it. Link (shared folder, reachable by name): ",
        "“ لديها ذلك. اربط (مجلد مشترك، وصول متبادل): ",
    ),
    ("Der Container „", "The container „", "الحاوية „"),
    (
        "“ läuft schon. Verknüpfen statt einen zweiten: ",
        "“ is already running. Link instead of a second one: ",
        "“ تعمل مسبقاً. اربط بدل حاوية ثانية: ",
    ),
    (
        "nichts Vergleichbares vorhanden – Installation ohne Bedenken",
        "nothing similar installed – go ahead",
        "لا يوجد شيء مشابه – التثبيت بلا مشكلة",
    ),
    (
        "Speicher der Zelle voll (",
        "Cell storage full (",
        "تخزين الخلية ممتلئ (",
    ),
    (
        " GB) – Grenze erhöhen: ",
        " GB) – raise the limit: ",
        " GB) – ارفع الحد: ",
    ),
    (
        "Name: nur a-z, 0-9 und -, höchstens 32 Zeichen",
        "Name: only a-z, 0-9 and -, at most 32 characters",
        "الاسم: a-z و0-9 و- فقط، 32 حرفاً كحد أقصى",
    ),
    (
        "Zellen startet nur root (über ultar-init/uapp)",
        "Only root starts cells (via ultar-init/uapp)",
        "لا يشغّل الخلايا إلا root (عبر ultar-init/uapp)",
    ),
    ("unbekanntes Feld ", "unknown field ", "حقل غير معروف "),
    ("Zahl erwartet", "number expected", "يُتوقع رقم"),
    (
        "“ ist keine VPN-Zelle",
        "“ is not a VPN cell",
        "“ ليست خلية VPN",
    ),
    (
        " (--vpn-zelle beim Anlegen)",
        " (--vpn-zelle when creating)",
        " (--vpn-zelle عند الإنشاء)",
    ),
    (
        "--app ID oder -- befehl angeben",
        "give --app ID or -- command",
        "حدّد --app المعرّف أو -- أمر",
    ),
    ("App „", "App „", "التطبيق „"),
    (
        "“ ist nicht installiert",
        "“ is not installed",
        "“ غير مثبّت",
    ),
    (
        "“ hat keinen Startbefehl",
        "“ has no start command",
        "“ ليس له أمر تشغيل",
    ),
    ("Kiri eingerichtet: ", "Kiri set up: ", "Kiri مُعدّ: "),
    (
        "nein (zelle kiri einrichten)",
        "no (zelle kiri setup)",
        "لا (zelle kiri إعداد)",
    ),
    ("Dienst läuft: ", "Service running: ", "الخدمة تعمل: "),
    (
        "Container-Dienst (",
        "Container service (",
        "خدمة الحاويات (",
    ),
    (
        "Prüfsumme falsch – nicht eingerichtet (",
        "Checksum wrong – not set up (",
        "المجموع الاختباري خاطئ – لم يُعدّ (",
    ),
    (
        " eingerichtet (SHA-256 geprüft) – Dienst: ultar-ctl start kiri, Docker-Werkzeuge: ",
        " set up (SHA-256 verified) – service: ultar-ctl start kiri, Docker tools: ",
        " أُعدّ (تم التحقق بـ SHA-256) – الخدمة: ultar-ctl start kiri، أدوات Docker: ",
    ),
    ("Netz der Zelle: ", "Cell network: ", "شبكة الخلية: "),
    ("Zelle starten: ", "Start cell: ", "تشغيل الخلية: "),
    ("Wächter: ", "Guard: ", "الحارس: "),
    // Internet-Regeln
    (
        "Internet für das Gerät selbst: ",
        "Internet for the device itself: ",
        "إنترنت الجهاز نفسه: ",
    ),
    (
        "Netz neuer Zellen: ",
        "Network of new cells: ",
        "شبكة الخلايا الجديدة: ",
    ),
    (
        "Systemdienste mit Internet: ",
        "System services with internet: ",
        "خدمات النظام التي لها إنترنت: ",
    ),
    (
        "Zellen mit Internet: ",
        "Cells with internet: ",
        "الخلايا التي لها إنترنت: ",
    ),
    (
        "Weiterleitung für Zellen bleibt aktiv",
        "forwarding for cells stays active",
        "التمرير للخلايا يبقى فعّالاً",
    ),
    (
        " darf ins Internet (Neustart des Dienstes)",
        " may go online (service restarted)",
        " يُسمح له بالإنترنت (أُعيد تشغيل الخدمة)",
    ),
    (" bleibt offline", " stays offline", " يبقى دون إنترنت"),
    ("Ort ", "Location ", "الموقع "),
    ("Zeitzone ", "time zone ", "المنطقة الزمنية "),
    ("Sprache ", "language ", "اللغة "),
];

static VERBUND: &[(&str, &str, &str)] = &[
    (
        "Verbund ist AUS – zuerst: ultar-verbund an",
        "Cluster is OFF – first: ultar-verbund on",
        "الاتحاد مطفأ – أولاً: ultar-verbund تفعيل",
    ),
    ("Kopplungscode: ", "Pairing code: ", "رمز الاقتران: "),
    (
        "(5 Minuten gültig)",
        "(valid for 5 minutes)",
        "(صالح 5 دقائق)",
    ),
    (
        "Auf dem anderen Gerät: ",
        "On the other device: ",
        "على الجهاز الآخر: ",
    ),
    ("gekoppelt mit „", "paired with „", "تم الاقتران مع „"),
    ("Gerät „", "Device „", "الجهاز „"),
    ("“ nicht gefunden", "“ not found", "“ غير موجود"),
    (
        "keine Geräte gefunden",
        "no devices found",
        "لم يُعثر على أجهزة",
    ),
    (" ist nicht gekoppelt", " is not paired", " غير مقترن"),
    ("nicht gekoppelt", "not paired", "غير مقترن"),
    (
        "nicht erreichbar, gekoppelt über ",
        "not reachable, paired via ",
        "غير متاح، مقترن عبر ",
    ),
    ("entkoppelt", "unpaired", "أُلغي الاقتران"),
    (
        "Kopplung abgebrochen",
        "pairing aborted",
        "أُلغي الاقتران قبل اكتماله",
    ),
    (
        "Konto verbunden – ",
        "Account connected – ",
        "الحساب متصل – ",
    ),
    (
        " weitere Geräte im Konto vertraut",
        " more devices in the account trusted",
        " أجهزة أخرى في الحساب موثوقة",
    ),
    ("Gekoppelt  ", "Paired     ", "مقترن      "),
    (
        " Geräte, erledigte Aufgaben ",
        " devices, tasks done ",
        " أجهزة، المهام المنجزة ",
    ),
    ("Gerät      ", "Device     ", "الجهاز     "),
    ("Dienst     ", "Service    ", "الخدمة     "),
    ("Relais     ", "Relay      ", "المرحّل    "),
    ("Kerne      ", "Cores      ", "الأنوية    "),
    ("Erlaubt    ", "Allowed    ", "مسموح      "),
    ("läuft seit ", "running for ", "يعمل منذ "),
    (
        "Datei zu groß (höchstens 48 MB)",
        "file too large (at most 48 MB)",
        "الملف كبير جداً (48 MB كحد أقصى)",
    ),
    (
        "Gerät ist voll ausgelastet",
        "device is fully busy",
        "الجهاز مشغول بالكامل",
    ),
    ("Zeit abgelaufen", "timed out", "انتهت المهلة"),
    ("unerwartete Antwort", "unexpected answer", "رد غير متوقع"),
    (
        "Antwort nicht echt (Code passt nicht)",
        "answer not genuine (code does not match)",
        "الرد غير موثوق (الرمز غير مطابق)",
    ),
    (
        "keine Geräte zum Rechnen",
        "no devices for computing",
        "لا توجد أجهزة للحساب",
    ),
    ("Ergebnisse falsch", "results wrong", "النتائج خاطئة"),
    ("keine Aufnahme", "no recording", "لا يوجد تسجيل"),
    (
        " Aufgaben mit Fehlercode",
        " tasks with error code",
        " مهام برمز خطأ",
    ),
    ("nur dieses Gerät", "this device only", "هذا الجهاز فقط"),
    ("ganzer Verbund", "whole cluster", "الاتحاد كله"),
    ("Beschleunigung", "Speed-up", "التسريع"),
    ("Aufgaben auf ", "tasks on ", "مهام على "),
    (" Geräte (", " devices (", " أجهزة ("),
    ("fertig in ", "done in ", "انتهى في "),
    ("Ansicht von „", "View of „", "عرض „"),
    (
        "“ – im Browser öffnen: ",
        "“ – open in the browser: ",
        "“ – افتحه في المتصفح: ",
    ),
    ("(Strg+C beendet)", "(Ctrl+C ends)", "(Ctrl+C للإنهاء)"),
    ("höre „", "listening to „", "أستمع إلى „"),
    ("Durchsage auf „", "Announcement on „", "إعلان على „"),
    (
        " s Ton empfangen",
        " s of sound received",
        " ث من الصوت استُقبلت",
    ),
    (" s gesprochen", " s spoken", " ث تحدث"),
    ("gesendet (", "sent (", "أُرسل ("),
    (
        " Eingaben), Bildschirm ",
        " inputs), screen ",
        " مدخلات)، الشاشة ",
    ),
    ("gespeichert", "saved", "حُفظ"),
    ("Mining auf ", "Mining set on ", "التعدين مُعدّ على "),
    (
        " weiteren Geräten eingestellt – läuft nur, wenn ein Gerät ruht",
        " more devices – runs only while a device is idle",
        " أجهزة أخرى – يعمل فقط حين يكون الجهاز خاملاً",
    ),
    ("nur im Leerlauf (", "only when idle (", "فقط عند الخمول ("),
    (
        " % der Kerne, nur am Strom: ",
        " % of cores, only on power: ",
        " % من الأنوية، فقط على الشاحن: ",
    ),
    ("Währung     ", "Currency    ", "العملة      "),
    ("Leistung    ", "Hashrate    ", "الأداء      "),
    ("Regeln      ", "Rules       ", "القواعد     "),
    ("unbekannt: ", "unknown: ", "غير معروف: "),
];

static ALLGEMEIN: &[(&str, &str, &str)] = &[
    ("Benutzung: uapp detect|install|list|run|remove|android-sync|policy …\n  uapp install <datei> [--silent] [--allow-foreign] [--name NAME]", "Usage: uapp detect|install|list|run|remove|android-sync|policy …\n  uapp install <file> [--silent] [--allow-foreign] [--name NAME]", "الاستخدام: uapp detect|install|list|run|remove|android-sync|policy …\n  uapp install <ملف> [--silent] [--allow-foreign] [--name الاسم]"),
    ("Keine Apps installiert.", "No apps installed.", "لا توجد تطبيقات مثبّتة."),
    (" App(s) installiert", " app(s) installed", " تطبيق(ات) مثبّتة"),
    ("Aufruf: upkg info|verify|install <datei.uap>", "Usage: upkg info|verify|install <file.uap>", "الاستخدام: upkg info|verify|install <ملف.uap>"),
    ("keine Verbindung zu ", "no connection to ", "لا اتصال بـ "),
    ("nicht gefunden", "not found", "غير موجود"),
    ("nicht installiert", "not installed", "غير مثبّت"),
    ("Keine Berechtigung", "Permission denied", "لا صلاحية"),
    ("netz", "net", "الشبكة"),
    ("protokoll", "log", "سجل"),
    ("Gerät", "device", "الجهاز"),
    ("Geräte", "devices", "الأجهزة"),
    ("erlaubt", "allowed", "مسموح"),
    ("gesperrt", "blocked", "محجوب"),
    ("verborgen", "hidden", "مخفي"),
    ("vorgegeben", "set by you", "محدد يدوياً"),
    ("frei", "open", "مفتوح"),
    ("über ", "via ", "عبر "),
    ("an", "on", "مفعّل"),
    ("aus", "off", "مطفأ"),
    ("ja", "yes", "نعم"),
    ("nein", "no", "لا"),
    ("läuft", "running", "يعمل"),
    ("keine", "none", "لا شيء"),
    ("Fehler", "Error", "خطأ"),
];
