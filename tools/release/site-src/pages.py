"""Content of ultar.code-craft.tech – every text in Arabic first, then English and German.

Rules for this file (owner, 2026-11-28):
* Arabic first; digits always 0–9.
* Ultar OS is not compared with other systems anywhere – our own goals and promises instead.
* Only what is true today: what works, what is new in development and what is not there yet.
* No link to the customer portal yet; the account binding comes later.
Markup inside texts: [label](/path/), **bold**, `code`. Internal paths get the language prefix.
"""

ASSET_VERSION = "091"


def L(ar, en, de):
    return {"ar": ar, "en": en, "de": de}


LANG_NAME = {"ar": "العربية", "en": "English", "de": "Deutsch"}
LANG_SHORT = {"ar": "ع", "en": "EN", "de": "DE"}

# ------------------------------------------------------------------ interface words
T = {
    "menu": L("القائمة", "Menu", "Menü"),
    "lang_label": L("اللغة", "Language", "Sprache"),
    "nav_label": L("التنقل الرئيسي", "Main navigation", "Hauptnavigation"),
    "sub_label": L("صفحات هذا القسم", "Pages of this section", "Seiten dieses Bereichs"),
    "get": L("تنزيل Ultar OS", "Get Ultar OS", "Ultar OS holen"),
    "skip": L("انتقل إلى المحتوى", "Skip to content", "Zum Inhalt"),
    "by": L("تطوير", "developed by", "entwickelt von"),
    "privacy": L("خصوصية هذا الموقع", "Privacy of this site", "Datenschutz dieser Seite"),
    "totop": L("إلى الأعلى", "Back to top", "Nach oben"),
    "more": L("اعرف المزيد", "Learn more", "Mehr erfahren"),
    "desc": L("Ultar OS – نظام تشغيل مستقل بنواة خاصة مكتوبة بلغة Rust من KM Code Craft. العربية أولاً، خصوصية، حزم وتحديثات موقّعة.",
              "Ultar OS – an independent operating system with its own kernel written in Rust, by KM Code Craft. Arabic first, private, signed packages and updates.",
              "Ultar OS – ein unabhängiges Betriebssystem mit eigenem Kern in Rust von KM Code Craft. Arabisch zuerst, privat, signierte Pakete und Updates."),
    "latest_wait": L("جارٍ تحميل أحدث إصدار…", "Loading the newest release…", "Neueste Ausgabe wird geladen …"),
    "rel_wait": L("جارٍ تحميل قائمة الإصدارات… إذا لم تظهر، فالقائمة موجودة أيضاً في [releases.json](/downloads/releases.json).",
                  "Loading the list of releases… if it does not appear, the list is also in [releases.json](/downloads/releases.json).",
                  "Die Liste der Ausgaben wird geladen … erscheint sie nicht, steht sie auch in [releases.json](/downloads/releases.json)."),
    "shots_note": L("صور من برنامج واجهة Ultar OS الحقيقي (صور الاختبار الآلي)، وليست تصاميم.",
                    "Pictures rendered by the real Ultar OS interface software (its automatic test images), not designs.",
                    "Bilder aus der echten Oberflächen-Software von Ultar OS (ihre automatischen Prüfbilder), keine Entwürfe."),
    "st_ok": L("يعمل في الإصدار الحالي", "Works in the current release", "Funktioniert in der aktuellen Ausgabe"),
    "st_new": L("جديد قيد التطوير", "New in development", "Neu in der Entwicklung"),
    "st_no": L("لم يكتمل بعد", "Not there yet", "Noch nicht fertig"),
    "upd_k": L("أحدث تحديث للنظام عبر الشبكة", "Newest system update over the network", "Neueste Systemaktualisierung über das Netz"),
    "upd_wait": L("جارٍ السؤال…", "Asking the server…", "Wird abgefragt …"),
}

# ------------------------------------------------------------------ navigation (like a product site: products, get, guides, help)
NAV = [
    {"label": L("المنتجات", "Products", "Produkte"), "group": "products", "cols": [
        {"h": L("النظام", "The system", "Das System"), "items": [
            {"l": "Ultar Desktop", "href": "/desktop/", "d": L("للحاسوب المكتبي والمحمول", "For desktop and laptop", "Für PC und Laptop")},
            {"l": "Ultar Server", "href": "/server/", "d": L("بلا شاشة، عبر SSH والويب", "Headless, over SSH and the web", "Ohne Bildschirm, per SSH und Web")},
            {"l": L("Ultar للهاتف", "Ultar Phone", "Ultar Handy"), "href": "/phone/", "d": L("واجهة خاصة بالهاتف", "Its own phone interface", "Eigene Oberfläche fürs Handy")},
            {"l": L("نواة Ultar", "Ultar kernel", "Ultar-Kern"), "href": "/kernel/", "d": L("نواتنا الخاصة بلغة Rust", "Our own kernel in Rust", "Der eigene Kern in Rust")},
        ]},
        {"h": L("المزايا", "Features", "Funktionen"), "items": [
            {"l": L("مجموعة الأجهزة", "Device mesh", "Geräteverbund"), "href": "/mesh/", "d": L("اربط أجهزتك وتابع العمل بينها", "Pair your devices, carry work across", "Geräte koppeln, Arbeit weitergeben")},
            {"l": L("المساعد", "Helper", "Helfer"), "href": "/assistant/", "d": L("ذكاء اصطناعي على جهازك", "AI on your own device", "KI auf dem eigenen Gerät")},
            {"l": L("المتجر والتطبيقات", "Store and apps", "Store und Apps"), "href": "/apps/", "d": L("تطبيقات موقّعة في خلايا معزولة", "Signed apps in isolated cells", "Signierte Apps in eigenen Zellen")},
            {"l": L("الأمان والخصوصية", "Security and privacy", "Sicherheit und Datenschutz"), "href": "/security/", "d": L("تواقيع وتشفير من طرف إلى طرف", "Signatures, end-to-end encryption", "Signaturen, Ende-zu-Ende")},
        ]},
        {"h": L("المزيد", "More", "Mehr"), "items": [
            {"l": L("العتاد المختبَر", "Tested hardware", "Geprüfte Hardware"), "href": "/hardware/", "d": L("الأجهزة وحالتها", "Devices and their status", "Geräte und ihr Stand")},
            {"l": L("للمطوّرين", "For developers", "Für Entwickler"), "href": "/developers/", "d": L("بناء التطبيقات ونشرها", "Build and publish apps", "Apps bauen und veröffentlichen")},
            {"l": L("حساب Ultar", "Ultar account", "Ultar-Konto"), "href": "/account/", "d": L("الرقم والمكالمات والمجموعة", "Number, calls, mesh", "Nummer, Anrufe, Verbund")},
        ]},
    ]},
    {"label": L("التنزيل", "Get Ultar", "Ultar holen"), "group": "download", "cols": [
        {"items": [
            {"l": "Desktop", "href": "/download/desktop/", "d": L("صورة تثبيت USB", "USB installer image", "USB-Installationsabbild")},
            {"l": "Server", "href": "/download/server/", "d": L("نفس المثبّت، نوع «خادم»", "Same installer, kind “server”", "Gleicher Installer, Art „Server“")},
            {"l": L("الهاتف", "Phone", "Handy"), "href": "/download/phone/", "d": L("إصدار تجريبي للهاتف", "Experimental phone release", "Experimentelle Handy-Ausgabe")},
            {"l": L("آلة افتراضية", "Virtual machine", "Virtuelle Maschine"), "href": "/download/vm/", "d": L("للتجربة بأمان", "To try it safely", "Gefahrlos ausprobieren")},
            {"l": L("كل الإصدارات", "All releases", "Alle Ausgaben"), "href": "/releases/", "d": L("الملاحظات والحالة", "Notes and status", "Hinweise und Stand")},
        ]},
    ]},
    {"label": L("الأدلّة", "Guides", "Anleitungen"), "group": "install", "cols": [
        {"items": [
            {"l": L("التثبيت على حاسوب", "Install on a computer", "Auf einem Rechner installieren"), "href": "/install/desktop/", "d": L("Desktop وServer عبر USB", "Desktop and server via USB", "Desktop und Server per USB")},
            {"l": L("في آلة افتراضية", "In a virtual machine", "In einer virtuellen Maschine"), "href": "/install/vm/", "d": "QEMU · VMware"},
            {"l": L("على الهاتف", "On a phone", "Auf dem Handy"), "href": "/install/phone/", "d": "Samsung Galaxy A30"},
            {"l": L("التحقق من التنزيل", "Verify a download", "Download prüfen"), "href": "/install/verify/", "d": "SHA-256 · Ed25519"},
            {"l": L("تحديث النظام", "Update the system", "System aktualisieren"), "href": "/update/", "d": L("A/B مع رجوع تلقائي", "A/B with automatic fallback", "A/B mit automatischem Rückfall")},
        ]},
    ]},
    {"label": L("المساعدة", "Help", "Hilfe"), "group": "support", "cols": [
        {"items": [
            {"l": L("أسئلة شائعة", "Questions and answers", "Häufige Fragen"), "href": "/support/"},
            {"l": L("حساب Ultar", "Ultar account", "Ultar-Konto"), "href": "/account/"},
            {"l": L("الإبلاغ عن خطأ", "Report a problem", "Fehler melden"), "href": "/support/#contact"},
            {"l": L("الإبلاغ عن ثغرة أمنية", "Report a security issue", "Sicherheitslücke melden"), "href": "/security/#report"},
        ]},
    ]},
    {"label": L("الأخبار", "News", "Neuigkeiten"), "href": "/news/"},
    {"label": L("من نحن", "About", "Über uns"), "href": "/about/"},
]

SUBNAV = {
    "products": [
        ("Desktop", "/desktop/"), ("Server", "/server/"), (L("الهاتف", "Phone", "Handy"), "/phone/"),
        (L("النواة", "Kernel", "Kern"), "/kernel/"), (L("المجموعة", "Mesh", "Verbund"), "/mesh/"),
        (L("المساعد", "Helper", "Helfer"), "/assistant/"), (L("المتجر", "Store", "Store"), "/apps/"),
        (L("الأمان", "Security", "Sicherheit"), "/security/"), (L("العتاد", "Hardware", "Hardware"), "/hardware/"),
        (L("للمطوّرين", "Developers", "Entwickler"), "/developers/"),
    ],
    "download": [
        (L("نظرة عامة", "Overview", "Übersicht"), "/download/"), ("Desktop", "/download/desktop/"),
        ("Server", "/download/server/"), (L("الهاتف", "Phone", "Handy"), "/download/phone/"),
        (L("آلة افتراضية", "Virtual machine", "Virtuelle Maschine"), "/download/vm/"),
        (L("كل الإصدارات", "All releases", "Alle Ausgaben"), "/releases/"),
    ],
    "install": [
        (L("نظرة عامة", "Overview", "Übersicht"), "/install/"),
        (L("حاسوب", "Computer", "Rechner"), "/install/desktop/"),
        (L("آلة افتراضية", "Virtual machine", "Virtuelle Maschine"), "/install/vm/"),
        (L("الهاتف", "Phone", "Handy"), "/install/phone/"),
        (L("التحقق", "Verify", "Prüfen"), "/install/verify/"),
        (L("التحديث", "Update", "Aktualisieren"), "/update/"),
    ],
    "support": [
        (L("أسئلة شائعة", "Questions", "Fragen"), "/support/"), (L("حساب Ultar", "Ultar account", "Ultar-Konto"), "/account/"),
    ],
}

FOOTER = [
    {"h": L("المنتجات", "Products", "Produkte"), "items": [
        ("Ultar Desktop", "/desktop/"), ("Ultar Server", "/server/"), (L("Ultar للهاتف", "Ultar Phone", "Ultar Handy"), "/phone/"),
        (L("نواة Ultar", "Ultar kernel", "Ultar-Kern"), "/kernel/"), (L("مجموعة الأجهزة", "Device mesh", "Geräteverbund"), "/mesh/"),
        (L("المساعد", "Helper", "Helfer"), "/assistant/"), (L("المتجر والتطبيقات", "Store and apps", "Store und Apps"), "/apps/"),
        (L("العتاد المختبَر", "Tested hardware", "Geprüfte Hardware"), "/hardware/"),
    ]},
    {"h": L("التنزيل", "Get Ultar", "Ultar holen"), "items": [
        ("Desktop", "/download/desktop/"), ("Server", "/download/server/"), (L("الهاتف", "Phone", "Handy"), "/download/phone/"),
        (L("آلة افتراضية", "Virtual machine", "Virtuelle Maschine"), "/download/vm/"), (L("كل الإصدارات", "All releases", "Alle Ausgaben"), "/releases/"),
    ]},
    {"h": L("الأدلّة", "Guides", "Anleitungen"), "items": [
        (L("التثبيت على حاسوب", "Install on a computer", "Installieren"), "/install/desktop/"),
        (L("في آلة افتراضية", "In a virtual machine", "In einer VM"), "/install/vm/"),
        (L("على الهاتف", "On a phone", "Auf dem Handy"), "/install/phone/"),
        (L("التحقق من التنزيل", "Verify a download", "Download prüfen"), "/install/verify/"),
        (L("تحديث النظام", "Update the system", "Aktualisieren"), "/update/"),
    ]},
    {"h": L("المساعدة", "Help", "Hilfe"), "items": [
        (L("أسئلة شائعة", "Questions and answers", "Häufige Fragen"), "/support/"), (L("حساب Ultar", "Ultar account", "Ultar-Konto"), "/account/"),
        (L("الأمان والخصوصية", "Security and privacy", "Sicherheit"), "/security/"), (L("للمطوّرين", "For developers", "Für Entwickler"), "/developers/"),
    ]},
    {"h": "KM Code Craft", "items": [
        (L("من نحن", "About", "Über uns"), "/about/"), (L("الأخبار", "News", "Neuigkeiten"), "/news/"),
        (L("خصوصية هذا الموقع", "Privacy of this site", "Datenschutz dieser Seite"), "/privacy/"), ("code-craft.tech", "https://code-craft.tech"),
    ]},
]

# ------------------------------------------------------------------ news (newest first)
NEWS = [
    {"date": "2026-11-28",
     "h": L("تشفير البيانات وSSH وشبكة USB-C على نواة Ultar", "Data encryption, SSH and USB-C networking on the Ultar kernel", "Datenverschlüsselung, SSH und USB-C-Netz auf dem Ultar-Kern"),
     "p": L("برنامج التثبيت يشفّر قسم البيانات بعبارة مرور، ويطلبها النظام عند الإقلاع بشاشة فتح رسومية. خادم SSH مكتوب بلغة Rust يعمل مباشرة على نواتنا (مفاتيح فقط)، والاتصال بالشبكة عبر كابل USB-C يعمل. وتُطبَّق اللغة والمظهر وحجم النص فوراً دون إعادة تشغيل.",
            "The installer encrypts the data partition with a passphrase, and the system asks for it at start on a graphical unlock screen. An SSH server written in Rust runs directly on our kernel (keys only), and networking over a USB-C cable works. Language, theme and text size now apply at once, without a restart.",
            "Der Installer verschlüsselt die Datenpartition mit einer Passphrase, das System fragt sie beim Start auf einem grafischen Entsperr-Bildschirm ab. Ein SSH-Server in Rust läuft direkt auf unserem Kern (nur Schlüssel), und Netz über ein USB-C-Kabel funktioniert. Sprache, Thema und Textgröße wirken jetzt sofort, ohne Neustart.")},
    {"date": "2026-11-28",
     "h": L("الإصدار 0.8.1: تحديثات النظام عبر الشبكة", "Release 0.8.1: system updates over the network", "Ausgabe 0.8.1: Systemaktualisierungen über das Netz"),
     "p": L("الإعدادات ← تحديث النظام: بحث، تنزيل، تثبيت، ثم إعادة تشغيل. يُكتب التحديث في القسم الثاني (A/B) وتبقى النسخة السابقة جاهزة. الأجهزة المثبّت عليها 0.8.0 تحتاج إلى إعادة تثبيت مرة واحدة. التفاصيل في [صفحة التحديث](/update/).",
            "Settings → System update: check, download, install, restart. The update goes into the second slot (A/B) and the previous version stays ready. Computers with 0.8.0 need one reinstall. Details on the [update page](/update/).",
            "Einstellungen → Systemaktualisierung: suchen, herunterladen, installieren, neu starten. Die Aktualisierung landet im zweiten Bereich (A/B), die vorige Fassung bleibt bereit. Rechner mit 0.8.0 brauchen einmal eine Neuinstallation. Einzelheiten auf der [Update-Seite](/update/).")},
    {"date": "2026-11-26",
     "h": L("التثبيت الرسومي مكتمل ومختبَر", "The graphical installation, complete and tested", "Grafische Installation vollständig geprüft"),
     "p": L("الخطوات الست عشرة كلها تُختبر آلياً: من اللغة حتى إعادة التشغيل، ثم الإقلاع الأول من القرص المثبّت. ويأخذ الإقلاع الأول كل اختياراتك: اللغة ولوحة المفاتيح والمنطقة الزمنية والمكوّنات.",
            "All sixteen steps are tested automatically: from language to restart, then the first start from the installed disk. The first start takes over all your choices: language, keyboard, time zone and components.",
            "Alle sechzehn Schritte werden automatisch geprüft: von der Sprache bis zum Neustart, dann der erste Start von der installierten Platte. Der Erststart übernimmt alle Antworten: Sprache, Tastatur, Zeitzone und Komponenten.")},
    {"date": "2026-11-26",
     "h": L("إصدار الهاتف التجريبي 0.8.0-phone", "Experimental phone release 0.8.0-phone", "Experimentelle Handy-Ausgabe 0.8.0-phone"),
     "p": L("واجهة الهاتف الخاصة: شاشة القفل، الشاشة الرئيسية، الإعدادات السريعة، التطبيقات الأخيرة. صورة USB لأجهزة x86_64 وحزمة لهاتف Samsung Galaxy A30 للتجربة.",
            "The phone's own interface: lock screen, home screen, quick settings, recent apps. A USB image for x86_64 devices and a package for the Samsung Galaxy A30 to try.",
            "Die eigene Handy-Oberfläche: Sperrbildschirm, Startbildschirm, Schnelleinstellungen, zuletzt benutzte Apps. Ein USB-Abbild für x86_64-Geräte und ein Paket für das Samsung Galaxy A30 zum Ausprobieren.")},
    {"date": "2026-11-25",
     "h": L("Ultar OS 0.8.0 تجريبي متاح للتنزيل", "Ultar OS 0.8.0 beta available", "Ultar OS 0.8.0 Beta verfügbar"),
     "p": L("أول صورة تثبيت عامة بنواة Ultar الخاصة، مع مكالمات صوت وصورة مشفّرة من طرف إلى طرف عبر رقم Ultar. الخادم يوصل الطرفين فقط ولا يحفظ شيئاً عن المكالمة.",
            "The first public installer image with our own Ultar kernel, with end-to-end encrypted voice and video calls over the Ultar number. The server only connects the two sides and keeps nothing about the call.",
            "Das erste öffentliche Installationsabbild mit dem eigenen Ultar-Kern, mit Ende-zu-Ende verschlüsselten Sprach- und Videoanrufen über die Ultar-Nummer. Der Server verbindet nur und speichert nichts über den Anruf.")},
    {"date": "2026-11-24",
     "h": L("مجموعة الأجهزة والمساعد المحلي", "Device mesh and local helper", "Geräteverbund und lokaler Helfer"),
     "p": L("أجهزتك تتعرّف على بعضها، تُقرَن مرة واحدة برمز من ستة أرقام، وتتابع العمل من جهاز إلى آخر – والجهاز الهدف يسأل دائماً. والمساعد يعمل بنموذج على جهازك أو على جهاز مقترن.",
            "Your devices find each other, pair once with a six-digit code and carry work from one to another – the target device always asks. The helper works with a model on your device or on a paired one.",
            "Ihre Geräte finden sich, werden einmal mit einem sechsstelligen Code gekoppelt und geben Arbeit weiter – das Zielgerät fragt immer. Der Helfer arbeitet mit einem Modell auf Ihrem oder einem gekoppelten Gerät.")},
]

# ------------------------------------------------------------------ shared pieces
PROMISE = [
    {"h": L("سريع من أول لمسة", "Fast from the first touch", "Schnell ab der ersten Berührung"),
     "p": L("يقلع خلال ثوانٍ، ويستجيب فوراً، ويستهلك القليل من الذاكرة – فيبقى جهازك سريعاً بعد سنوات كما في يومه الأول.",
            "Starts in seconds, answers at once and uses little memory – so your device stays as fast years later as on its first day.",
            "Startet in Sekunden, reagiert sofort und braucht wenig Speicher – so bleibt Ihr Gerät nach Jahren so schnell wie am ersten Tag.")},
    {"h": L("جهازك ملكك", "Your device is yours", "Ihr Gerät gehört Ihnen"),
     "p": L("لا برامج مخفية، لا إعلانات، لا تتبّع. لا شيء يعمل على جهازك دون علمك.",
            "No hidden software, no ads, no tracking. Nothing runs on your device without you knowing.",
            "Keine versteckte Software, keine Werbung, kein Tracking. Nichts läuft ohne Ihr Wissen.")},
    {"h": L("خصوصية بلا تنازل", "Privacy without compromise", "Datenschutz ohne Kompromiss"),
     "p": L("مكالماتك مشفّرة من طرف إلى طرف، وبياناتك لا تغادر جهازك إلا بإذنك، والتشخيص والموقع مطفآن من البداية.",
            "Your calls are end-to-end encrypted, your data leaves your device only with your permission, diagnostics and location are off from the start.",
            "Anrufe sind Ende-zu-Ende verschlüsselt, Daten verlassen das Gerät nur mit Ihrer Erlaubnis, Diagnose und Standort sind von Anfang an aus.")},
    {"h": L("يعمل بلا إنترنت", "Works without internet", "Funktioniert ohne Internet"),
     "p": L("التثبيت والتعريفات تعمل دون اتصال، والمتجر يعمل بآخر فهرس موثوق – يكفي ذاكرة USB.",
            "Installation and drivers work offline, and the store keeps working with the last trusted catalogue – a USB stick is enough.",
            "Installation und Treiber gehen offline, der Store arbeitet mit dem letzten vertrauenswürdigen Katalog weiter – ein USB-Stick genügt.")},
    {"h": L("لا يتعطّل بعد التحديث", "Never broken by an update", "Kein Update legt Sie lahm"),
     "p": L("كل تحديث موقّع ويُثبَّت بجانب النسخة الحالية. إذا فشل شيء، يعود النظام تلقائياً إلى النسخة السابقة.",
            "Every update is signed and installed next to the current version. If anything fails, the system returns to the previous one by itself.",
            "Jedes Update ist signiert und wird neben der aktuellen Fassung installiert. Geht etwas schief, kehrt das System selbst zur vorigen zurück.")},
    {"h": L("يتكيّف مع جهازك", "Adapts to your device", "Passt sich Ihrem Gerät an"),
     "p": L("النظام يعرف عتاد جهازك ويضبط نفسه عليه. صِل هاتفك بشاشة ولوحة مفاتيح فيصبح حاسوباً كاملاً.",
            "The system knows your hardware and tunes itself to it. Connect your phone to a screen and keyboard and it becomes a full computer.",
            "Das System kennt Ihre Hardware und stellt sich darauf ein. Handy an Bildschirm und Tastatur – und es wird zum vollwertigen Computer.")},
    {"h": L("مساعد ذكي داخل جهازك", "A smart helper inside your device", "Ein kluger Helfer im Gerät"),
     "p": L("الذكاء الاصطناعي يعمل على جهازك نفسه، دون سحابة. وبعد اكتمال التحكم الصوتي: قل «Hey KM» وتحدّث بالعربية أو الإنجليزية.",
            "The AI runs on your device itself, without a cloud. Once voice control is finished: say “Hey KM” and talk in Arabic or English.",
            "Die KI läuft auf Ihrem Gerät selbst, ohne Cloud. Sobald die Sprachsteuerung fertig ist: „Hey KM“ sagen und Arabisch oder Englisch sprechen.")},
    {"h": L("برامجك تبقى معك", "Your programs stay with you", "Ihre Programme bleiben bei Ihnen"),
     "p": L("النظام صغير ويحتوي ما تحتاجه فقط. ما تريده تضيفه من المتجر وتزيله دون أثر.",
            "The system is small and holds only what you need. Add what you want from the store and remove it without a trace.",
            "Das System ist klein und enthält nur, was Sie brauchen. Was Sie wollen, holen Sie aus dem Store und entfernen es spurlos.")},
]

STATUS_NOW = {
    "ok": [
        L("الإقلاع عبر UEFI على حواسيب x86_64، في VMware وQEMU", "Start via UEFI on x86_64 computers, in VMware and QEMU", "Start über UEFI auf x86_64-Rechnern, in VMware und QEMU"),
        L("جلسة سطح المكتب: مركّب النوافذ، الشريط، شاشة القفل، حوارات النظام", "The desktop session: compositor, bar, lock screen, system dialogs", "Die Desktop-Sitzung: Compositor, Leiste, Sperrbildschirm, Systemdialoge"),
        L("المتجر والإعدادات والملفات كتطبيقات مستقلة، وحزم موقّعة من خادم المتجر", "Store, Settings and Files as separate apps, and signed packages from the store server", "Store, Einstellungen und Dateien als eigene Apps, signierte Pakete vom Store-Server"),
        L("تحديثات النظام الموقّعة على النسختين A/B مع الرجوع التلقائي (عبر الشبكة من 0.8.1)", "Signed system updates on the two A/B copies with automatic fallback (over the network from 0.8.1)", "Signierte System-Updates auf zwei A/B-Kopien mit automatischem Rückfall (über das Netz ab 0.8.1)"),
        L("مكالمات صوت وصورة مشفّرة من طرف إلى طرف، مع الصوت وكاميرات USB في النواة", "End-to-end encrypted voice and video calls, with sound and USB cameras in the kernel", "Ende-zu-Ende verschlüsselte Sprach- und Videoanrufe, mit Ton und USB-Kameras im Kern"),
        L("الملفات ومحرّر النصوص والطرفية ومتصفح وضع القراءة", "Files, text editor, terminal and a reading-mode browser", "Dateien, Texteditor, Terminal und ein Browser im Lesemodus"),
    ],
    "new": [
        L("التثبيت الرسومي بخطواته الست عشرة مع إعادة التشغيل في النهاية", "The graphical installation in sixteen steps, with a restart at the end", "Grafische Installation in sechzehn Schritten mit Neustart am Ende"),
        L("تشفير قسم البيانات بعبارة مرور وشاشة فتح عند الإقلاع", "Encryption of the data partition with a passphrase and an unlock screen at start", "Verschlüsselung der Datenpartition mit Passphrase und Entsperr-Bildschirm beim Start"),
        L("خادم SSH على نواة Ultar، والشبكة عبر كابل USB-C", "SSH server on the Ultar kernel, and networking over a USB-C cable", "SSH-Server auf dem Ultar-Kern und Netz über ein USB-C-Kabel"),
        L("محرّك الويب Servo للمتصفح الكامل (فيديو وصوت)", "The Servo web engine for the full browser (video and sound)", "Die Web-Engine Servo für den vollständigen Browser (Video und Ton)"),
    ],
    "no": [
        L("تعريفات Wi-Fi وعرض الرسوميات على العتاد الحقيقي، والإقلاع المباشر عليه ما زال قيد الاختبار", "Wi-Fi drivers and graphics output on real hardware; starting directly on it is still being tested", "WLAN-Treiber und Grafikausgabe auf echter Hardware; der direkte Start darauf wird noch geprüft"),
        L("فتح القرص بشريحة الجهاز (TPM)", "Unlocking the disk with the device chip (TPM)", "Entsperren der Platte mit dem Gerätechip (TPM)"),
        L("تطبيقات المكتب (PDF وWord وExcel) والتحكم الصوتي «Hey KM»", "Office apps (PDF, Word, Excel) and voice control “Hey KM”", "Büro-Apps (PDF, Word, Excel) und Sprachsteuerung „Hey KM“"),
        L("نسخة الهاتف على هاتف حقيقي، ونواة Ultar لمعالجات ARM64", "The phone version on a real phone, and the Ultar kernel for ARM64", "Die Handy-Version auf einem echten Handy und der Ultar-Kern für ARM64"),
    ],
}

BETA_NOTE = L("Ultar OS حالياً إصدار تجريبي: للتجربة على آلة افتراضية أو جهاز اختبار، وليس لجهاز يحتوي بيانات مهمة.",
              "Ultar OS is a beta for now: to try in a virtual machine or on a test device, not on a computer with important data.",
              "Ultar OS ist vorerst eine Beta: zum Ausprobieren in einer virtuellen Maschine oder auf einem Testgerät, nicht auf einem Rechner mit wichtigen Daten.")

INSTALLER_STEPS = [
    (L("اللغة", "Language", "Sprache"), L("العربية أو الإنجليزية أو الألمانية – لبرنامج التثبيت وللنظام.", "Arabic, English or German – for the installer and the system.", "Arabisch, Englisch oder Deutsch – für Installer und System.")),
    (L("تسهيلات الاستخدام", "Accessibility", "Barrierefreiheit"), L("نص كبير وقارئ الشاشة.", "Large text and screen reader.", "Große Schrift und Bildschirmleser.")),
    (L("لوحة المفاتيح", "Keyboard", "Tastatur"), L("عربي+إنجليزي، إنجليزي، ألماني.", "Arabic+English, English, German.", "Arabisch+Englisch, Englisch, Deutsch.")),
    (L("الشبكة", "Network", "Netzwerk"), L("اختياري – التثبيت يعمل بلا إنترنت.", "Optional – the installation works without internet.", "Optional – die Installation geht ohne Internet.")),
    (L("تجربة أو تثبيت", "Try or install", "Testen oder installieren"), L("جرّب النظام من الذاكرة دون أن يتغيّر شيء على القرص.", "Try the system from the stick without changing anything on the disk.", "Das System vom Stick testen, ohne etwas auf der Platte zu ändern.")),
    (L("نوع التثبيت", "Kind of installation", "Art der Installation"), L("سطح مكتب أو خادم.", "Desktop or server.", "Desktop oder Server.")),
    (L("العتاد والتعريفات", "Hardware and drivers", "Hardware und Treiber"), L("التعريفات الموقّعة على وسيط التثبيت تُختار تلقائياً.", "The signed drivers on the installation medium are chosen by themselves.", "Die signierten Treiber auf dem Installationsmedium werden selbst gewählt.")),
    (L("المكوّنات", "Components", "Komponenten"), L("ما تريده الآن، والباقي لاحقاً من المتجر.", "What you want now; the rest later from the store.", "Was Sie jetzt wollen; den Rest später aus dem Store.")),
    (L("إضافات", "Extras", "Extras"), L("التحديثات أثناء التثبيت وبرامج ترميز الوسائط.", "Updates during installation and media codecs.", "Updates während der Installation und Medien-Codecs.")),
    (L("القرص", "Disk", "Datenträger"), L("القرص كاملاً، أو بجانب نظام موجود في المساحة الحرة فقط، أو يدوياً.", "The whole disk, beside an existing system in free space only, or manual.", "Ganze Platte, neben einem vorhandenen System nur im freien Platz, oder manuell.")),
    (L("التشفير", "Encryption", "Verschlüsselung"), L("بلا تشفير أو بعبارة مرور (12 حرفاً على الأقل).", "None or a passphrase (at least 12 characters).", "Keine oder Passphrase (mindestens 12 Zeichen).")),
    (L("الحساب", "Account", "Konto"), L("اسمك واسم المستخدم واسم الجهاز وكلمة مرور (8 أحرف على الأقل).", "Your name, user name, device name and a password (at least 8 characters).", "Ihr Name, Benutzername, Gerätename und ein Kennwort (mindestens 8 Zeichen).")),
    (L("المنطقة الزمنية", "Time zone", "Zeitzone"), L("دمشق أولاً، مع التوقيت الصيفي.", "Damascus first, with daylight saving time.", "Damaskus zuerst, mit Sommerzeit.")),
    (L("الملخّص", "Summary", "Zusammenfassung"), L("راجع كل شيء وأكّد المسح صراحةً.", "Check everything and confirm the erase explicitly.", "Alles prüfen und das Löschen ausdrücklich bestätigen.")),
    (L("التثبيت", "Installation", "Installation"), L("الكتابة على القرص مع شريط تقدّم.", "Writing to the disk with a progress bar.", "Schreiben auf die Platte mit Fortschritt.")),
    (L("اكتمل التثبيت", "Installation finished", "Installation abgeschlossen"), L("«إعادة التشغيل» – انزع ذاكرة USB عندما يُطلب منك.", "“Restart” – remove the USB stick when asked.", "„Neu starten“ – den USB-Stick entfernen, wenn danach gefragt wird.")),
]

# ------------------------------------------------------------------ pages
PAGES = []


def page(slug, group, title, blocks, lead=None, desc=None, hero=False):
    PAGES.append({"slug": slug, "group": group, "title": title, "blocks": blocks, "lead": lead, "desc": desc, "hero": hero})


# ---- home
page("", "home", L("الرئيسية", "Home", "Start"), hero=True, blocks=[
    ("hero", {
        "badge": L("Ultar OS 0.8 تجريبي · نواة خاصة مكتوبة بلغة Rust", "Ultar OS 0.8 beta · own kernel written in Rust", "Ultar OS 0.8 Beta · eigener Kern in Rust"),
        "h1": L("نظام تشغيل مستقل، العربية أولاً.", "An independent operating system, Arabic first.", "Ein unabhängiges Betriebssystem, Arabisch zuerst."),
        "lead": L("Ultar OS نظام مبني من الأساس لدى KM Code Craft: نواة خاصة بلغة Rust، وواجهة لسطح المكتب وللخادم وللهاتف، ومتجر تطبيقات موقّع، وكل تطبيق في خلية معزولة بأقل الصلاحيات.",
                  "Ultar OS is built from the ground up by KM Code Craft: its own kernel in Rust, editions for desktop, server and phone, a signed app store, and every app in an isolated cell with the fewest rights.",
                  "Ultar OS entsteht bei KM Code Craft von Grund auf: ein eigener Kern in Rust, Ausgaben für Desktop, Server und Handy, ein signierter App-Store und jede App in einer abgeschotteten Zelle mit den wenigsten Rechten."),
        "btns": [(L("تنزيل Ultar OS", "Download Ultar OS", "Ultar OS herunterladen"), "/download/", "primary"),
                 (L("ما الجديد", "What is new", "Was ist neu"), "/releases/", "")],
        "latest": True, "img": "apps",
        "alt": L("قائمة التطبيقات في Ultar OS", "The app grid of Ultar OS", "Das App-Raster von Ultar OS"),
    }),
    ("sec", {"h": L("نظام واحد لكل جهاز", "One system for every device", "Ein System für jedes Gerät"),
             "lead": L("القاعدة نفسها والنواة نفسها والأمان نفسه – بواجهة تناسب كل جهاز.", "The same base, the same kernel and the same security – with an interface that fits each device.", "Dieselbe Basis, derselbe Kern, dieselbe Sicherheit – mit einer Oberfläche, die zum Gerät passt."),
             "body": [("cards", [
                 {"k": "Desktop", "h": L("لحاسوبك المكتبي والمحمول", "For your desktop and laptop", "Für PC und Laptop"),
                  "p": L("شريط وقائمة تطبيقات وبحث وإعدادات سريعة، فاتح وداكن، العربية من اليمين لليسار في كل مكان.", "Bar, app grid, search and quick settings, light and dark, Arabic right-to-left everywhere.", "Leiste, App-Raster, Suche und Schnelleinstellungen, hell und dunkel, Arabisch von rechts nach links überall."),
                  "href": "/desktop/"},
                 {"k": "Server", "h": L("لخادمك", "For your server", "Für Ihren Server"),
                  "p": L("بلا شاشة: إدارة عبر SSH بالمفاتيح فقط وواجهة ويب مشفّرة.", "Headless: managed over SSH with keys only and an encrypted web console.", "Ohne Bildschirm: verwaltet per SSH nur mit Schlüsseln und über eine verschlüsselte Web-Verwaltung."),
                  "href": "/server/"},
                 {"k": L("الهاتف", "Phone", "Handy"), "h": L("لهاتفك", "For your phone", "Für Ihr Handy"),
                  "p": L("واجهة خاصة باللمس: شاشة قفل، شاشة رئيسية، إعدادات سريعة. إصدار تجريبي.", "Its own touch interface: lock screen, home screen, quick settings. Experimental.", "Eigene Touch-Oberfläche: Sperrbildschirm, Startbildschirm, Schnelleinstellungen. Experimentell."),
                  "href": "/phone/"},
                 {"k": L("المجموعة", "Mesh", "Verbund"), "h": L("لكل أجهزتك معاً", "For all your devices together", "Für alle Geräte zusammen"),
                  "p": L("اقرن أجهزتك مرة واحدة وتابع ما تعمل عليه على جهاز آخر – مشفّر من طرف إلى طرف.", "Pair your devices once and continue your work on another one – end-to-end encrypted.", "Geräte einmal koppeln und die Arbeit auf einem anderen weiterführen – Ende-zu-Ende verschlüsselt."),
                  "href": "/mesh/"},
             ], 4)]}),
    ("sec", {"alt": True, "h": L("صور من النظام", "A look at the system", "Ein Blick ins System"),
             "body": [("shots", [
                 ("desktop", L("سطح المكتب: الشريط والبحث والتطبيقات", "The desktop: bar, search and apps", "Der Desktop: Leiste, Suche und Apps")),
                 ("overview", L("نظرة عامة على النوافذ", "Window overview", "Fensterübersicht")),
                 ("quick", L("الإعدادات السريعة", "Quick settings", "Schnelleinstellungen")),
                 ("lock", L("شاشة القفل", "The lock screen", "Der Sperrbildschirm")),
             ])]}),
    ("sec", {"h": L("وعدنا لك", "Our promise to you", "Unser Versprechen an Sie"),
             "lead": L("هذا ما نبني Ultar OS من أجله. كل قرار في النظام يُقاس بهذه الوعود.", "This is what we build Ultar OS for. Every decision in the system is measured against these promises.", "Dafür bauen wir Ultar OS. Jede Entscheidung im System wird an diesen Versprechen gemessen."),
             "body": [("cards", PROMISE[:4], 4), ("btns", [(L("كل الوعود", "All promises", "Alle Versprechen"), "/about/", "")])]}),
    ("sec", {"alt": True, "body": [("split", {
        "h": L("آمن من التصميم", "Secure by design", "Sicher von Grund auf"),
        "ps": [L("جهازك لا يثق بخادمنا: كل ما يُثبَّت – النظام والتعريفات والتطبيقات – يفحص الجهاز توقيعه بنفسه ويرفض ما ليس موقّعاً.",
                 "Your device does not trust our server: it checks the signature of everything it installs – system, drivers and apps – itself, and refuses what is not signed.",
                 "Ihr Gerät vertraut unserem Server nicht: Alles, was es installiert – System, Treiber und Apps –, prüft es selbst auf seine Signatur und lehnt Unsigniertes ab.")],
        "list": [L("كل تطبيق في خليته بأقل الصلاحيات", "Every app in its own cell with the fewest rights", "Jede App in ihrer Zelle mit den wenigsten Rechten"),
                 L("مكالمات مشفّرة من طرف إلى طرف – الخادم لا يحفظ شيئاً", "End-to-end encrypted calls – the server keeps nothing", "Ende-zu-Ende verschlüsselte Anrufe – der Server speichert nichts"),
                 L("تحديثات بنسختين A/B مع رجوع تلقائي", "Updates on two A/B copies with automatic fallback", "Updates auf zwei A/B-Kopien mit automatischem Rückfall")],
        "btns": [(L("الأمان والخصوصية", "Security and privacy", "Sicherheit und Datenschutz"), "/security/", "")],
        "img": "permission", "alt": L("حوار صلاحية من النظام", "A permission dialog of the system", "Ein Rechte-Dialog des Systems")})]}),
    ("sec", {"h": L("المتجر والعتاد", "Store and hardware", "Store und Hardware"), "body": [("cards", [
        {"h": L("متجر Ultar", "Ultar Store", "Ultar Store"),
         "p": L("فهرس موقّع عبر HTTPS أو من ذاكرة USB. يعمل دون حساب ودون إنترنت بآخر فهرس موثوق.", "A signed catalogue over HTTPS or from a USB stick. Works without an account and offline with the last trusted catalogue.", "Signierter Katalog über HTTPS oder vom USB-Stick. Geht ohne Konto und offline mit dem letzten vertrauenswürdigen Katalog."),
         "href": "/apps/"},
        {"h": L("العتاد المختبَر", "Tested hardware", "Geprüfte Hardware"),
         "p": L("على أي أجهزة يعمل Ultar OS اليوم، وما الذي ما زال قيد الاختبار.", "Which devices Ultar OS runs on today, and what is still being tested.", "Auf welchen Geräten Ultar OS heute läuft und was noch geprüft wird."),
         "href": "/hardware/"},
        {"h": L("نواة Ultar", "The Ultar kernel", "Der Ultar-Kern"),
         "p": L("نواة خاصة بلغة Rust تجهز في المحاكي خلال نحو 145 ميلي ثانية، مع نحو 2 450 اختباراً آلياً.", "Our own kernel in Rust, ready in about 145 ms in the emulator, with about 2 450 automatic tests.", "Eigener Kern in Rust, im Emulator nach rund 145 ms bereit, mit rund 2 450 automatischen Tests."),
         "href": "/kernel/"},
    ])]}),
    ("sec", {"alt": True, "h": L("آخر الأخبار", "Latest news", "Neuigkeiten"), "body": [("news", 3), ("btns", [(L("كل الأخبار", "All news", "Alle Neuigkeiten"), "/news/", "")])]}),
    ("sec", {"id": "contact", "h": L("أسئلة؟ نحن هنا.", "Questions? We are here.", "Fragen? Wir sind da."),
             "lead": L("اكتب لنا على [info@code-craft.tech](mailto:info@code-craft.tech) – بالعربية أو الإنجليزية أو الألمانية.", "Write to us at [info@code-craft.tech](mailto:info@code-craft.tech) – in Arabic, English or German.", "Schreiben Sie uns an [info@code-craft.tech](mailto:info@code-craft.tech) – auf Arabisch, Englisch oder Deutsch."),
             "body": [("btns", [(L("أسئلة شائعة", "Questions and answers", "Häufige Fragen"), "/support/", ""), (L("تنزيل Ultar OS", "Download Ultar OS", "Ultar OS herunterladen"), "/download/", "primary")])]}),
])

# ---- products
page("desktop", "products", "Ultar Desktop",
     lead=L("سطح مكتب هادئ وسريع، بُني بالعربية أولاً – لحاسوبك المكتبي والمحمول.", "A calm and fast desktop, built Arabic first – for your desktop and laptop.", "Ein ruhiger, schneller Desktop, Arabisch zuerst gebaut – für PC und Laptop."),
     blocks=[
    ("sec", {"body": [("split", {
        "h": L("كل شيء في مكانه", "Everything in its place", "Alles an seinem Platz"),
        "ps": [L("مركّب نوافذ خاص (compositor)، شريط في الأسفل مع البحث والتطبيقات، قائمة تطبيقات بفئات، نظرة عامة على النوافذ، إعدادات سريعة، شاشة قفل وحوارات نظام واضحة. أزرار النوافذ في نهاية شريط العنوان – وفي العربية على اليسار.",
               "Its own compositor, a bar at the bottom with search and apps, an app grid with groups, a window overview, quick settings, a lock screen and clear system dialogs. Window buttons sit at the end of the title bar – on the left in Arabic.",
               "Eigener Compositor, eine Leiste unten mit Suche und Apps, App-Raster mit Gruppen, Fensterübersicht, Schnelleinstellungen, Sperrbildschirm und klare Systemdialoge. Fensterknöpfe am Ende der Titelleiste – im Arabischen links.")],
        "list": [L("فاتح وداكن، لون مميّز وحجم نص – يُطبَّق فوراً", "Light and dark, accent colour and text size – applied at once", "Hell und dunkel, Akzentfarbe und Textgröße – wirken sofort"),
                 L("ساعة بالمنطقة الزمنية والتوقيت الصيفي والتاريخ الهجري", "Clock with time zone, daylight saving and Hijri date", "Uhr mit Zeitzone, Sommerzeit und Hidschri-Datum"),
                 L("شاشة ثانية: تمديد أو نسخ أو اختيار الشاشة الرئيسية", "Second screen: extend, mirror or choose the main screen", "Zweiter Bildschirm: erweitern, spiegeln oder Hauptbildschirm wählen")],
        "img": "desktop"})]}),
    ("sec", {"alt": True, "h": L("التطبيقات المرافقة", "Apps that come with it", "Apps, die dabei sind"), "body": [("cards", [
        {"h": L("الملفات", "Files", "Dateien"), "p": L("تصفّح ونسخ وسلة محذوفات ومحرّكات أقراص.", "Browse, copy, trash and drives.", "Durchsuchen, kopieren, Papierkorb und Laufwerke.")},
        {"h": L("محرّر النصوص", "Text editor", "Texteditor"), "p": L("العربية والشيفرة في ملف واحد.", "Arabic and code in one file.", "Arabisch und Code in einer Datei.")},
        {"h": L("الطرفية", "Terminal", "Terminal"), "p": L("أوامر بالعربية والإنجليزية.", "Commands in Arabic and English.", "Befehle auf Arabisch und Englisch.")},
        {"h": L("المتصفح", "Browser", "Browser"), "p": L("يعرض الصفحات اليوم في وضع القراءة؛ المحرّك الكامل Servo قيد التطوير.", "Shows pages in reading mode today; the full Servo engine is in development.", "Zeigt Seiten heute im Lesemodus; die volle Engine Servo ist in Arbeit.")},
        {"h": L("الهاتف", "Phone", "Telefon"), "p": L("مكالمات صوت وصورة مشفّرة عبر رقم Ultar.", "Encrypted voice and video calls over the Ultar number.", "Verschlüsselte Sprach- und Videoanrufe über die Ultar-Nummer.")},
        {"h": L("الإعدادات والمتجر", "Settings and Store", "Einstellungen und Store"), "p": L("كل إعداد في مكان واحد، وكل تطبيق من متجر موقّع.", "Every setting in one place, every app from a signed store.", "Jede Einstellung an einem Ort, jede App aus einem signierten Store.")},
    ], 3), ("shots", [("apps", L("كل التطبيقات", "All apps", "Alle Apps")), ("overview", L("نظرة عامة على النوافذ", "Window overview", "Fensterübersicht")), ("quick", L("الإعدادات السريعة", "Quick settings", "Schnelleinstellungen"))])]}),
    ("sec", {"h": L("تختار ما تريده", "You choose what you want", "Sie wählen, was Sie wollen"),
             "lead": L("أثناء التثبيت تختار المكوّنات، وتضيف الباقي لاحقاً من المتجر أو تزيله دون أثر: أدوات المكتب، أدوات المطوّرين، الوسائط، التواصل، مجموعة الأجهزة، المساعد، وبرامج من أنظمة أخرى في منطقة معزولة.",
                       "During installation you choose the components, and add the rest later from the store or remove it without a trace: office tools, developer tools, media, communication, device mesh, helper, and programs from other systems in an isolated area.",
                       "Bei der Installation wählen Sie die Komponenten und holen den Rest später aus dem Store oder entfernen ihn spurlos: Büro, Entwicklung, Medien, Kommunikation, Geräteverbund, Helfer und Programme anderer Systeme in einem abgeschotteten Bereich."),
             "body": [("note", L("بعض المكوّنات ما زالت قيد التطوير ولا تتوفر بعد كحزم؛ يُظهر المتجر ذلك بوضوح.", "Some components are still in development and not yet available as packages; the store shows that clearly.", "Einige Komponenten sind noch in Arbeit und noch nicht als Paket verfügbar; der Store zeigt das deutlich."))]}),
    ("sec", {"alt": True, "h": L("الحالة بصدق", "Honest status", "Ehrlicher Stand"), "body": [("status", STATUS_NOW), ("note", BETA_NOTE)]}),
    ("sec", {"body": [("btns", [(L("تنزيل Ultar Desktop", "Download Ultar Desktop", "Ultar Desktop herunterladen"), "/download/desktop/", "primary"), (L("دليل التثبيت", "Installation guide", "Installationsanleitung"), "/install/desktop/", "")])]}),
])

page("server", "products", "Ultar Server",
     lead=L("Ultar OS بلا شاشة: صغير، معزول، ويُدار عن بعد بأمان.", "Ultar OS without a screen: small, isolated and managed remotely in safety.", "Ultar OS ohne Bildschirm: klein, abgeschottet und sicher aus der Ferne verwaltet."),
     blocks=[
    ("sec", {"h": L("صورة واحدة، نوع «خادم»", "One image, kind “server”", "Ein Abbild, Art „Server“"),
             "lead": L("لا تحتاج إلى تنزيل منفصل: نزّل صورة التثبيت واختر في الخطوة 6 «نوع التثبيت» ← **خادم**.", "No separate download: get the installer image and choose **server** in step 6 “Kind of installation”.", "Kein eigener Download: das Installationsabbild holen und in Schritt 6 „Art der Installation“ **Server** wählen."),
             "body": [("cards", [
                 {"h": L("SSH بمفاتيح فقط", "SSH with keys only", "SSH nur mit Schlüsseln"),
                  "p": L("خادم SSH مكتوب بلغة Rust يعمل مباشرة على نواة Ultar: أوامر وscp وsftp. لا كلمات مرور – المفاتيح المسجّلة فقط. مفعّل في نوع الخادم.", "An SSH server written in Rust runs directly on the Ultar kernel: commands, scp and sftp. No passwords – registered keys only. On by default in the server kind.", "Ein SSH-Server in Rust läuft direkt auf dem Ultar-Kern: Befehle, scp und sftp. Keine Kennwörter – nur eingetragene Schlüssel. In der Server-Art standardmäßig an.")},
                 {"h": L("إدارة عبر الويب", "Web console", "Web-Verwaltung"),
                  "p": L("واجهة إدارة عبر HTTPS بشهادة خاصة يظهر بصمتها على الشاشة، مع قفل بعد محاولات فاشلة. ومنها تُضاف مفاتيح SSH.", "A management page over HTTPS with its own certificate whose fingerprint is shown on the console, locked after failed attempts. SSH keys are added there.", "Verwaltung über HTTPS mit eigenem Zertifikat, dessen Fingerabdruck auf der Konsole steht, Sperre nach Fehlversuchen. Dort werden SSH-Schlüssel eingetragen.")},
                 {"h": L("شبكة سريعة", "Fast networking", "Schnelles Netz"),
                  "p": L("تعريف 10 جيجابت (Intel E610) بمقاطعات MSI-X، وحماية الذاكرة من الأجهزة عبر VT-d، والشبكة عبر USB-C.", "A 10-gigabit driver (Intel E610) with MSI-X interrupts, memory protection against devices through VT-d, and networking over USB-C.", "10-Gigabit-Treiber (Intel E610) mit MSI-X-Unterbrechungen, Speicherschutz vor Geräten über VT-d und Netz über USB-C.")},
                 {"h": L("خدمات معزولة", "Isolated services", "Abgeschottete Dienste"),
                  "p": L("كل خدمة بمستخدم خاص وحدود ذاكرة وأقل الصلاحيات، وتشرح ملفاتها سبب كل صلاحية.", "Every service with its own user, memory limits and the fewest rights; its file explains every right it asks for.", "Jeder Dienst mit eigenem Benutzer, Speichergrenzen und den wenigsten Rechten; seine Datei begründet jedes Recht.")},
                 {"h": L("جزء من مجموعة الأجهزة", "Part of the device mesh", "Teil des Geräteverbunds"),
                  "p": L("يمكن للخادم أن يقرض قدرته الحسابية أو نموذج الذكاء الاصطناعي لأجهزتك الأخرى – بإذنك.", "The server can lend its computing power or its AI model to your other devices – with your permission.", "Der Server kann Rechenleistung oder sein KI-Modell an Ihre anderen Geräte verleihen – mit Ihrer Erlaubnis.")},
                 {"h": L("تحديثات بلا توقف", "Updates without standstill", "Updates ohne Stillstand"),
                  "p": L("التحديث يُكتب في النسخة الثانية A/B، والخادم يعود تلقائياً إن لم تعمل النسخة الجديدة.", "The update is written into the second A/B copy, and the server falls back by itself if the new one does not come up.", "Das Update geht in die zweite A/B-Kopie; startet die neue nicht, kehrt der Server selbst zurück.")},
             ], 3)]}),
    ("sec", {"alt": True, "h": L("الاتصال من حاسوبك", "Connect from your computer", "Vom eigenen Rechner verbinden"), "body": [
        ("code", "ssh -p 2222 root@<address>\nscp -P 2222 file root@<address>:/data/\nsftp -P 2222 root@<address>"),
        ("note", L("SSH على نواة Ultar والشبكة عبر USB-C ومقاطعات MSI-X جديدة قيد التطوير وتأتي مع الإصدارات القادمة؛ سرعة الاستقبال عبر SSH ما زالت قيد التحسين.", "SSH on the Ultar kernel, networking over USB-C and MSI-X interrupts are new in development and come with the next releases; the SSH receive speed is still being improved.", "SSH auf dem Ultar-Kern, Netz über USB-C und MSI-X-Unterbrechungen sind neu in der Entwicklung und kommen mit den nächsten Ausgaben; das SSH-Empfangstempo wird noch verbessert."))]}),
    ("sec", {"body": [("btns", [(L("تنزيل صورة التثبيت", "Download the installer image", "Installationsabbild herunterladen"), "/download/server/", "primary"), (L("دليل التثبيت", "Installation guide", "Installationsanleitung"), "/install/desktop/", ""), (L("العتاد المختبَر", "Tested hardware", "Geprüfte Hardware"), "/hardware/", "")])]}),
])

page("phone", "products", L("Ultar للهاتف", "Ultar Phone", "Ultar Handy"),
     lead=L("واجهة خاصة بالهاتف، منفصلة عن سطح المكتب، على القاعدة الآمنة نفسها.", "The phone's own interface, separate from the desktop, on the same secure base.", "Eine eigene Handy-Oberfläche, getrennt vom Desktop, auf derselben sicheren Basis."),
     blocks=[
    ("sec", {"h": L("مصمّمة للّمس", "Made for touch", "Für Touch gemacht"), "body": [("cards", [
        {"h": L("شاشة القفل", "Lock screen", "Sperrbildschirm"), "p": L("ساعة بمصفوفة نقاط والتاريخ الهجري. الفتح دائماً عبر خدمة تسجيل الدخول.", "A dot-matrix clock and the Hijri date. Unlocking always goes through the sign-in service.", "Punktmatrix-Uhr und Hidschri-Datum. Entsperren immer über den Anmeldedienst.")},
        {"h": L("الشاشة الرئيسية", "Home screen", "Startbildschirm"), "p": L("صفحات تطبيقات وشريط ثابت وبطاقتا «التالي» و«الجهاز».", "App pages, a dock and the cards “Next” and “Device”.", "App-Seiten, Dock und die Karten „Als Nächstes“ und „Gerät“.")},
        {"h": L("الإعدادات السريعة", "Quick settings", "Schnelleinstellungen"), "p": L("Wi-Fi وبيانات الهاتف وغيرها بلمسة.", "Wi-Fi, mobile data and more with one touch.", "WLAN, mobile Daten und mehr mit einer Berührung.")},
        {"h": L("التطبيقات الأخيرة والبحث", "Recent apps and search", "Zuletzt benutzt und Suche"), "p": L("انتقل بين التطبيقات وابحث في كل شيء.", "Switch between apps and search everything.", "Zwischen Apps wechseln und alles durchsuchen.")},
        {"h": L("مكالمات عبر رقم Ultar", "Calls over the Ultar number", "Anrufe über die Ultar-Nummer"), "p": L("صوت وصورة مشفّران من طرف إلى طرف.", "Voice and video, end-to-end encrypted.", "Sprache und Video, Ende-zu-Ende verschlüsselt.")},
        {"h": L("هاتف يصبح حاسوباً", "A phone that becomes a computer", "Ein Handy wird zum Computer"), "p": L("الهدف: صِل الهاتف بشاشة ولوحة مفاتيح فيظهر سطح المكتب.", "The goal: connect the phone to a screen and keyboard and the desktop appears.", "Ziel: Handy an Bildschirm und Tastatur – der Desktop erscheint.")},
    ], 3)]}),
    ("sec", {"alt": True, "h": L("الحالة بصدق", "Honest status", "Ehrlicher Stand"), "body": [("status", {
        "ok": [L("واجهة الهاتف مختبَرة آلياً بالعربية والإنجليزية والألمانية، فاتحة وداكنة، وكل زر مضغوط", "The phone interface is tested automatically in Arabic, English and German, light and dark, every button pressed", "Die Handy-Oberfläche ist automatisch geprüft auf Arabisch, Englisch und Deutsch, hell und dunkel, jeder Knopf gedrückt"),
               L("صورة USB لأجهزة x86_64 (هواتف وأجهزة لوحية x86، QEMU) – مختبَرة في QEMU", "USB image for x86_64 devices (x86 phones and tablets, QEMU) – tested in QEMU", "USB-Abbild für x86_64-Geräte (x86-Handys und -Tablets, QEMU) – in QEMU geprüft")],
        "no": [L("حزمة Samsung Galaxy A30 لم تُجرَّب بعد على هاتف حقيقي، والهاتف بلا حساب يُفتح دون قفل", "The Samsung Galaxy A30 package has not been started on a real phone yet, and a phone without an account opens without a lock", "Das Paket für das Samsung Galaxy A30 lief noch auf keinem echten Handy, und ein Handy ohne Konto öffnet ohne Sperre"),
               L("نواة Ultar لمعالجات ARM64 لم تكتمل؛ حزمة A30 تعمل على نواة Linux الأصلية من Samsung", "The Ultar kernel for ARM64 is not finished; the A30 package runs on Samsung's own Linux kernel", "Der Ultar-Kern für ARM64 ist nicht fertig; das A30-Paket läuft auf Samsungs eigenem Linux-Kernel"),
               L("الاتصال الخلوي والرسائل القصيرة وUSSD", "Cellular calls, SMS and USSD", "Mobilfunk-Anrufe, SMS und USSD")]})]}),
    ("sec", {"body": [("btns", [(L("تنزيل إصدار الهاتف", "Download the phone release", "Handy-Ausgabe herunterladen"), "/download/phone/", "primary"), (L("دليل الهاتف", "Phone guide", "Anleitung Handy"), "/install/phone/", "")])]}),
])

page("kernel", "products", L("نواة Ultar", "The Ultar kernel", "Der Ultar-Kern"),
     lead=L("نواة خاصة مكتوبة بلغة Rust – أساس كل إصدارات Ultar OS.", "Our own kernel written in Rust – the base of every Ultar OS edition.", "Ein eigener Kern in Rust – die Grundlage aller Ausgaben von Ultar OS."),
     blocks=[
    ("sec", {"body": [("kv", [
        (L("جاهزة خلال", "Ready in", "Bereit nach"), L("نحو 145 ميلي ثانية في المحاكي", "about 145 ms in the emulator", "rund 145 ms im Emulator")),
        (L("الاختبارات", "Tests", "Tests"), L("نحو 2 450 اختباراً آلياً بلا أخطاء", "about 2 450 automatic tests without failures", "rund 2 450 automatische Tests ohne Fehler")),
        (L("اللغة", "Language", "Sprache"), "Rust"),
        (L("المعمارية", "Architecture", "Architektur"), L("x86_64 اليوم؛ ARM64 قيد العمل", "x86_64 today; ARM64 in progress", "x86_64 heute; ARM64 in Arbeit")),
    ])]}),
    ("sec", {"alt": True, "h": L("ما تقدّمه النواة", "What the kernel gives you", "Was der Kern leistet"), "body": [("cards", [
        {"h": L("عزل حقيقي", "Real isolation", "Echte Abschottung"), "p": L("مستخدمون وصلاحيات حقيقية، مساحات أسماء للتركيب والعمليات، وحدود ذاكرة لكل مجموعة.", "Real users and rights, mount and process namespaces, and memory limits per group.", "Echte Benutzer und Rechte, Mount- und PID-Namensräume und Speichergrenzen je Gruppe.")},
        {"h": L("إقلاع A/B", "A/B start", "A/B-Start"), "p": L("محمّل UEFI خاص بنسختين للنواة والنظام، وعدّاد إقلاع يعود إلى النسخة السابقة عند الفشل.", "Its own UEFI loader with two copies of kernel and system, and a start counter that falls back to the previous copy.", "Eigener UEFI-Lader mit zwei Kopien von Kern und System und einem Startzähler, der auf die vorige Kopie zurückfällt.")},
        {"h": L("تعريفات في النواة", "Drivers in the kernel", "Treiber im Kern"), "p": L("USB (لوحة مفاتيح وفأرة وكاميرات وشبكة)، الصوت، الأقراص، شبكة 10 جيجابت، قارئ بطاقات SD، بلوتوث Intel – بعضها ما زال قيد الإكمال.", "USB (keyboard, mouse, cameras, network), sound, disks, 10-gigabit network, SD card reader, Intel Bluetooth – some are still being completed.", "USB (Tastatur, Maus, Kameras, Netz), Ton, Platten, 10-Gigabit-Netz, SD-Kartenleser, Intel-Bluetooth – manche werden noch vervollständigt.")},
        {"h": L("حماية من الأجهزة", "Protection against devices", "Schutz vor Geräten"), "p": L("وحدة IOMMU ‏(VT-d) تمنع الأجهزة من الوصول إلى ذاكرة ليست لها.", "The IOMMU (VT-d) keeps devices away from memory that is not theirs.", "Die IOMMU (VT-d) hält Geräte von fremdem Speicher fern.")},
        {"h": L("أقراص مشفّرة", "Encrypted disks", "Verschlüsselte Platten"), "p": L("النواة تفتح قسم البيانات المشفّر بـ LUKS2 ‏(aes-xts) وتمسح المفاتيح عند الإطفاء.", "The kernel opens the LUKS2 (aes-xts) encrypted data partition and wipes the keys at power-off.", "Der Kern öffnet die mit LUKS2 (aes-xts) verschlüsselte Datenpartition und löscht die Schlüssel beim Ausschalten.")},
        {"h": L("شبكة وSSH خاصّان", "Own network and SSH", "Eigenes Netz und SSH"), "p": L("مكدّس TCP/IP خاص، وخادم SSH بلغة Rust يعمل مباشرة فوق النواة.", "Its own TCP/IP stack, and an SSH server in Rust that runs right on the kernel.", "Eigener TCP/IP-Stapel und ein SSH-Server in Rust direkt auf dem Kern.")},
    ], 3)]}),
    ("sec", {"h": L("سبعة أقسام منفصلة بوضوح", "Seven clearly separated areas", "Sieben klar getrennte Bereiche"),
             "lead": L("النواة، التعريفات، قلب النظام، خدمات النظام، المنصّة، تطبيقات النظام، وتطبيقات الآخرين – لكل قسم حدوده وصلاحياته.", "Kernel, drivers, system core, system services, platform, system apps and third-party apps – each area with its own borders and rights.", "Kern, Treiber, Systemkern, Systemdienste, Plattform, System-Apps und Fremd-Apps – jeder Bereich mit eigenen Grenzen und Rechten."),
             "body": [("btns", [(L("العتاد المختبَر", "Tested hardware", "Geprüfte Hardware"), "/hardware/", "primary"), (L("للمطوّرين", "For developers", "Für Entwickler"), "/developers/", "")])]}),
])

page("mesh", "products", L("مجموعة الأجهزة", "Device mesh", "Geräteverbund"),
     lead=L("هاتفك وحاسوبك وخادمك – فريق واحد. ابدأ على جهاز وتابع على آخر.", "Your phone, computer and server – one team. Start on one device and carry on with another.", "Handy, Rechner und Server – ein Team. Auf einem Gerät beginnen, auf dem anderen weitermachen."),
     blocks=[
    ("sec", {"body": [("split", {
        "h": L("تابع من حيث توقفت", "Continue where you stopped", "Weitermachen, wo Sie aufgehört haben"),
        "ps": [L("ما هو مفتوح على جهاز ينتقل إلى جهاز آخر مقترن. لا شيء يصل دون سؤال: الجهاز الهدف يسألك دائماً، وإذا وُجدت عدة أجهزة يسألك المرسل إلى أيها.",
                 "What is open on one device moves to another paired one. Nothing arrives unasked: the target device always asks you, and with several possible devices the sender asks which one.",
                 "Was auf einem Gerät offen ist, geht auf ein anderes gekoppeltes über. Nichts kommt ungefragt an: Das Zielgerät fragt immer, und bei mehreren Geräten fragt der Sender, welches.")],
        "list": [L("اقتران مرة واحدة برمز من ستة أرقام", "Pair once with a six-digit code", "Einmal koppeln mit einem sechsstelligen Code"),
                 L("كل اتصال مشفّر من طرف إلى طرف", "Every connection end-to-end encrypted", "Jede Verbindung Ende-zu-Ende verschlüsselt"),
                 L("في الشبكة نفسها مباشرة، وفي شبكات مختلفة عبر مرحِّل خادمنا", "Directly in the same network, through our server's relay across networks", "Im selben Netz direkt, über Netze hinweg über das Relais unseres Servers"),
                 L("صلاحيات لكل جهاز، ومفتاح إيقاف حتى على مستوى النواة", "Rights per device, and an off switch down to the kernel", "Rechte je Gerät und ein Aus-Schalter bis in den Kern")],
        "img": "handoff", "alt": L("سؤال الجهاز الهدف قبل المتابعة", "The target device asks before continuing", "Das Zielgerät fragt vor dem Weitermachen")})]}),
    ("sec", {"alt": True, "h": L("الحالة بصدق", "Honest status", "Ehrlicher Stand"), "body": [("status", {
        "ok": [L("الخدمة، صفحة «المجموعة» في الإعدادات مع الاقتران والصلاحيات، بطاقة الإعدادات السريعة، «المتابعة على جهاز آخر» في قائمة التطبيق، والسؤال على الجهاز الهدف", "The service, the “Mesh” page in Settings with pairing and rights, the quick-settings tile, “Continue on another device” in the app menu, and the question on the target device", "Der Dienst, die Seite „Verbund“ in den Einstellungen mit Koppeln und Rechten, die Kachel in den Schnelleinstellungen, „Auf anderem Gerät weitermachen“ im App-Menü und die Frage am Zielgerät"),
               L("مختبَرة ضد خادمنا الحقيقي مع أعطال مفتعلة (انقطاع، تلف، تأخير)", "Tested against our real server with injected faults (drops, damage, delays)", "Geprüft gegen unseren echten Server mit eingebauten Fehlern (Abbruch, Beschädigung, Verzögerung)")],
        "no": [L("التجربة على أجهزة حقيقية، والمشاهدة عن بعد، والصوت والمكالمات عبر المجموعة", "Trying it on real devices, remote view, sound and calls across the mesh", "Erprobung auf echten Geräten, Fernansicht, Ton und Anrufe im Verbund")]})]}),
])

page("assistant", "products", L("المساعد", "The helper", "Der Helfer"),
     lead=L("مساعد يعمل على جهازك نفسه – يفهم كلامك وينفّذ عبر النظام، ولا يفعل شيئاً خطيراً دون موافقتك.", "A helper that runs on your own device – it understands your words and acts through the system, and does nothing dangerous without your consent.", "Ein Helfer auf Ihrem eigenen Gerät – er versteht Ihre Worte, handelt über das System und tut nichts Gefährliches ohne Ihre Zustimmung."),
     blocks=[
    ("sec", {"h": L("ذكي، ومحلي، وتحت سيطرتك", "Smart, local and under your control", "Klug, lokal und unter Ihrer Kontrolle"), "body": [("cards", [
        {"h": L("على جهازك أولاً", "On your device first", "Zuerst auf Ihrem Gerät"), "p": L("بنموذج لغوي على الجهاز نفسه. الجهاز الضعيف يستعير نموذج جهاز مقترن يسمح بذلك.", "With a language model on the device itself. A weak device borrows the model of a paired device that allows it.", "Mit einem Sprachmodell auf dem Gerät selbst. Ein schwaches Gerät leiht sich das Modell eines gekoppelten Geräts, das es erlaubt.")},
        {"h": L("السحابة آخر خيار", "Cloud only as a last resort", "Cloud nur als letzter Weg"), "p": L("فقط إن لم يتوفر نموذج محلي، وفقط بمفتاحك الخاص لدى المزوّد.", "Only when no local model is possible, and only with your own key at the provider.", "Nur wenn kein lokales Modell möglich ist, und nur mit Ihrem eigenen Schlüssel beim Anbieter.")},
        {"h": L("مطفأ حتى تشغّله", "Off until you switch it on", "Aus, bis Sie ihn einschalten"), "p": L("خدمة النموذج لا تعمل إلا إذا فعّلتها، والمساعد لا يتصرف إلا بطلبك.", "The model service runs only when you enabled it, and the helper acts only when asked.", "Der Modelldienst läuft nur, wenn Sie ihn aktiviert haben, und der Helfer handelt nur auf Anfrage.")},
        {"h": L("كل خطوة مرئية", "Every step visible", "Jeder Schritt sichtbar"), "p": L("سجلّ بالخطوات، وصفحة «ما يُسمح للمساعد» مع سجلّ لكل ما فعله.", "A history with the steps, and a page “What the helper may do” with a log of everything it did.", "Verlauf mit sichtbaren Schritten und eine Seite „Was der Helfer darf“ mit Protokoll.")},
    ], 4)]}),
    ("sec", {"alt": True, "h": L("الحالة بصدق", "Honest status", "Ehrlicher Stand"), "body": [("status", {
        "ok": [L("الخدمة مبنية ومختبَرة: تفهم أوامر ثابتة وتجيب عن أسئلة النظام بالعربية والإنجليزية والألمانية دون نموذج", "The service is built and tested: without a model it understands fixed commands and answers questions about the system in Arabic, English and German", "Der Dienst ist gebaut und geprüft: ohne Modell versteht er feste Befehle und beantwortet Fragen zum System auf Arabisch, Englisch und Deutsch")],
        "no": [L("التحكم الصوتي «Hey KM» وكلمة السر الصوتية", "Voice control “Hey KM” and the spoken secret word", "Sprachsteuerung „Hey KM“ und das gesprochene Geheimwort"),
               L("تشغيل النماذج على معالج الرسوميات والمعالج العصبي (NPU) عبر نواة Ultar", "Running models on the GPU and NPU through the Ultar kernel", "Modelle auf GPU und NPU über den Ultar-Kern")]})]}),
])

page("apps", "products", L("المتجر والتطبيقات", "Store and apps", "Store und Apps"),
     lead=L("كل تطبيق موقّع، وكل تطبيق في خليته الخاصة.", "Every app signed, every app in its own cell.", "Jede App signiert, jede App in ihrer eigenen Zelle."),
     blocks=[
    ("sec", {"h": L("متجر Ultar", "Ultar Store", "Ultar Store"), "body": [("cards", [
        {"h": L("موقّع من البداية إلى النهاية", "Signed from end to end", "Durchgehend signiert"), "p": L("الفهرس وكل حزمة موقّعان، وتُفحص كل حزمة مرتين قبل التثبيت – مرة في المتجر ومرة في النظام.", "The catalogue and every package are signed, and every package is checked twice before it is installed – once in the store and once in the system.", "Katalog und jedes Paket sind signiert; jedes Paket wird vor der Installation zweimal geprüft – im Store und im System.")},
        {"h": L("دون حساب ودون إنترنت", "Without account or internet", "Ohne Konto und ohne Internet"), "p": L("عبر HTTPS من خادمنا، أو من ذاكرة USB، أو من النظام نفسه. وإن انقطع الخادم فبآخر فهرس موثوق.", "Over HTTPS from our server, from a USB stick or from the system itself. If the server is unreachable, with the last trusted catalogue.", "Über HTTPS von unserem Server, vom USB-Stick oder aus dem System selbst. Ist der Server weg, mit dem letzten vertrauenswürdigen Katalog.")},
        {"h": L("صلاحيات تعرفها", "Rights you know", "Rechte, die Sie kennen"), "p": L("كل تطبيق عملية مستقلة بمستخدم ومجلد بيانات خاصّين، ولا يحصل إلا على الصلاحيات المذكورة في بيانه الموقّع والتي وافقت عليها.", "Every app is its own process with its own user and data folder, and gets only the rights its signed manifest names and you allowed.", "Jede App ist ein eigener Prozess mit eigenem Benutzer und Datenordner und bekommt nur die Rechte aus ihrem signierten Manifest, die Sie erlaubt haben.")},
    ], 3)]}),
    ("sec", {"alt": True, "h": L("التطبيقات اليوم", "Apps today", "Apps heute"), "body": [("table",
        [L("التطبيق", "App", "App"), L("الحالة", "Status", "Stand")],
        [[L("الإعدادات، المتجر، الملفات", "Settings, Store, Files", "Einstellungen, Store, Dateien"), L("في النظام ومتوفرة كحزم في المتجر", "In the system and available as store packages", "Im System und als Store-Pakete verfügbar")],
         [L("محرّر النصوص، الطرفية", "Text editor, terminal", "Texteditor, Terminal"), L("في النظام", "In the system", "Im System")],
         [L("الهاتف (مكالمات Ultar)", "Phone (Ultar calls)", "Telefon (Ultar-Anrufe)"), L("في النظام، مشفّر من طرف إلى طرف", "In the system, end-to-end encrypted", "Im System, Ende-zu-Ende verschlüsselt")],
         [L("المتصفح", "Browser", "Browser"), L("وضع القراءة اليوم؛ محرّك Servo الكامل قيد التطوير", "Reading mode today; the full Servo engine in development", "Heute Lesemodus; volle Engine Servo in Arbeit")],
         [L("المساعد", "Helper", "Helfer"), L("في النظام، مطفأ حتى تشغّله", "In the system, off until you switch it on", "Im System, aus bis zum Einschalten")],
         [L("App-Builder للمطوّرين", "App-Builder for developers", "App-Builder für Entwickler"), L("في المتجر", "In the store", "Im Store")],
         [L("المكتب: PDF وWord وExcel", "Office: PDF, Word, Excel", "Büro: PDF, Word, Excel"), L("قيد التطوير", "In development", "In Arbeit")]])]}),
    ("sec", {"body": [("btns", [(L("للمطوّرين", "For developers", "Für Entwickler"), "/developers/", "primary"), (L("الأمان", "Security", "Sicherheit"), "/security/", "")])]}),
])

page("security", "products", L("الأمان والخصوصية", "Security and privacy", "Sicherheit und Datenschutz"),
     lead=L("جهازك لا يثق بخادمنا: كل ما يُثبَّت يفحصه الجهاز بنفسه.", "Your device does not trust our server: it checks everything it installs itself.", "Ihr Gerät vertraut unserem Server nicht: Es prüft alles, was es installiert, selbst."),
     blocks=[
    ("sec", {"h": L("تواقيع في كل مكان", "Signatures everywhere", "Signaturen überall"), "body": [("checks", [
        L("صور النظام وتطبيقات النظام والمتجر موقّعة بمفاتيح منفصلة. يحفظها على خادمنا برنامج توقيع مستقل بمستخدم خاص، لا يوقّع إلا بصمات بصيغة ثابتة، بحدّ لعدد التواقيع، ويسجّل كل توقيع.",
          "System images, system apps and the store are signed with separate keys. On our server they are held by a separate signing program with its own user that signs only digests in a fixed format, with a limit on the number of signatures, and logs every signature.",
          "Systemabbilder, System-Apps und Store sind mit getrennten Schlüsseln signiert. Auf unserem Server verwahrt sie ein eigenes Signierprogramm mit eigenem Benutzer, das nur Prüfsummen in festem Format signiert, die Anzahl begrenzt und jede Signatur protokolliert."),
        L("تحديثات النظام لها مفتاحها الخاص: مفتاح تطبيق لا يستطيع توقيع تحديث للنظام. وتمنع الحماية الرجوع إلى نسخة أقدم.",
          "System updates have their own key: an app key can never sign a system update. Rollback protection stops a return to older versions.",
          "System-Updates haben einen eigenen Schlüssel: ein App-Schlüssel kann nie ein System-Update signieren. Ein Rückfallschutz verhindert ältere Fassungen."),
        L("الجهاز يتحقق من كل توقيع بمفاتيح الثقة المدمجة ويرفض غير الموقّع أو الموقّع بمفتاح ملغى – مهما قال الخادم.",
          "The device checks every signature against the trusted keys built into the system and refuses anything unsigned or signed with a revoked key – whatever the server says.",
          "Das Gerät prüft jede Signatur gegen die eingebauten vertrauenswürdigen Schlüssel und lehnt Unsigniertes oder mit widerrufenem Schlüssel Signiertes ab – egal, was der Server sagt."),
        L("لا يوجد على الخادم أي وصول عن بعد إلى الأجهزة: لا أوامر، لا تحكم، لا تثبيت صامت.",
          "The server has no remote access to devices: no commands, no control, no silent installs.",
          "Der Server hat keinen Fernzugriff auf Geräte: keine Befehle, keine Steuerung, keine stillen Installationen."),
    ])]}),
    ("sec", {"alt": True, "h": L("مكالمات لا يعرف عنها الخادم شيئاً", "Calls the server knows nothing about", "Anrufe, von denen der Server nichts weiß"), "body": [("checks", [
        L("المفاتيح والصوت والصورة بين الجهازين فقط (X25519 وChaCha20-Poly1305)، موقّعة بمفتاح الجهاز.", "Keys, voice and picture stay between the two devices (X25519, ChaCha20-Poly1305), signed with the device key.", "Schlüssel, Stimme und Bild bleiben zwischen den beiden Geräten (X25519, ChaCha20-Poly1305), signiert mit dem Geräteschlüssel."),
        L("يرى الطرفان رقم أمان واحداً من 60 خانة للمقارنة، وتحذيراً إذا تغيّر المفتاح.", "Both sides see one 60-digit safety number to compare, and a warning if a key changes.", "Beide sehen dieselbe 60-stellige Sicherheitsnummer zum Vergleichen und eine Warnung, wenn sich ein Schlüssel ändert."),
        L("الخادم يوصل الطرفين فقط: لا يحفظ ولا يسجّل شيئاً عن المكالمة، ويمكن للطرفين إخفاء عنوانيهما عبر المرحِّل.", "The server only connects the two: it neither keeps nor logs anything about the call, and both sides can hide their addresses behind the relay.", "Der Server verbindet nur: Er speichert und protokolliert nichts über den Anruf, und beide Seiten können ihre Adressen hinter dem Relais verbergen."),
    ])]}),
    ("sec", {"h": L("على جهازك", "On your device", "Auf Ihrem Gerät"), "body": [("checks", [
        L("كل تطبيق وكل خدمة بأقل الصلاحيات، والحوارات المهمة يصدرها النظام وتظهر بعلامة واضحة.", "Every app and every service with the fewest rights; important dialogs come from the system and are clearly marked.", "Jede App und jeder Dienst mit den wenigsten Rechten; wichtige Dialoge kommen vom System und sind deutlich gekennzeichnet."),
        L("تشفير قسم البيانات بعبارة مرور (جديد قيد التطوير).", "Encryption of the data partition with a passphrase (new in development).", "Verschlüsselung der Datenpartition mit Passphrase (neu in der Entwicklung)."),
        L("التشخيص والموقع مطفآن من البداية.", "Diagnostics and location are off from the start.", "Diagnose und Standort sind von Anfang an aus."),
    ])]}),
    ("sec", {"alt": True, "h": L("تحقّق بنفسك", "Check it yourself", "Selbst prüfen"),
             "lead": L("كل ملف للتنزيل يأتي مع بصمة SHA-256 وتوقيع Ed25519. [هكذا تتحقق](/install/verify/).", "Every download comes with a SHA-256 fingerprint and an Ed25519 signature. [This is how you check](/install/verify/).", "Jeder Download kommt mit SHA-256-Fingerabdruck und Ed25519-Signatur. [So prüfen Sie](/install/verify/)."),
             "body": [("table", [L("المفتاح", "Key", "Schlüssel"), L("معرّف المفتاح", "Key id", "Schlüssel-ID")], [
                 [L("صور النظام (الإصدارات)", "System images (releases)", "Systemabbilder (Ausgaben)"), "`66a1912229841a19`"],
                 [L("تطبيقات النظام", "System apps", "System-Apps"), "`54d9f4ea441a5da2`"],
                 [L("المتجر", "Store", "Store"), "`68b4f13ebe621352`"]])]}),
    ("sec", {"id": "report", "h": L("الإبلاغ عن ثغرة أمنية", "Report a security issue", "Sicherheitslücke melden"),
             "lead": L("وجدت ثغرة؟ اكتب إلى [info@code-craft.tech](mailto:info@code-craft.tech) وضع في الموضوع «Security». صف الخطوات والإصدار والجهاز. لا تنشر التفاصيل قبل أن نصلحها.",
                       "Found a weakness? Write to [info@code-craft.tech](mailto:info@code-craft.tech) with “Security” in the subject. Describe the steps, the release and the device. Please do not publish details before we have fixed it.",
                       "Eine Lücke gefunden? Schreiben Sie an [info@code-craft.tech](mailto:info@code-craft.tech) mit „Security“ im Betreff. Beschreiben Sie Schritte, Ausgabe und Gerät. Bitte keine Einzelheiten veröffentlichen, bevor wir sie behoben haben.")}),
])

page("hardware", "products", L("العتاد المختبَر", "Tested hardware", "Geprüfte Hardware"),
     lead=L("على أي أجهزة يعمل Ultar OS اليوم – بصدق.", "Which devices Ultar OS runs on today – honestly.", "Auf welchen Geräten Ultar OS heute läuft – ehrlich."),
     blocks=[
    ("sec", {"body": [("table", [L("الجهاز", "Device", "Gerät"), L("الحالة", "Status", "Stand"), L("ملاحظة", "Note", "Hinweis")], [
        [L("آلة افتراضية: QEMU وVMware ‏(x86_64، UEFI)", "Virtual machine: QEMU and VMware (x86_64, UEFI)", "Virtuelle Maschine: QEMU und VMware (x86_64, UEFI)"), L("✓ يعمل", "✓ Works", "✓ Läuft"),
         L("الطريقة الموصى بها للتجربة. الإقلاع وسطح المكتب والتثبيت مختبَرة آلياً.", "The recommended way to try it. Start, desktop and installation are tested automatically.", "Der empfohlene Weg zum Ausprobieren. Start, Desktop und Installation automatisch geprüft.")],
        ["Beelink GTi15 Ultra (Intel Core Ultra 9 285H)", L("◐ قيد الاختبار", "◐ Being tested", "◐ In Prüfung"),
         L("المرحلة الأولى ناجحة مع المعالج الحقيقي (16 نواة). شبكة 10 جيجابت (E610) وبلوتوث BE200 وقارئ SD تعمل على العتاد الحقيقي؛ عرض الرسوميات وWi-Fi والإقلاع المباشر قيد العمل.",
           "Stage 1 passed on the real processor (16 cores). 10-gigabit network (E610), Bluetooth BE200 and the SD reader work on the real hardware; graphics output, Wi-Fi and starting directly on it are in progress.",
           "Stufe 1 auf dem echten Prozessor (16 Kerne) bestanden. 10-Gigabit-Netz (E610), Bluetooth BE200 und SD-Leser arbeiten auf der echten Hardware; Grafikausgabe, WLAN und direkter Start in Arbeit.")],
        ["Lenovo ThinkPad P1", L("○ مخطَّط", "○ Planned", "○ Geplant"), L("جهاز اختبار؛ لم يُختبر بعد.", "Test device; not tested yet.", "Testgerät; noch nicht geprüft.")],
        ["Samsung Galaxy A30 (SM-A305F)", L("◐ تجريبي", "◐ Experimental", "◐ Experimentell"),
         L("حزمة الهاتف على نواة Samsung؛ لم تُجرَّب بعد على الهاتف.", "Phone package on Samsung's kernel; not yet started on the phone.", "Handy-Paket auf Samsungs Kernel; noch nicht auf dem Handy gestartet.")],
        [L("كاميرات USB ‏(UVC، MJPEG)", "USB cameras (UVC, MJPEG)", "USB-Kameras (UVC, MJPEG)"), L("✓ يعمل", "✓ Works", "✓ Läuft"),
         L("للمكالمات بالصورة؛ مختبَرة بكاميرا Logitech C920.", "For video calls; tested with a Logitech C920.", "Für Videoanrufe; geprüft mit einer Logitech C920.")],
        [L("لوحة مفاتيح وفأرة USB", "USB keyboard and mouse", "USB-Tastatur und -Maus"), L("✓ يعمل", "✓ Works", "✓ Läuft"), L("مع التوصيل والفصل أثناء العمل.", "Including plugging in and out while running.", "Mit Ein- und Ausstecken im Betrieb.")],
        [L("شبكة عبر USB-C ‏(RNDIS، CDC-ECM)", "Network over USB-C (RNDIS, CDC-ECM)", "Netz über USB-C (RNDIS, CDC-ECM)"), L("◐ جديد", "◐ New", "◐ Neu"), L("مثلاً مشاركة الإنترنت من هاتف؛ مختبَرة في المحاكي.", "For example tethering from a phone; tested in the emulator.", "Etwa Tethering vom Handy; im Emulator geprüft.")],
    ])]}),
    ("sec", {"alt": True, "h": L("المتطلبات", "Requirements", "Voraussetzungen"), "body": [("checks", [
        L("حاسوب x86_64 بنظام UEFI، مع إيقاف Secure Boot (المحمّل غير موقّع له بعد)", "An x86_64 computer with UEFI, Secure Boot turned off (the loader is not signed for it yet)", "Ein x86_64-Rechner mit UEFI, Secure Boot aus (der Lader ist dafür noch nicht signiert)"),
        L("ذاكرة USB بسعة 8 GB على الأقل للتثبيت", "A USB stick with at least 8 GB for the installation", "Ein USB-Stick mit mindestens 8 GB für die Installation"),
        L("قرص فارغ أو جهاز اختبار – أو مساحة حرة بجانب نظام موجود", "An empty disk or a test device – or free space beside an existing system", "Eine leere Platte oder ein Testgerät – oder freier Platz neben einem vorhandenen System"),
    ], "dot"), ("p", L("هل جرّبت Ultar OS على جهاز آخر؟ أخبرنا بالنتيجة على [info@code-craft.tech](mailto:info@code-craft.tech).", "Tried Ultar OS on another device? Tell us how it went at [info@code-craft.tech](mailto:info@code-craft.tech).", "Ultar OS auf einem anderen Gerät ausprobiert? Schreiben Sie uns das Ergebnis an [info@code-craft.tech](mailto:info@code-craft.tech)."))]}),
])

page("developers", "products", L("للمطوّرين", "For developers", "Für Entwickler"),
     lead=L("ابنِ تطبيقات لـ Ultar OS وانشرها موقّعة في المتجر.", "Build apps for Ultar OS and publish them signed in the store.", "Apps für Ultar OS bauen und signiert im Store veröffentlichen."),
     blocks=[
    ("sec", {"h": L("كيف يُبنى تطبيق", "How an app is built", "Wie eine App aufgebaut ist"), "body": [("cards", [
        {"h": L("الواجهة تطبيق، والقواعد خدمة", "Interface = app, rules = service", "Oberfläche = App, Regeln = Dienst"), "p": L("التطبيق يعرض ويستقبل الإدخال؛ كل ما يخص العتاد والشبكة والحسابات تفعله خدمات النظام عبر ناقل النظام.", "The app shows and takes input; everything about hardware, network and accounts is done by system services over the system bus.", "Die App zeigt an und nimmt Eingaben an; alles zu Hardware, Netz und Konten erledigen Systemdienste über den Systembus.")},
        {"h": L("بيان موقّع", "A signed manifest", "Ein signiertes Manifest"), "p": L("ملف `ultar-app.toml` يذكر كل صلاحية مع سببها؛ لا يحصل التطبيق على أكثر منها.", "The file `ultar-app.toml` names every right with its reason; the app never gets more.", "Die Datei `ultar-app.toml` nennt jedes Recht mit Begründung; mehr bekommt die App nie.")},
        {"h": L("حزمة ‎.uap", "A .uap package", "Ein .uap-Paket"), "p": L("حزمة موقّعة بمفتاح الناشر؛ النظام يرفض أي حزمة غير موقّعة أو معدّلة.", "A package signed with the publisher's key; the system refuses any unsigned or changed package.", "Ein mit dem Herausgeber-Schlüssel signiertes Paket; das System lehnt unsignierte oder veränderte Pakete ab.")},
        {"h": L("العربية أولاً", "Arabic first", "Arabisch zuerst"), "p": L("كل النصوص في ملفات الترجمة (ar وen وde)، والعربية هي المرجع، وكل تخطيط يفكّر في البداية والنهاية لا في اليمين واليسار.", "All texts in translation files (ar, en, de), Arabic is the reference, and every layout thinks in start and end, not left and right.", "Alle Texte in Übersetzungsdateien (ar, en, de), Arabisch ist die Referenz, jedes Layout denkt in Anfang und Ende statt links und rechts.")},
    ], 4)]}),
    ("sec", {"alt": True, "h": L("أدوات", "Tools", "Werkzeuge"), "body": [("cards", [
        {"h": "App-Builder", "p": L("تطبيق في المتجر: يفحص تطبيقك ويحزمه ويتحقق منه قبل التثبيت، ويعرض كل مشكلة.", "An app in the store: it checks, packages and verifies your app before it is installed, and lists every issue.", "Eine App im Store: prüft, verpackt und verifiziert Ihre App vor der Installation und zeigt jedes Problem.")},
        {"h": L("SSH إلى الجهاز", "SSH to the device", "SSH zum Gerät"), "p": L("الإعدادات ← النظام ← خيارات المطوّرين: ادخل إلى جهازك بمفتاحك عبر المنفذ 2222 (قيد التطوير).", "Settings → System → Developer options: reach your device with your key on port 2222 (in development).", "Einstellungen → System → Entwickleroptionen: mit Ihrem Schlüssel über Port 2222 aufs Gerät (in Arbeit).")},
        {"h": L("مفتاح ناشر", "A publisher key", "Ein Herausgeber-Schlüssel"), "p": L("لنشر تطبيقك في متجر Ultar، اكتب إلينا على [info@code-craft.tech](mailto:info@code-craft.tech).", "To publish your app in the Ultar Store, write to [info@code-craft.tech](mailto:info@code-craft.tech).", "Um Ihre App im Ultar Store zu veröffentlichen, schreiben Sie an [info@code-craft.tech](mailto:info@code-craft.tech).")},
    ], 3)]}),
])

page("account", "support", L("حساب Ultar", "Ultar account", "Ultar-Konto"),
     lead=L("حساب Ultar اختياري: الجهاز يعمل كاملاً بدونه.", "The Ultar account is optional: the device works fully without it.", "Das Ultar-Konto ist freiwillig: Das Gerät funktioniert auch ohne vollständig."),
     blocks=[
    ("sec", {"h": L("ما الذي يقدّمه الحساب", "What the account gives you", "Was das Konto bringt"), "body": [("cards", [
        {"h": L("رقم Ultar", "Ultar number", "Ultar-Nummer"), "p": L("رقم بصيغة `UOS-XXXX-XXXX` لمكالمات صوت وصورة مشفّرة من طرف إلى طرف.", "A number like `UOS-XXXX-XXXX` for end-to-end encrypted voice and video calls.", "Eine Nummer wie `UOS-XXXX-XXXX` für Ende-zu-Ende verschlüsselte Sprach- und Videoanrufe.")},
        {"h": L("المجموعة عبر الإنترنت", "Mesh over the internet", "Verbund über das Internet"), "p": L("أجهزتك تجد بعضها حتى في شبكات مختلفة، عبر مرحِّل خادمنا.", "Your devices find each other even in different networks, through our server's relay.", "Ihre Geräte finden sich auch in verschiedenen Netzen, über das Relais unseres Servers.")},
        {"h": L("أجهزتك", "Your devices", "Ihre Geräte"), "p": L("لكل جهاز مفتاحه الخاص؛ الجهاز الجديد يحتاج إلى موافقتك، والاسترداد يأتي بمهلة انتظار وتحذير على كل أجهزتك.", "Every device has its own key; a new device needs your approval, and recovery comes with a waiting time and a warning on all your devices.", "Jedes Gerät hat einen eigenen Schlüssel; ein neues Gerät braucht Ihre Freigabe, und eine Wiederherstellung kommt mit Wartezeit und Warnung auf allen Geräten.")},
    ], 3), ("note", L("المتجر وتحديثات النظام لا تحتاج إلى حساب.", "The store and system updates need no account.", "Store und System-Updates brauchen kein Konto."))]}),
    ("sec", {"alt": True, "h": L("كيف تنشئ حساباً", "How to create an account", "So legen Sie ein Konto an"), "body": [("steps", [
        {"h": L("افتح الإعدادات", "Open Settings", "Einstellungen öffnen"), "p": L("الإعدادات ← الحسابات ← حساب Ultar.", "Settings → Accounts → Ultar account.", "Einstellungen → Konten → Ultar-Konto.")},
        {"h": L("أنشئ الحساب أو سجّل الدخول", "Create the account or sign in", "Konto anlegen oder anmelden"), "p": L("اسم المستخدم وكلمة المرور. يُنشأ مفتاح الجهاز على جهازك ولا يغادره.", "User name and password. The device key is made on your device and never leaves it.", "Benutzername und Kennwort. Der Geräteschlüssel entsteht auf Ihrem Gerät und verlässt es nie.")},
        {"h": L("فعّل ما تريده", "Switch on what you want", "Einschalten, was Sie wollen"), "p": L("استقبال مكالمات Ultar والمجموعة عبر الإنترنت – لكل خدمة مفتاح منفصل، واستقبال المكالمات مطفأ من البداية.", "Receiving Ultar calls and the mesh over the internet – each service has its own switch, and receiving calls is off from the start.", "Ultar-Anrufe annehmen und Verbund über das Internet – jeder Dienst hat einen eigenen Schalter, Anrufe annehmen ist anfangs aus.")},
    ]), ("note", L("إدارة الحساب والأجهزة اليوم من إعدادات الجهاز. إدارتها عبر الويب مخطَّطة لاحقاً.", "Today you manage the account and devices in the device's Settings. Managing them on the web is planned for later.", "Konto und Geräte verwalten Sie heute in den Einstellungen des Geräts. Eine Verwaltung im Web ist für später geplant."))]}),
])

# ---- get Ultar
page("download", "download", L("تنزيل Ultar OS", "Get Ultar OS", "Ultar OS holen"),
     lead=L("اختر الإصدار المناسب لجهازك. كل ملف موقّع ومعه بصمته.", "Choose the edition for your device. Every file is signed and comes with its fingerprint.", "Wählen Sie die Ausgabe für Ihr Gerät. Jede Datei ist signiert und kommt mit Fingerabdruck."),
     blocks=[
    ("sec", {"body": [("cards", [
        {"k": "Desktop", "h": L("للحاسوب المكتبي والمحمول", "For desktop and laptop", "Für PC und Laptop"), "p": L("صورة تثبيت USB لحواسيب x86_64 بنظام UEFI.", "USB installer image for x86_64 UEFI computers.", "USB-Installationsabbild für x86_64-UEFI-Rechner."), "href": "/download/desktop/", "more": L("تنزيل Desktop", "Download Desktop", "Desktop herunterladen")},
        {"k": "Server", "h": L("للخادم", "For servers", "Für Server"), "p": L("الصورة نفسها – اختر «خادم» أثناء التثبيت.", "The same image – choose “server” during installation.", "Dasselbe Abbild – bei der Installation „Server“ wählen."), "href": "/download/server/", "more": L("تنزيل Server", "Download Server", "Server herunterladen")},
        {"k": L("الهاتف", "Phone", "Handy"), "h": L("للهاتف – تجريبي", "For phones – experimental", "Fürs Handy – experimentell"), "p": L("صورة USB لأجهزة x86_64 وحزمة Galaxy A30.", "USB image for x86_64 devices and a Galaxy A30 package.", "USB-Abbild für x86_64-Geräte und ein Galaxy-A30-Paket."), "href": "/download/phone/", "more": L("تنزيل إصدار الهاتف", "Download phone release", "Handy-Ausgabe herunterladen")},
        {"k": L("آلة افتراضية", "Virtual machine", "Virtuelle Maschine"), "h": L("للتجربة بأمان", "To try it safely", "Gefahrlos ausprobieren"), "p": L("شغّل Ultar OS في QEMU أو VMware دون لمس قرصك.", "Run Ultar OS in QEMU or VMware without touching your disk.", "Ultar OS in QEMU oder VMware starten, ohne Ihre Platte anzufassen."), "href": "/download/vm/", "more": L("التجربة في آلة افتراضية", "Try in a virtual machine", "In einer VM ausprobieren")},
    ], 4)]}),
    ("sec", {"alt": True, "h": L("أحدث الإصدارات", "Newest releases", "Neueste Ausgaben"), "body": [("rel", "latest"), ("note", BETA_NOTE)]}),
    ("sec", {"h": L("بعد التنزيل", "After the download", "Nach dem Download"), "body": [("cards", [
        {"h": L("تحقّق من الملف", "Verify the file", "Datei prüfen"), "p": L("بصمة SHA-256 وتوقيع Ed25519.", "SHA-256 fingerprint and Ed25519 signature.", "SHA-256-Fingerabdruck und Ed25519-Signatur."), "href": "/install/verify/"},
        {"h": L("ثبّت النظام", "Install the system", "System installieren"), "p": L("خطوة بخطوة عبر ذاكرة USB.", "Step by step from a USB stick.", "Schritt für Schritt vom USB-Stick."), "href": "/install/desktop/"},
        {"h": L("حدّثه لاحقاً", "Update it later", "Später aktualisieren"), "p": L("من الإعدادات، بنسختين A/B.", "From Settings, on two A/B copies.", "Aus den Einstellungen, auf zwei A/B-Kopien."), "href": "/update/"},
    ], 3)]}),
])

page("download/desktop", "download", L("تنزيل Ultar Desktop", "Download Ultar Desktop", "Ultar Desktop herunterladen"),
     lead=L("صورة تثبيت USB لحواسيب x86_64 بنظام UEFI.", "USB installer image for x86_64 computers with UEFI.", "USB-Installationsabbild für x86_64-Rechner mit UEFI."),
     blocks=[
    ("sec", {"body": [("rel", "desktop"), ("note", BETA_NOTE)]}),
    ("sec", {"alt": True, "h": L("تحتاج إلى", "You need", "Sie brauchen"), "body": [("checks", [
        L("حاسوب x86_64 بنظام UEFI وSecure Boot مطفأ", "An x86_64 computer with UEFI and Secure Boot off", "Einen x86_64-Rechner mit UEFI und Secure Boot aus"),
        L("ذاكرة USB بسعة 8 GB على الأقل (سيُمسح كل ما عليها)", "A USB stick with at least 8 GB (everything on it is erased)", "Einen USB-Stick mit mindestens 8 GB (alles darauf wird gelöscht)"),
        L("قرص فارغ أو جهاز اختبار", "An empty disk or a test device", "Eine leere Platte oder ein Testgerät"),
    ], "dot"), ("btns", [(L("دليل التثبيت", "Installation guide", "Installationsanleitung"), "/install/desktop/", "primary"), (L("تحقّق من التنزيل", "Verify the download", "Download prüfen"), "/install/verify/", ""), (L("العتاد المختبَر", "Tested hardware", "Geprüfte Hardware"), "/hardware/", "")])]}),
])

page("download/server", "download", L("تنزيل Ultar Server", "Download Ultar Server", "Ultar Server herunterladen"),
     lead=L("صورة التثبيت نفسها – اختر «خادم» في الخطوة 6.", "The same installer image – choose “server” in step 6.", "Dasselbe Installationsabbild – in Schritt 6 „Server“ wählen."),
     blocks=[
    ("sec", {"body": [("rel", "server"), ("note", BETA_NOTE)]}),
    ("sec", {"alt": True, "h": L("التثبيت كخادم", "Installing as a server", "Als Server installieren"), "body": [("steps", [
        {"h": L("اكتب الصورة على ذاكرة USB وأقلع منها", "Write the image to a USB stick and start from it", "Abbild auf einen USB-Stick schreiben und davon starten"), "p": L("كما في [دليل التثبيت](/install/desktop/).", "As in the [installation guide](/install/desktop/).", "Wie in der [Installationsanleitung](/install/desktop/).")},
        {"h": L("اختر «خادم» في «نوع التثبيت»", "Choose “server” in “Kind of installation”", "In „Art der Installation“ „Server“ wählen"), "p": L("الخطوة 6 من 16.", "Step 6 of 16.", "Schritt 6 von 16.")},
        {"h": L("أضف مفتاح SSH", "Add an SSH key", "SSH-Schlüssel eintragen"), "p": L("بعد الإقلاع الأول، عبر إدارة الويب؛ ثم ادخل بالمفتاح عبر المنفذ 2222.", "After the first start, through the web console; then connect with the key on port 2222.", "Nach dem ersten Start über die Web-Verwaltung; dann mit dem Schlüssel über Port 2222 verbinden.")},
    ]), ("btns", [("Ultar Server", "/server/", "")])]}),
])

page("download/phone", "download", L("تنزيل إصدار الهاتف", "Download the phone release", "Handy-Ausgabe herunterladen"),
     lead=L("إصدار تجريبي لمن يريد التجربة على جهاز اختبار.", "An experimental release for anyone who wants to try it on a test device.", "Eine experimentelle Ausgabe zum Ausprobieren auf einem Testgerät."),
     blocks=[
    ("sec", {"body": [("rel", "phone"),
        ("note", L("حزمة Galaxy A30 لم تُجرَّب بعد على هاتف حقيقي. اكتبها فقط على هاتف اختبار وبعد نسخة احتياطية كاملة.", "The Galaxy A30 package has not been started on a real phone yet. Write it only to a test phone and after a full backup.", "Das Galaxy-A30-Paket lief noch auf keinem echten Handy. Nur auf ein Testhandy und nach einer Vollsicherung schreiben."))]}),
    ("sec", {"alt": True, "body": [("btns", [(L("دليل الهاتف", "Phone guide", "Anleitung Handy"), "/install/phone/", "primary"), (L("Ultar للهاتف", "Ultar Phone", "Ultar Handy"), "/phone/", "")])]}),
])

page("download/vm", "download", L("Ultar OS في آلة افتراضية", "Ultar OS in a virtual machine", "Ultar OS in einer virtuellen Maschine"),
     lead=L("أسهل طريقة وأكثرها أماناً للتجربة: لا شيء يتغيّر على قرصك.", "The easiest and safest way to try it: nothing changes on your disk.", "Der einfachste und sicherste Weg zum Ausprobieren: Auf Ihrer Platte ändert sich nichts."),
     blocks=[
    ("sec", {"body": [("rel", "vm"),
        ("p", L("إن لم يظهر هنا ملف خاص بـ VMware، فاستخدم صورة التثبيت العادية – [الدليل](/install/vm/) يشرح الطريقتين.", "If no VMware file is shown here, use the normal installer image – the [guide](/install/vm/) explains both ways.", "Steht hier keine eigene VMware-Datei, nehmen Sie das normale Installationsabbild – die [Anleitung](/install/vm/) erklärt beide Wege."))]}),
    ("sec", {"alt": True, "body": [("btns", [(L("دليل الآلة الافتراضية", "Virtual machine guide", "Anleitung VM"), "/install/vm/", "primary"), (L("صورة التثبيت", "Installer image", "Installationsabbild"), "/download/desktop/", "")])]}),
])

page("releases", "download", L("الإصدارات والحالة", "Releases and status", "Ausgaben und Stand"),
     lead=L("كل إصدار مع ملاحظاته وملفاته وتواقيعه، والحالة الصادقة للنظام.", "Every release with its notes, files and signatures, and the honest status of the system.", "Jede Ausgabe mit Hinweisen, Dateien und Signaturen, dazu der ehrliche Stand des Systems."),
     blocks=[
    ("sec", {"h": L("الحالة اليوم", "Status today", "Stand heute"), "body": [("status", STATUS_NOW)]}),
    ("sec", {"alt": True, "h": L("قنوات الإصدار", "Release channels", "Ausgabekanäle"), "body": [("table", [L("القناة", "Channel", "Kanal"), L("المعنى", "Meaning", "Bedeutung")], [
        ["stable", L("قناة التحديثات عبر الشبكة للأجهزة المثبّتة.", "The channel for network updates of installed devices.", "Kanal der Netz-Updates für installierte Geräte.")],
        ["beta", L("صورة التثبيت الحالية للحاسوب – للتجربة.", "The current installer image for computers – for testing.", "Das aktuelle Installationsabbild für Rechner – zum Testen.")],
        ["experimental", L("إصدارات مبكّرة مثل إصدار الهاتف – لأجهزة الاختبار فقط.", "Early releases such as the phone release – for test devices only.", "Frühe Ausgaben wie die Handy-Ausgabe – nur für Testgeräte.")]]),
        ("p", L("رقم الإصدار بصيغة `major.minor.patch`؛ لكل إصدار رقم أعلى، والجهاز لا يقبل نسخة أقدم من التي عليه.", "Versions are `major.minor.patch`; every release has a higher number, and a device never accepts an older version than it runs.", "Versionen haben die Form `major.minor.patch`; jede Ausgabe hat eine höhere Nummer, und ein Gerät nimmt nie eine ältere Fassung an."))]}),
    ("sec", {"h": L("كل الإصدارات", "All releases", "Alle Ausgaben"), "body": [("rel", "all")]}),
])

# ---- guides
page("install", "install", L("الأدلّة", "Guides", "Anleitungen"),
     lead=L("من التنزيل حتى التحديث – خطوة بخطوة.", "From download to update – step by step.", "Vom Download bis zum Update – Schritt für Schritt."),
     blocks=[
    ("sec", {"body": [("cards", [
        {"h": L("التثبيت على حاسوب", "Install on a computer", "Auf einem Rechner installieren"), "p": L("Desktop أو Server عبر ذاكرة USB – الخطوات الست عشرة كلها.", "Desktop or server from a USB stick – all sixteen steps.", "Desktop oder Server vom USB-Stick – alle sechzehn Schritte."), "href": "/install/desktop/"},
        {"h": L("في آلة افتراضية", "In a virtual machine", "In einer virtuellen Maschine"), "p": L("QEMU وVMware – للتجربة دون خطر.", "QEMU and VMware – to try without risk.", "QEMU und VMware – ohne Risiko ausprobieren."), "href": "/install/vm/"},
        {"h": L("على الهاتف", "On a phone", "Auf dem Handy"), "p": L("Samsung Galaxy A30 وأجهزة x86_64 – تجريبي.", "Samsung Galaxy A30 and x86_64 devices – experimental.", "Samsung Galaxy A30 und x86_64-Geräte – experimentell."), "href": "/install/phone/"},
        {"h": L("التحقق من التنزيل", "Verify a download", "Download prüfen"), "p": L("بصمة SHA-256 وتوقيع Ed25519.", "SHA-256 fingerprint and Ed25519 signature.", "SHA-256-Fingerabdruck und Ed25519-Signatur."), "href": "/install/verify/"},
        {"h": L("تحديث النظام", "Update the system", "System aktualisieren"), "p": L("عبر الشبكة مع رجوع تلقائي.", "Over the network with automatic fallback.", "Über das Netz mit automatischem Rückfall."), "href": "/update/"},
        {"h": L("حساب Ultar", "Ultar account", "Ultar-Konto"), "p": L("الرقم والمكالمات والمجموعة.", "Number, calls and mesh.", "Nummer, Anrufe und Verbund."), "href": "/account/"},
    ], 3)]}),
])

page("install/desktop", "install", L("تثبيت Ultar OS على حاسوب", "Install Ultar OS on a computer", "Ultar OS auf einem Rechner installieren"),
     lead=L("لـ Desktop وServer – عبر ذاكرة USB، في ست عشرة خطوة قصيرة.", "For desktop and server – from a USB stick, in sixteen short steps.", "Für Desktop und Server – vom USB-Stick, in sechzehn kurzen Schritten."),
     blocks=[
    ("sec", {"body": [("note", L("برنامج التثبيت يمسح القرص الذي تختاره كاملاً (إلا في وضع «بجانب نظام موجود»). استخدم جهاز اختبار أو قرصاً فارغاً واحفظ بياناتك أولاً.", "The installer erases the disk you choose completely (except in the “beside an existing system” mode). Use a test device or an empty disk and back up your data first.", "Der Installer löscht die gewählte Platte vollständig (außer im Modus „neben einem vorhandenen System“). Testgerät oder leere Platte nehmen und vorher Daten sichern.")),
        ("steps", [
        {"h": L("نزّل صورة التثبيت", "Download the installer image", "Installationsabbild herunterladen"), "p": L("من صفحة [التنزيل](/download/desktop/): الملف `ultar-os-…-installer-x86_64.img.xz` مع ملف التوقيع ‎.sig.", "From the [download page](/download/desktop/): the file `ultar-os-…-installer-x86_64.img.xz` with its .sig signature file.", "Von der [Download-Seite](/download/desktop/): die Datei `ultar-os-…-installer-x86_64.img.xz` mit der Signaturdatei .sig.")},
        {"h": L("تحقّق من الملف", "Verify the file", "Datei prüfen"), "p": L("قارن البصمة وتحقق من التوقيع كما في [دليل التحقق](/install/verify/).", "Compare the fingerprint and check the signature as in the [verification guide](/install/verify/).", "Fingerabdruck vergleichen und Signatur prüfen wie in der [Prüfanleitung](/install/verify/).")},
        {"h": L("فك الضغط واكتب الصورة على ذاكرة USB", "Unpack and write the image to a USB stick", "Entpacken und auf einen USB-Stick schreiben"),
         "p": L("في Linux أو macOS بالأوامر التالية (غيّر `/dev/sdX` إلى ذاكرتك – كل ما عليها سيُمسح). في Windows: فك الضغط بـ 7-Zip ثم اكتب ملف ‎.img بأداة لكتابة الصور الخام.",
                "On Linux or macOS with the commands below (change `/dev/sdX` to your stick – everything on it is erased). On Windows: unpack with 7-Zip, then write the .img file with a raw image writer.",
                "Unter Linux oder macOS mit den Befehlen unten (`/dev/sdX` durch Ihren Stick ersetzen – alles darauf wird gelöscht). Unter Windows: mit 7-Zip entpacken, dann die .img-Datei mit einem Programm für Rohabbilder schreiben."),
         "code": "xz -d ultar-os-<version>-installer-x86_64.img.xz\nsudo dd if=ultar-os-<version>-installer-x86_64.img of=/dev/sdX bs=4M status=progress conv=fsync"},
        {"h": L("أقلع من الذاكرة", "Start from the stick", "Vom Stick starten"), "p": L("افتح قائمة الإقلاع في UEFI (غالباً F12 أو F11 أو Esc عند التشغيل) واختر ذاكرة USB. أطفئ Secure Boot في إعدادات UEFI أولاً.", "Open the UEFI boot menu (often F12, F11 or Esc at power-on) and choose the USB stick. Turn Secure Boot off in the UEFI settings first.", "Das UEFI-Startmenü öffnen (oft F12, F11 oder Esc beim Einschalten) und den USB-Stick wählen. Vorher Secure Boot in den UEFI-Einstellungen ausschalten.")},
        {"h": L("اتبع برنامج التثبيت", "Follow the installer", "Dem Installer folgen"), "p": L("ست عشرة خطوة؛ يمكنك الرجوع وتعديل أي خطوة من الملخّص قبل الكتابة:", "Sixteen steps; you can go back and edit any step from the summary before anything is written:", "Sechzehn Schritte; vor dem Schreiben lässt sich jeder Schritt aus der Zusammenfassung ändern:"),
         "olist": [L(f"**{s[0]['ar']}** – {s[1]['ar']}", f"**{s[0]['en']}** – {s[1]['en']}", f"**{s[0]['de']}** – {s[1]['de']}") for s in INSTALLER_STEPS]},
        {"h": L("الإقلاع الأول", "The first start", "Der erste Start"), "p": L("سجّل الدخول بالحساب الذي أنشأته. النظام يأخذ لغتك ولوحة المفاتيح والمنطقة الزمنية، والمتجر يعرض المكوّنات التي اخترتها ولم تُثبَّت بعد.", "Sign in with the account you created. The system takes over your language, keyboard and time zone, and the store shows the components you chose that are not installed yet.", "Mit dem angelegten Konto anmelden. Das System übernimmt Sprache, Tastatur und Zeitzone, und der Store zeigt die gewählten, noch fehlenden Komponenten.")},
    ])]}),
    ("sec", {"alt": True, "h": L("إذا لم ينجح شيء", "If something does not work", "Wenn etwas nicht klappt"), "body": [("faq", [
        (L("الحاسوب لا يقلع من الذاكرة", "The computer does not start from the stick", "Der Rechner startet nicht vom Stick"), L("تأكد أن الحاسوب في وضع UEFI (وليس Legacy/CSM) وأن Secure Boot مطفأ، وأن الصورة كُتبت كصورة خام لا كملف.", "Make sure the computer is in UEFI mode (not Legacy/CSM), Secure Boot is off, and the image was written as a raw image, not copied as a file.", "Prüfen, dass der Rechner im UEFI-Modus ist (nicht Legacy/CSM), Secure Boot aus ist und das Abbild als Rohabbild geschrieben und nicht als Datei kopiert wurde.")),
        (L("الشاشة تبقى سوداء على جهاز حقيقي", "The screen stays black on a real computer", "Der Bildschirm bleibt auf echter Hardware schwarz"), L("الإقلاع المباشر على العتاد الحقيقي ما زال قيد الاختبار. جرّب في [آلة افتراضية](/install/vm/) وأخبرنا بطراز جهازك.", "Starting directly on real hardware is still being tested. Try a [virtual machine](/install/vm/) and tell us your device model.", "Der direkte Start auf echter Hardware wird noch geprüft. Probieren Sie eine [virtuelle Maschine](/install/vm/) und nennen Sie uns Ihr Gerätemodell.")),
        (L("القرص صغير جداً", "The disk is too small", "Die Platte ist zu klein"), L("يعرض برنامج التثبيت الحد الأدنى المطلوب لكل قسم؛ اختر قرصاً أكبر أو حرّر مساحة.", "The installer shows the minimum each partition needs; choose a bigger disk or free up space.", "Der Installer zeigt das Minimum je Partition; eine größere Platte wählen oder Platz freimachen.")),
    ])]}),
])

page("install/vm", "install", L("Ultar OS في آلة افتراضية", "Ultar OS in a virtual machine", "Ultar OS in einer virtuellen Maschine"),
     lead=L("جرّب كل شيء دون أن يتغيّر شيء على حاسوبك.", "Try everything without changing anything on your computer.", "Alles ausprobieren, ohne am Rechner etwas zu ändern."),
     blocks=[
    ("sec", {"h": "QEMU", "body": [("steps", [
        {"h": L("فك ضغط الصورة وأنشئ قرصاً افتراضياً", "Unpack the image and create a virtual disk", "Abbild entpacken und eine virtuelle Platte anlegen"),
         "code": "xz -d ultar-os-<version>-installer-x86_64.img.xz\nqemu-img create -f qcow2 ultar-disk.qcow2 32G"},
        {"h": L("شغّل المثبّت بـ UEFI ‏(OVMF)", "Start the installer with UEFI (OVMF)", "Installer mit UEFI (OVMF) starten"),
         "p": L("مسار ملف OVMF يختلف حسب التوزيعة. أضف `-enable-kvm` إن توفر لسرعة أعلى.", "The path of the OVMF file differs between systems. Add `-enable-kvm` where available for more speed.", "Der Pfad der OVMF-Datei unterscheidet sich je nach System. Mit `-enable-kvm` wird es schneller, wo verfügbar."),
         "code": "qemu-system-x86_64 -machine q35 -m 4G -smp 2 \\\n  -bios /usr/share/ovmf/OVMF.fd \\\n  -drive file=ultar-os-<version>-installer-x86_64.img,format=raw,if=virtio \\\n  -drive file=ultar-disk.qcow2,if=virtio \\\n  -device qemu-xhci -device usb-kbd -device usb-tablet"},
        {"h": L("ثبّت ثم أقلع من القرص", "Install, then start from the disk", "Installieren, dann von der Platte starten"), "p": L("اختر `ultar-disk.qcow2` في خطوة «القرص». بعد الانتهاء، أزل سطر صورة التثبيت وشغّل مجدداً.", "Choose `ultar-disk.qcow2` in the “Disk” step. When it is done, remove the installer image line and start again.", "Im Schritt „Datenträger“ `ultar-disk.qcow2` wählen. Danach die Zeile mit dem Installationsabbild entfernen und neu starten.")},
    ])]}),
    ("sec", {"alt": True, "h": "VMware", "body": [("steps", [
        {"h": L("إن كان للإصدار ملف VMware", "If the release has a VMware file", "Hat die Ausgabe eine VMware-Datei"), "p": L("نزّله من [صفحة الآلة الافتراضية](/download/vm/)، فك الضغط، وافتح ملف ‎.vmx في VMware Workstation.", "Get it from the [virtual machine page](/download/vm/), unpack it and open the .vmx file in VMware Workstation.", "Von der [VM-Seite](/download/vm/) holen, entpacken und die .vmx-Datei in VMware Workstation öffnen.")},
        {"h": L("وإلا: حوّل صورة التثبيت", "Otherwise: convert the installer image", "Sonst: Installationsabbild umwandeln"), "p": L("حوّل الصورة إلى قرص VMware وأضفه إلى آلة جديدة مع قرص فارغ ثانٍ. اختر البرنامج الثابت UEFI وأطفئ Secure Boot.", "Convert the image into a VMware disk and add it to a new machine together with a second, empty disk. Choose UEFI firmware and turn Secure Boot off.", "Das Abbild in eine VMware-Platte umwandeln und zusammen mit einer zweiten, leeren Platte an eine neue Maschine hängen. UEFI-Firmware wählen und Secure Boot ausschalten."),
         "code": "qemu-img convert -O vmdk ultar-os-<version>-installer-x86_64.img ultar-installer.vmdk"},
    ]), ("note", L("نصيحة: VMware يلتقط أجهزة USB الحرة تلقائياً. افصل الكاميرا أو الذاكرة عن الآلة إن احتجتها على حاسوبك.", "Tip: VMware grabs free USB devices automatically. Disconnect a camera or stick from the machine if you need it on your computer.", "Tipp: VMware greift frei werdende USB-Geräte automatisch ab. Kamera oder Stick von der Maschine trennen, wenn Sie sie am Rechner brauchen."))]}),
])

page("install/phone", "install", L("Ultar OS على الهاتف", "Ultar OS on a phone", "Ultar OS auf dem Handy"),
     lead=L("إصدار تجريبي – لهواتف الاختبار فقط.", "Experimental – for test phones only.", "Experimentell – nur für Testhandys."),
     blocks=[
    ("sec", {"body": [("note", L("هذه الحزمة لم تُجرَّب بعد على هاتف حقيقي. قد يتوقف الهاتف عن الإقلاع. لا تستخدم هاتفاً تحتاجه يومياً.", "This package has not been started on a real phone yet. The phone may stop starting. Do not use a phone you need every day.", "Dieses Paket lief noch auf keinem echten Handy. Das Handy startet danach eventuell nicht mehr. Kein Handy verwenden, das Sie täglich brauchen.")),
        ("h3", "Samsung Galaxy A30 (SM-A305F)"),
        ("steps", [
            {"h": L("فعّل «إلغاء قفل OEM»", "Turn on “OEM unlocking”", "„OEM-Entsperrung“ einschalten"), "p": L("في خيارات المطوّر على الهاتف، ثم افتح محمّل الإقلاع.", "In the phone's developer options, then unlock the boot loader.", "In den Entwickleroptionen des Handys, dann den Bootloader entsperren.")},
            {"h": L("خذ نسخة احتياطية كاملة", "Make a full backup", "Vollsicherung machen"), "p": L("Ultar Flash يقرأ الأقسام ويحفظها قبل أي كتابة.", "Ultar Flash reads and saves the partitions before writing anything.", "Ultar Flash liest die Partitionen und sichert sie vor jedem Schreiben.")},
            {"h": L("اكتب الحزمة بـ Ultar Flash", "Write the package with Ultar Flash", "Paket mit Ultar Flash schreiben"), "p": L("الحزمة ‎`…-a30-linux.zip` تكتب قسمي system وvbmeta فقط – لا محمّل الإقلاع ولا المودم. Ultar Flash أداة Windows من KM Code Craft ولا يُقدَّم بعد للتنزيل هنا.", "The package `…-a30-linux.zip` writes only the system and vbmeta partitions – never the boot loader or the modem. Ultar Flash is a Windows tool by KM Code Craft and is not offered for download here yet.", "Das Paket `…-a30-linux.zip` schreibt nur die Partitionen system und vbmeta – nie Bootloader oder Modem. Ultar Flash ist ein Windows-Werkzeug von KM Code Craft und wird hier noch nicht zum Download angeboten.")},
        ]),
        ("h3", L("أجهزة x86_64 والمحاكي", "x86_64 devices and the emulator", "x86_64-Geräte und Emulator")),
        ("p", L("صورة ‎`…-phone-installer-x86_64.img.xz` تُثبَّت مثل [إصدار الحاسوب](/install/desktop/) أو في [آلة افتراضية](/install/vm/)، وتُظهر واجهة الهاتف.", "The image `…-phone-installer-x86_64.img.xz` installs like the [computer edition](/install/desktop/) or in a [virtual machine](/install/vm/), and shows the phone interface.", "Das Abbild `…-phone-installer-x86_64.img.xz` installiert sich wie die [Rechner-Ausgabe](/install/desktop/) oder in einer [virtuellen Maschine](/install/vm/) und zeigt die Handy-Oberfläche.")),
        ("btns", [(L("تنزيل إصدار الهاتف", "Download the phone release", "Handy-Ausgabe herunterladen"), "/download/phone/", "primary")])]}),
])

page("install/verify", "install", L("التحقق من التنزيل", "Verify a download", "Download prüfen"),
     lead=L("دقيقتان تضمنان أن الملف منّا ولم يتغيّر.", "Two minutes that make sure the file is ours and unchanged.", "Zwei Minuten, die sicherstellen, dass die Datei von uns ist und unverändert."),
     blocks=[
    ("sec", {"body": [("steps", [
        {"h": L("البصمة (SHA-256)", "Fingerprint (SHA-256)", "Fingerabdruck (SHA-256)"), "p": L("احسب البصمة وقارنها بالقيمة المعروضة في صفحة التنزيل.", "Compute the fingerprint and compare it with the value on the download page.", "Fingerabdruck berechnen und mit dem Wert auf der Download-Seite vergleichen."),
         "code": "sha256sum <file>                              # Linux\nshasum -a 256 <file>                           # macOS\nGet-FileHash <file> -Algorithm SHA256          # Windows PowerShell"},
        {"h": L("التوقيع (Ed25519)", "Signature (Ed25519)", "Signatur (Ed25519)"), "p": L("نزّل ملف ‎.sig وملف المفاتيح [ultar-allowed-signers](/downloads/ultar-allowed-signers)، ثم تحقّق بأداة ssh-keygen الموجودة في Linux وmacOS وWindows 10/11:", "Download the .sig file and the key file [ultar-allowed-signers](/downloads/ultar-allowed-signers), then check with ssh-keygen, which Linux, macOS and Windows 10/11 already have:", "Die .sig-Datei und die Schlüsseldatei [ultar-allowed-signers](/downloads/ultar-allowed-signers) herunterladen und mit ssh-keygen prüfen, das Linux, macOS und Windows 10/11 mitbringen:"),
         "code": "ssh-keygen -Y verify -f ultar-allowed-signers -I release@ultar.code-craft.tech \\\n  -n ultar-release -s <file>.sig < <file>"},
        {"h": L("النتيجة", "The result", "Das Ergebnis"), "p": L("يجب أن ترى `Good \"ultar-release\" signature`. أي نتيجة أخرى: لا تستخدم الملف واكتب إلينا.", "You must see `Good \"ultar-release\" signature`. Any other result: do not use the file and write to us.", "Sie müssen `Good \"ultar-release\" signature` sehen. Jedes andere Ergebnis: Datei nicht verwenden und uns schreiben.")},
    ])]}),
])

page("update", "install", L("تحديث Ultar OS", "Updating Ultar OS", "Ultar OS aktualisieren"),
     lead=L("تحديثات موقّعة تُثبَّت بجانب النسخة الحالية – وإن فشلت يعود الجهاز وحده.", "Signed updates installed next to the current version – if one fails, the device goes back by itself.", "Signierte Updates neben der aktuellen Fassung – schlägt eines fehl, kehrt das Gerät selbst zurück."),
     blocks=[
    ("sec", {"body": [("upd",)]}),
    ("sec", {"h": L("تحديث النظام (من 0.8.1)", "Updating the system (from 0.8.1)", "System aktualisieren (ab 0.8.1)"), "body": [("steps", [
        {"h": L("افتح الإعدادات ← تحديث النظام", "Open Settings → System update", "Einstellungen → Systemaktualisierung öffnen")},
        {"h": L("بحث ← تنزيل ← تثبيت", "Check → Download → Install", "Suchen → Herunterladen → Installieren"), "p": L("الجهاز يتحقق من التوقيع والبصمة والحجم قبل أي كتابة.", "The device checks signature, fingerprint and size before writing anything.", "Das Gerät prüft Signatur, Fingerabdruck und Größe, bevor es etwas schreibt.")},
        {"h": L("أعد التشغيل", "Restart", "Neu starten"), "p": L("من قائمة التشغيل/الإيقاف. يقلع الجهاز النسخة الجديدة على سبيل التجربة؛ إن لم تعمل يعود إلى السابقة تلقائياً.", "From the power menu. The device starts the new version on trial; if it does not come up, it goes back to the previous one by itself.", "Über das Ein/Aus-Menü. Das Gerät startet die neue Fassung zur Probe; kommt sie nicht hoch, kehrt es selbst zur vorigen zurück.")},
    ]), ("note", L("الأجهزة المثبّت عليها 0.8.0 ليس فيها برنامج التحديث: ثبّت 0.8.1 أو أحدث مرة واحدة، وبعدها تصل التحديثات عبر الشبكة.", "Devices with 0.8.0 have no updater: install 0.8.1 or newer once, after that updates arrive over the network.", "Geräte mit 0.8.0 haben keinen Updater: einmal 0.8.1 oder neuer installieren, danach kommen Updates über das Netz."))]}),
    ("sec", {"alt": True, "h": L("لماذا هو آمن", "Why it is safe", "Warum das sicher ist"), "body": [("cards", [
        {"h": L("نسختان A/B", "Two copies A/B", "Zwei Kopien A/B"), "p": L("النسخة العاملة لا تُكتب أبداً. التحديث يذهب إلى النسخة الثانية، وتبقى بياناتك في قسمها المنفصل.", "The running copy is never written. The update goes into the second copy, and your data stays in its own partition.", "Die laufende Kopie wird nie beschrieben. Das Update geht in die zweite Kopie, Ihre Daten bleiben in ihrer eigenen Partition.")},
        {"h": L("موقّع ومحمي من الرجوع", "Signed, no going back", "Signiert, kein Zurück"), "p": L("بيان موقّع بمفتاح التحديث، ورقم لا يقبل الجهاز أقل منه، وتاريخ انتهاء.", "A manifest signed with the update key, a number below which the device accepts nothing, and an expiry date.", "Ein mit dem Update-Schlüssel signiertes Manifest, eine Nummer, unter die das Gerät nichts annimmt, und ein Ablaufdatum.")},
        {"h": L("آمن عند انقطاع الكهرباء", "Safe at power loss", "Sicher bei Stromausfall"), "p": L("مختبَر عند 29 نقطة انقطاع: نسخة نصف مكتوبة لا يُقلع منها أبداً.", "Tested at 29 power-loss points: a half-written copy is never started.", "An 29 Stromausfall-Stellen geprüft: eine halb geschriebene Kopie wird nie gestartet.")},
    ], 3)]}),
    ("sec", {"h": L("التطبيقات والتعريفات", "Apps and drivers", "Apps und Treiber"), "body": [("checks", [
        L("التطبيقات يحدّثها متجر Ultar – بالتواقيع نفسها، ودون حساب.", "Apps are updated by the Ultar Store – with the same signatures, and without an account.", "Apps aktualisiert der Ultar Store – mit denselben Signaturen und ohne Konto."),
        L("التعريفات موقّعة وتأتي مع وسيط التثبيت ومع تحديثات النظام.", "Drivers are signed and come with the installation medium and with system updates.", "Treiber sind signiert und kommen mit dem Installationsmedium und den System-Updates."),
        L("التحديث التلقائي الليلي يأتي لاحقاً؛ اليوم تبدأ التحديث أنت من الإعدادات.", "Automatic updates at night come later; today you start the update yourself in Settings.", "Automatische Updates in der Nacht kommen später; heute starten Sie das Update selbst in den Einstellungen."),
    ], "dot")]}),
])

# ---- help, news, company
page("support", "support", L("المساعدة", "Help", "Hilfe"),
     lead=L("أجوبة سريعة، وطريقة الوصول إلينا.", "Quick answers, and how to reach us.", "Schnelle Antworten und wie Sie uns erreichen."),
     blocks=[
    ("sec", {"h": L("أسئلة شائعة", "Questions and answers", "Häufige Fragen"), "body": [("faq", [
        (L("هل أحتاج إلى حساب؟", "Do I need an account?", "Brauche ich ein Konto?"), L("لا. النظام والمتجر والتحديثات تعمل دون حساب. الحساب للمكالمات عبر رقم Ultar والمجموعة عبر الإنترنت. [المزيد](/account/).", "No. The system, the store and updates work without an account. The account is for calls over the Ultar number and the mesh over the internet. [More](/account/).", "Nein. System, Store und Updates gehen ohne Konto. Das Konto ist für Anrufe über die Ultar-Nummer und den Verbund über das Internet. [Mehr](/account/).")),
        (L("هل يعمل على حاسوبي؟", "Does it run on my computer?", "Läuft es auf meinem Rechner?"), L("يحتاج إلى حاسوب x86_64 بنظام UEFI. الإقلاع المباشر على العتاد الحقيقي ما زال قيد الاختبار؛ الطريقة المضمونة اليوم هي [آلة افتراضية](/install/vm/). انظر [العتاد المختبَر](/hardware/).", "It needs an x86_64 computer with UEFI. Starting directly on real hardware is still being tested; the sure way today is a [virtual machine](/install/vm/). See [tested hardware](/hardware/).", "Es braucht einen x86_64-Rechner mit UEFI. Der direkte Start auf echter Hardware wird noch geprüft; sicher geht es heute in einer [virtuellen Maschine](/install/vm/). Siehe [geprüfte Hardware](/hardware/).")),
        (L("هل أستطيع تثبيته بجانب نظام آخر؟", "Can I install it beside another system?", "Kann ich es neben einem anderen System installieren?"), L("نعم: خيار «بجانب نظام موجود» يكتب في أكبر مساحة حرة فقط، ولا يصغّر أو ينقل شيئاً. لكنه إصدار تجريبي – احفظ بياناتك أولاً.", "Yes: the option “beside an existing system” writes only into the largest free region and shrinks or moves nothing. It is a beta, though – back up your data first.", "Ja: die Option „neben einem vorhandenen System“ schreibt nur in den größten freien Bereich und verkleinert oder verschiebt nichts. Es ist aber eine Beta – vorher Daten sichern.")),
        (L("لماذا يجب إطفاء Secure Boot؟", "Why must Secure Boot be off?", "Warum muss Secure Boot aus sein?"), L("لأن محمّل الإقلاع الخاص بنا غير موقّع بعد لـ Secure Boot. كل ما بعده يُتحقق من توقيعه بمفاتيحنا.", "Because our own boot loader is not signed for Secure Boot yet. Everything after it is checked against our keys.", "Weil unser eigener Lader noch nicht für Secure Boot signiert ist. Alles danach wird gegen unsere Schlüssel geprüft.")),
        (L("هل أفقد بياناتي عند التحديث؟", "Do I lose data in an update?", "Verliere ich bei einem Update Daten?"), L("لا: التحديث يكتب نسخة النظام الثانية فقط، والبيانات في قسم منفصل. [كيف يعمل التحديث](/update/).", "No: an update writes only the second system copy; your data is in a separate partition. [How updates work](/update/).", "Nein: ein Update schreibt nur die zweite Systemkopie; die Daten liegen in einer eigenen Partition. [So funktionieren Updates](/update/).")),
        (L("ما اللغات المتوفرة؟", "Which languages are there?", "Welche Sprachen gibt es?"), L("العربية والإنجليزية والألمانية – العربية من اليمين لليسار في كل مكان، والأرقام دائماً 0–9.", "Arabic, English and German – Arabic right-to-left everywhere, digits always 0–9.", "Arabisch, Englisch und Deutsch – Arabisch von rechts nach links überall, Ziffern immer 0–9.")),
        (L("هل يمكنني تشغيل برامجي الحالية؟", "Can I run my existing programs?", "Kann ich meine bisherigen Programme nutzen?"), L("هذا هدفنا: برامج من أنظمة أخرى تعمل في منطقة معزولة خاصة بها. هذه المكوّنات قيد التطوير.", "That is our goal: programs from other systems run in an isolated area of their own. These components are in development.", "Das ist unser Ziel: Programme anderer Systeme laufen in einem eigenen abgeschotteten Bereich. Diese Komponenten sind in Arbeit.")),
    ])]}),
    ("sec", {"alt": True, "id": "contact", "h": L("الإبلاغ عن خطأ أو طرح سؤال", "Report a problem or ask a question", "Fehler melden oder Frage stellen"),
             "lead": L("اكتب إلى [info@code-craft.tech](mailto:info@code-craft.tech) واذكر:", "Write to [info@code-craft.tech](mailto:info@code-craft.tech) and mention:", "Schreiben Sie an [info@code-craft.tech](mailto:info@code-craft.tech) und nennen Sie:"),
             "body": [("checks", [
                 L("رقم الإصدار (الإعدادات ← حول)", "The version (Settings → About)", "Die Version (Einstellungen → Info)"),
                 L("الجهاز أو الآلة الافتراضية", "The device or virtual machine", "Das Gerät oder die virtuelle Maschine"),
                 L("ما فعلته، وما توقعته، وما حدث – مع صورة شاشة إن أمكن", "What you did, what you expected and what happened – with a screenshot if possible", "Was Sie getan haben, was Sie erwartet haben und was passiert ist – wenn möglich mit Bildschirmfoto"),
             ], "dot"), ("p", L("للثغرات الأمنية: [الإبلاغ عن ثغرة](/security/#report).", "For security issues: [report a security issue](/security/#report).", "Für Sicherheitslücken: [Sicherheitslücke melden](/security/#report)."))]}),
])

page("news", "news", L("الأخبار", "News", "Neuigkeiten"),
     lead=L("ما الجديد في Ultar OS.", "What is new in Ultar OS.", "Was es Neues in Ultar OS gibt."),
     blocks=[("sec", {"body": [("news",)]})])

page("about", "about", L("من نحن", "About", "Über uns"),
     lead=L("Ultar OS من KM Code Craft – نظام نبنيه من الأساس، بالعربية أولاً.", "Ultar OS by KM Code Craft – a system we build from the ground up, Arabic first.", "Ultar OS von KM Code Craft – ein System, das wir von Grund auf bauen, Arabisch zuerst."),
     blocks=[
    ("sec", {"h": L("وعدنا لك", "Our promise to you", "Unser Versprechen an Sie"),
             "lead": L("هذه أهدافنا التي نعمل عليها كل يوم. ما يعمل منها اليوم تجده في [الحالة](/releases/).", "These are the goals we work on every day. What already works today is listed under [status](/releases/).", "Das sind die Ziele, an denen wir jeden Tag arbeiten. Was heute schon funktioniert, steht unter [Stand](/releases/)."),
             "body": [("cards", PROMISE, 4)]}),
    ("sec", {"alt": True, "h": L("كيف نعمل", "How we work", "Wie wir arbeiten"), "body": [("checks", [
        L("**العربية أولاً:** كل نص يُكتب بالعربية أولاً – صيغ الجمع الست، التقويم الهجري، وبحث يتجاهل التشكيل.", "**Arabic first:** every text is written in Arabic first – the six plural forms, the Hijri calendar, and search that ignores diacritics.", "**Arabisch zuerst:** jeder Text entsteht zuerst auf Arabisch – sechs Mehrzahlformen, Hidschri-Kalender und eine Suche, die Vokalzeichen übergeht."),
        L("**الصدق:** نكتب ما يعمل وما لا يعمل بعد، بلا تجميل.", "**Honesty:** we write what works and what does not yet, without polish.", "**Ehrlichkeit:** wir schreiben, was funktioniert und was noch nicht, ohne Schönfärberei."),
        L("**الاستقلال:** النواة والنظام والخدمات من صنعنا، وكل جزء قابل للفحص.", "**Independence:** kernel, system and services are our own, and every part can be checked.", "**Unabhängigkeit:** Kern, System und Dienste sind unsere eigenen, jedes Teil ist prüfbar."),
        L("**الاختبار:** آلاف الاختبارات الآلية، وكل نقرة يجب أن تعمل.", "**Testing:** thousands of automatic tests, and every click has to work.", "**Prüfen:** tausende automatische Tests, und jeder Klick muss funktionieren."),
    ])]}),
    ("sec", {"h": "KM Code Craft", "body": [("kv", [
        (L("التطوير", "Development", "Entwicklung"), "محمد أنس قرة محمد · Mohamad Anas Kara Mohamad"),
        (L("الشركة", "Company", "Unternehmen"), L("KM Code Craft، إدلب", "KM Code Craft, Idlib", "KM Code Craft, Idlib")),
        (L("الموقع", "Website", "Webseite"), "[code-craft.tech](https://code-craft.tech)"),
        (L("التواصل", "Contact", "Kontakt"), "[info@code-craft.tech](mailto:info@code-craft.tech)"),
    ])]}),
])

page("privacy", "about", L("خصوصية هذا الموقع", "Privacy of this site", "Datenschutz dieser Seite"),
     lead=L("هذا الموقع لا يتتبّعك.", "This site does not track you.", "Diese Seite verfolgt Sie nicht."),
     blocks=[
    ("sec", {"body": [("checks", [
        L("لا ملفات تعريف ارتباط، لا تتبّع، لا إحصاءات، ولا خطوط أو نصوص من جهات خارجية.", "No cookies, no tracking, no analytics, and no third-party fonts or scripts.", "Keine Cookies, kein Tracking, keine Statistik und keine fremden Schriften oder Skripte."),
        L("لا نحفظ عناوين IP للزوار.", "We do not store visitors' IP addresses.", "Wir speichern keine IP-Adressen der Besucher."),
        L("الصفحة تطلب من خادمنا فقط قائمة الإصدارات ورقم أحدث تحديث، دون أي معلومات عنك.", "The page asks our own server only for the list of releases and the newest update version, without any information about you.", "Die Seite fragt unseren eigenen Server nur nach der Liste der Ausgaben und der neuesten Update-Version, ohne Angaben über Sie."),
        L("اختيار اللغة لا يُحفظ؛ هو جزء من عنوان الصفحة فقط.", "Your language choice is not stored; it is only part of the page address.", "Die Sprachwahl wird nicht gespeichert; sie steckt nur in der Seitenadresse."),
    ])]}),
])
