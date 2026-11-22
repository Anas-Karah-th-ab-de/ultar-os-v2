//! Hilfe-Wissensbasis von Ultar OS.
//!
//! Eine einzige Quelle für zwei Zwecke:
//!   1. Die Hilfe in der Oberfläche (Hilfe-Zentrum + „?" in jeder Kopfzeile).
//!   2. Die Basis für den lokalen Agenten: der Systemprompt bekommt einen kurzen
//!      Katalog, das Werkzeug `help` liefert den vollen Artikel, und beim Start wird
//!      alles als Markdown nach /data/ultar/help geschrieben (für den Agenten und
//!      externe Werkzeuge lesbar).
//!
//! Jeder Eintrag erklärt einen Teil des Systems in drei Sprachen: Englisch, Deutsch,
//! Arabisch. `title` ist ein englischer Schlüssel, der über i18n übersetzt wird.

/// Ein Hilfethema zu einem Bildschirm oder einer Funktion.
pub struct Topic {
    /// Kennung, passend zum Bildschirm (siehe `App::cur_help`).
    pub id: &'static str,
    /// Titelschlüssel (englisch), via i18n übersetzt.
    pub title: &'static str,
    /// Hilfetext in [en, de, ar].
    pub body: [&'static str; 3],
    /// true = systemnahe Funktion (für die Gruppierung im Hilfe-Zentrum).
    pub system: bool,
}

const fn t(id: &'static str, title: &'static str, en: &'static str, de: &'static str, ar: &'static str) -> Topic {
    Topic { id, title, body: [en, de, ar], system: false }
}
const fn s(id: &'static str, title: &'static str, en: &'static str, de: &'static str, ar: &'static str) -> Topic {
    Topic { id, title, body: [en, de, ar], system: true }
}

pub const TOPICS: &[Topic] = &[
    // ---- Grundlagen -----------------------------------------------------
    t("home", "Home",
      "The home screen shows the time, your connection and the apps. Swipe down from the top for quick settings and notifications. Tap the search bar to find apps, settings, contacts and notes. Hold an app on the recent-apps list to pin it on top.",
      "Der Startbildschirm zeigt Uhrzeit, Verbindung und Apps. Vom oberen Rand nach unten ziehen öffnet Schnelleinstellungen und Benachrichtigungen. Über die Suchleiste findest du Apps, Einstellungen, Kontakte und Notizen. Eine App in den zuletzt verwendeten halten, um sie oben anzuheften.",
      "الشاشة الرئيسية تعرض الوقت والاتصال والتطبيقات. اسحب من الأعلى إلى الأسفل لفتح الإعدادات السريعة والإشعارات. انقر شريط البحث للعثور على التطبيقات والإعدادات وجهات الاتصال والملاحظات. اضغط مطوّلاً على تطبيق في قائمة التطبيقات الأخيرة لتثبيته في الأعلى."),
    t("apps", "All apps",
      "This is the full list of apps. Tap an app to open it. The most used apps also sit on the home screen. Use the search bar on the home screen to jump to any app quickly.",
      "Hier stehen alle Apps. Tippe eine App an, um sie zu öffnen. Die meistgenutzten liegen zusätzlich auf dem Startbildschirm. Mit der Suchleiste auf dem Startbildschirm springst du schnell zu jeder App.",
      "هنا كل التطبيقات. انقر تطبيقاً لفتحه. التطبيقات الأكثر استخداماً موجودة أيضاً على الشاشة الرئيسية. استخدم شريط البحث في الشاشة الرئيسية للانتقال بسرعة إلى أي تطبيق."),
    s("settings", "Settings",
      "Settings are grouped by area: connections, device, personal, security and system. Use the search field at the top to find any setting by name. Each line shows its current value on the right.",
      "Die Einstellungen sind nach Bereichen gruppiert: Verbindungen, Gerät, Persönlich, Sicherheit und System. Über das Suchfeld oben findest du jede Einstellung nach Namen. Rechts steht jeweils der aktuelle Wert.",
      "الإعدادات مُرتّبة حسب المجال: الاتصالات والجهاز والشخصي والأمان والنظام. استخدم حقل البحث في الأعلى للعثور على أي إعداد بالاسم. تظهر القيمة الحالية لكل بند على اليمين."),
    t("agent", "Assistant",
      "KM is your system assistant – like Jarvis. It runs on the device itself (Ultar core, no cloud) and acts only when you ask. It sees what you see: ask \"what is on my screen?\" or tap + to bring it the current screen, a photo, an image or a file (also from Files: hold a file → Give to assistant). On request it looks at you through the front camera. It has root access to the whole system – processes, files, network connections, logs, privacy switches, commands – but root commands, security changes, the camera, writing or deleting files, packages and power always ask for your approval first. Models come from Hugging Face or Ollama; Settings → Local agent recommends the right one for this device (a chat model and, if there is enough memory, a vision model). Type in Arabic, English or German.",
      "KM ist dein System-Assistent – wie Jarvis. Er läuft auf dem Gerät selbst (Ultar-Kern, keine Cloud) und handelt nur, wenn du ihn bittest. Er sieht, was du siehst: frag „Was ist auf meinem Bildschirm?\" oder tippe auf +, um ihm den aktuellen Bildschirm, ein Foto, ein Bild oder eine Datei zu bringen (auch aus Dateien: Datei halten → Dem Assistenten geben). Auf Wunsch sieht er dich über die Frontkamera. Er hat Root-Zugriff auf das ganze System – Prozesse, Dateien, Netzverbindungen, Protokolle, Datenschutz-Schalter, Befehle –, aber Root-Befehle, Sicherheitsänderungen, die Kamera, Schreiben oder Löschen von Dateien, Pakete und Ausschalten fragen immer vorher nach. Modelle kommen von Hugging Face oder Ollama; Einstellungen → Lokaler Agent empfiehlt das passende für dieses Gerät (ein Gesprächsmodell und, wenn genug Speicher da ist, ein Bild-Modell). Schreib auf Arabisch, Englisch oder Deutsch.",
      "KM هو مساعد النظام لديك – مثل جارفيس. يعمل على الجهاز نفسه (نواة Ultar، بدون سحابة) ولا يتصرف إلا عندما تطلب. يرى ما تراه: اسأل «ما الذي على شاشتي؟» أو انقر + لتُحضر له الشاشة الحالية أو صورة أو ملفاً (من تطبيق الملفات أيضاً: اضغط مطوّلاً على ملف ← أعطِ المساعد). عند الطلب ينظر إليك عبر الكاميرا الأمامية. لديه صلاحية الجذر على النظام كله – العمليات والملفات واتصالات الشبكة والسجلات ومفاتيح الخصوصية والأوامر – لكن أوامر الجذر والتغييرات الأمنية والكاميرا وكتابة الملفات أو حذفها والحزم والإطفاء تطلب موافقتك دائماً أولاً. تأتي النماذج من Hugging Face أو Ollama؛ الإعدادات ← الوكيل المحلي يوصي بالنموذج المناسب لهذا الجهاز (نموذج محادثة، ونموذج رؤية إذا توفرت ذاكرة كافية). اكتب بالعربية أو الإنجليزية أو الألمانية."),

    // ---- Verbindung -----------------------------------------------------
    t("radio", "Mobile network",
      "The mobile centre shows both SIM cards, the live signal and the data rate. You pick which SIM is for calls and which for data. Tap a SIM to enter its PIN or see its number and operator. If there is no service, check that the SIM is inserted and mobile data is on.",
      "Das Mobilfunk-Zentrum zeigt beide SIM-Karten, das Live-Signal und die Datenrate. Du wählst, welche SIM für Anrufe und welche für Daten dient. Tippe eine SIM an, um die PIN einzugeben oder Nummer und Anbieter zu sehen. Kein Netz? Prüfe, ob die SIM steckt und mobile Daten an sind.",
      "مركز شبكة الجوّال يعرض بطاقتي SIM والإشارة الحيّة ومعدّل البيانات. تختار أيّ شريحة للمكالمات وأيّها للبيانات. انقر شريحة لإدخال رمز PIN أو لرؤية رقمها ومشغّلها. لا توجد خدمة؟ تأكّد من إدخال الشريحة وتشغيل بيانات الجوّال."),
    t("cells", "Towers",
      "Here you choose how the phone picks a tower (cell). Automatic lets the network decide; Smart prefers the strongest or fastest tower; Manual pins one tower you choose. Use Smart when the signal keeps dropping, and Manual only if you know the right tower.",
      "Hier wählst du, wie das Telefon einen Funkturm (Zelle) auswählt. Automatisch überlässt es dem Netz; Smart bevorzugt den stärksten oder schnellsten Turm; Manuell fixiert einen von dir gewählten Turm. Nutze Smart bei ständig abbrechendem Signal, Manuell nur, wenn du den richtigen Turm kennst.",
      "هنا تختار كيف يلتقط الهاتف برج الإرسال (الخليّة). «تلقائي» يترك القرار للشبكة؛ «ذكي» يفضّل البرج الأقوى أو الأسرع؛ «يدوي» يثبّت برجاً تختاره. استخدم «ذكي» عند تكرار انقطاع الإشارة، و«يدوي» فقط إذا كنت تعرف البرج المناسب."),
    t("route", "Connection path",
      "This screen draws the whole path your data takes: device → tower → operator → VPN → internet. It helps you see where a connection stops. A broken link is marked so you know whether the problem is the SIM, the operator or the VPN.",
      "Dieser Bildschirm zeichnet den ganzen Weg deiner Daten: Gerät → Funkturm → Anbieter → VPN → Internet. So siehst du, wo eine Verbindung endet. Ein unterbrochenes Glied wird markiert, damit du erkennst, ob es an SIM, Anbieter oder VPN liegt.",
      "تعرض هذه الشاشة مسار بياناتك كاملاً: الجهاز ← البرج ← المشغّل ← VPN ← الإنترنت. تساعدك على رؤية أين يتوقّف الاتصال. تُعلَّم الحلقة المقطوعة لتعرف إن كان الخلل في الشريحة أو المشغّل أو VPN."),
    t("net", "Network",
      "Here you connect to Wi-Fi and wired networks. Tap a network and enter its password; a lock icon means it needs one. Hold a saved network to forget it. You can also add a hidden network by name and set a fixed IP under the wired network.",
      "Hier verbindest du dich mit WLAN und Kabelnetzen. Tippe ein Netz an und gib das Kennwort ein; ein Schloss bedeutet, dass eines nötig ist. Ein gespeichertes Netz halten, um es zu vergessen. Du kannst auch ein verstecktes Netz über den Namen hinzufügen und beim Kabelnetz eine feste IP einstellen.",
      "هنا تتّصل بشبكات Wi‑Fi والشبكات السلكية. انقر شبكة وأدخل كلمة المرور؛ رمز القفل يعني أنها مطلوبة. اضغط مطوّلاً على شبكة محفوظة لنسيانها. يمكنك أيضاً إضافة شبكة مخفيّة بالاسم وتعيين عنوان IP ثابت للشبكة السلكية."),
    t("bluetooth", "Bluetooth",
      "Turn Bluetooth on to connect headsets, speakers and keyboards. Tap Scan to find nearby devices, then tap one to pair. Paired devices reconnect by themselves. Hold a device to remove it.",
      "Schalte Bluetooth ein, um Kopfhörer, Lautsprecher und Tastaturen zu verbinden. Tippe auf Suchen, um Geräte in der Nähe zu finden, dann eines antippen zum Koppeln. Gekoppelte Geräte verbinden sich von selbst wieder. Ein Gerät halten, um es zu entfernen.",
      "شغّل البلوتوث لتوصيل السمّاعات ومكبّرات الصوت ولوحات المفاتيح. انقر «بحث» للعثور على الأجهزة القريبة، ثم انقر جهازاً للاقتران. الأجهزة المقترنة تعيد الاتصال تلقائياً. اضغط مطوّلاً على جهاز لإزالته."),
    t("vpn", "VPN",
      "A VPN sends your traffic through an encrypted tunnel, useful on public Wi-Fi. Add a profile, then switch it on; the status shows whether the tunnel is up. Turn it off when you do not need it to save battery and speed.",
      "Ein VPN leitet deinen Verkehr durch einen verschlüsselten Tunnel – nützlich in öffentlichen WLANs. Füge ein Profil hinzu und schalte es ein; der Status zeigt, ob der Tunnel steht. Schalte es aus, wenn du es nicht brauchst, für Akku und Tempo.",
      "تُرسِل شبكة VPN بياناتك عبر نفق مشفّر، وهي مفيدة على شبكات Wi‑Fi العامة. أضِف ملفاً ثم شغّله؛ تُظهر الحالة إن كان النفق قائماً. أطفئه عند عدم الحاجة لتوفير البطارية والسرعة."),

    // ---- Kommunikation --------------------------------------------------
    t("phone", "Phone",
      "Open the keypad to dial, or tap a recent call to call back. During a call you can mute, turn on the speaker and open the keypad for menus. An incoming call also shows up in the quick bar. Blocked numbers never ring.",
      "Öffne das Tastenfeld zum Wählen oder tippe einen letzten Anruf an, um zurückzurufen. Im Gespräch kannst du stummschalten, den Lautsprecher einschalten und das Tastenfeld für Menüs öffnen. Ein eingehender Anruf erscheint auch in der Schnellleiste. Gesperrte Nummern klingeln nie.",
      "افتح لوحة الأرقام للاتصال، أو انقر مكالمة حديثة لمعاودة الاتصال. أثناء المكالمة يمكنك الكتم وتشغيل مكبّر الصوت وفتح لوحة الأرقام للقوائم. المكالمة الواردة تظهر أيضاً في الشريط السريع. الأرقام المحظورة لا ترنّ أبداً."),
    t("messages", "Messages",
      "Read and write text messages (SMS) here. Tap a conversation to open it, or start a new one with the plus button. Type the number or pick a contact, write your text and send. Long messages are split automatically.",
      "Hier liest und schreibst du Kurznachrichten (SMS). Tippe einen Verlauf an, um ihn zu öffnen, oder starte mit dem Plus einen neuen. Nummer eingeben oder Kontakt wählen, Text schreiben und senden. Lange Nachrichten werden automatisch geteilt.",
      "هنا تقرأ وتكتب الرسائل النصّية (SMS). انقر محادثة لفتحها، أو ابدأ واحدة جديدة بزرّ الجمع. أدخل الرقم أو اختر جهة اتصال، اكتب نصّك وأرسِل. الرسائل الطويلة تُقسَّم تلقائياً."),
    t("contacts", "Contacts",
      "Keep names and numbers here. Add a contact with the plus button; tap one to call, message or edit. Contacts are stored on the device. The assistant and the phone app both use this list.",
      "Hier bewahrst du Namen und Nummern auf. Füge mit dem Plus einen Kontakt hinzu; tippe einen an, um anzurufen, zu schreiben oder zu bearbeiten. Kontakte liegen auf dem Gerät. Assistent und Telefon-App nutzen beide diese Liste.",
      "احتفظ بالأسماء والأرقام هنا. أضِف جهة اتصال بزرّ الجمع؛ انقر واحدة للاتصال أو المراسلة أو التعديل. جهات الاتصال محفوظة على الجهاز. يستخدم المساعد وتطبيق الهاتف هذه القائمة معاً."),
    t("scanner", "Document scanner",
      "Open the camera and choose Scan (or tell the helper: scan document). Lay the sheet in the picture – its edges are found and marked. Press the shutter, then drag the four corners if needed and confirm. Choose colour, grey or black-and-white. Each page is saved as a picture in DCIM/Camera; the PDF button joins all pages of this session into one PDF in Documents. The flashlight helps in dim light.",
      "Kamera öffnen und „Scan“ wählen (oder dem Helfer sagen: „Dokument scannen“). Das Blatt ins Bild legen – die Kanten werden gefunden und markiert. Auslösen, danach bei Bedarf die vier Ecken zurechtziehen und bestätigen. Farbe, Grau oder Schwarz-Weiß wählen. Jede Seite wird als Bild in DCIM/Camera gespeichert; der PDF-Knopf fasst alle Seiten dieser Sitzung zu einem PDF unter Dokumente zusammen. Bei wenig Licht hilft die Taschenlampe.",
      "افتح الكاميرا واختر «مسح» (أو قل للمساعد: امسح مستندًا). ضع الورقة داخل الصورة – تُكتشف حوافها وتُعلَّم. اضغط زر الالتقاط ثم اسحب الزوايا الأربع عند الحاجة وأكّد. اختر ملوّن أو رمادي أو أبيض وأسود. تُحفظ كل صفحة صورةً في DCIM/Camera، وزر PDF يجمع صفحات هذه الجلسة في ملف PDF واحد داخل المستندات. الكشاف يساعد في الإضاءة الضعيفة."),
    t("helper", "Helper",
      "The helper is the small figure beside you on every page. Tap it to open its field, drag it anywhere, send it away and call it back with the tab at the edge. It carries out commands at once and without internet: flashlight, Wi-Fi, Bluetooth, brightness, volume, open apps, scan, timer, alarm, notes. It reminds you: say \"remind me in 10 minutes to …\" or \"remind me at 17:30 of …\" – when it is time it comes to the middle and calls you, also with the screen off; answer Done or In 10 min. It watches the links on each page, warns about suspicious ones and tests a link in a sealed box. Ask it how something works and it answers from these help pages. Say \"tour\" for the introduction.",
      "Der Helfer ist die kleine Figur, die auf jeder Seite neben dir ist. Antippen öffnet sein Feld, ziehen setzt ihn um, wegschicken geht auch – die Lasche am Rand ruft ihn zurück. Er führt Anweisungen sofort und ohne Internet aus: Taschenlampe, WLAN, Bluetooth, Helligkeit, Lautstärke, Apps öffnen, Scannen, Timer, Wecker, Notizen. Er erinnert dich: sag „Erinnere mich in 10 Minuten an …“ oder „Erinnere mich um 17:30 an …“ – wenn es so weit ist, kommt er in die Mitte und ruft dich, auch bei ausgeschaltetem Bildschirm; antworte mit „Fertig“ oder „In 10 Min“. Er achtet auf die Links jeder Seite, warnt vor auffälligen und prüft einen Link in einer abgeschotteten Box. Frag ihn, wie etwas funktioniert, und er antwortet aus diesen Hilfeseiten. „Rundgang“ zeigt die Vorstellung noch einmal.",
      "المساعد هو الشكل الصغير الذي يرافقك في كل صفحة. المسه ليفتح حقله، اسحبه إلى أي مكان، وأبعده إن شئت – اللسان عند الحافة يعيده. ينفّذ الأوامر فورًا ودون إنترنت: الكشاف، الواي فاي، البلوتوث، السطوع، الصوت، فتح التطبيقات، المسح، المؤقت، المنبه، الملاحظات. يذكّرك: قل «ذكرني بعد 10 دقائق بـ…» أو «ذكرني الساعة 17:30 بـ…» – وعند الموعد يأتي إلى وسط الشاشة وينبّهك حتى والشاشة مطفأة؛ أجب بـ«تم» أو «بعد ١٠ دقائق». يراقب روابط كل صفحة ويحذّر من المريب منها ويفحص الرابط في صندوق معزول. اسأله كيف يعمل شيء ما فيجيب من صفحات المساعدة هذه. قل «جولة» لإعادة التعريف."),
    t("camera", "Camera",
      "Tap the round button to take a photo. Switch between the back and front camera with the flip button, and turn the flash on for dark scenes. Photos and videos are saved to the gallery. The grid lines help you keep the shot level.",
      "Tippe den runden Knopf für ein Foto. Mit dem Wechselknopf schaltest du zwischen Rück- und Frontkamera, und für dunkle Szenen schaltest du den Blitz ein. Fotos und Videos landen in der Galerie. Die Rasterlinien helfen, gerade zu halten.",
      "انقر الزرّ الدائري لالتقاط صورة. بدّل بين الكاميرا الخلفية والأمامية بزرّ القلب، وشغّل الفلاش للمَشاهد المظلمة. تُحفَظ الصور والفيديوهات في المعرض. خطوط الشبكة تساعدك على إبقاء اللقطة مستوية."),
    t("gallery", "Gallery",
      "All your photos and videos live here, newest first. Tap one to see it full screen, then swipe to move between them. From the full view you can share or delete. Videos play when you tap them.",
      "Hier liegen alle Fotos und Videos, neueste zuerst. Tippe eines an für Vollbild, dann wische, um zu blättern. Aus der Vollansicht kannst du teilen oder löschen. Videos starten beim Antippen.",
      "كل صورك وفيديوهاتك هنا، الأحدث أولاً. انقر واحدة لعرضها بملء الشاشة، ثم اسحب للتنقّل بينها. من العرض الكامل يمكنك المشاركة أو الحذف. تُشغَّل الفيديوهات عند النقر عليها."),
    t("files", "Files",
      "Browse the files on the device. Tap a folder to open it and a file to view it; images and text open directly. Hold a file to rename, move or delete it. Use the sort button to order by name, date or size.",
      "Durchsuche die Dateien auf dem Gerät. Tippe einen Ordner zum Öffnen und eine Datei zum Ansehen; Bilder und Text öffnen direkt. Eine Datei halten, um sie umzubenennen, zu verschieben oder zu löschen. Mit dem Sortierknopf ordnest du nach Name, Datum oder Größe.",
      "تصفّح ملفّات الجهاز. انقر مجلداً لفتحه وملفّاً لعرضه؛ الصور والنصوص تُفتَح مباشرة. اضغط مطوّلاً على ملف لإعادة تسميته أو نقله أو حذفه. استخدم زرّ الترتيب للفرز حسب الاسم أو التاريخ أو الحجم."),
    t("notes", "Notes",
      "Write quick notes here. Tap the plus to start one, type, and it saves on its own. Tap a note to edit it, hold to delete. Notes stay on the device.",
      "Hier schreibst du schnelle Notizen. Tippe das Plus für eine neue, schreib los, sie speichert von selbst. Eine Notiz antippen zum Bearbeiten, halten zum Löschen. Notizen bleiben auf dem Gerät.",
      "اكتب ملاحظات سريعة هنا. انقر الجمع لبدء واحدة، اكتب وتُحفَظ تلقائياً. انقر ملاحظة لتعديلها، واضغط مطوّلاً للحذف. الملاحظات تبقى على الجهاز."),
    t("office", "Office apps",
      "Ultar has its own office apps, built into the system without Linux or Windows programs: PDF reader (read, zoom, search, text view), Documents (open and write Word .docx files, Markdown and text: tap a paragraph to edit it, choose title, heading, list or quote above), Sheets (Excel .xlsx and .csv with formulas such as SUM, AVERAGE, IF, VLOOKUP: tap a cell, type in the bar above, Enter goes down; tap a cell while writing a formula to insert its reference) and the Code editor (any text or code file with colours, line numbers, find and replace with regular expressions, undo, tabs, encodings such as UTF-8 and Windows-1256). Changes are saved when you leave. The assistant can read, create and edit these files for you: ask it, or tap the sparkle button inside the app.",
      "Ultar hat eigene Büro-Apps, direkt im System ohne Linux- oder Windows-Programme: PDF-Leser (lesen, zoomen, suchen, Textansicht), Dokumente (Word-Dateien .docx, Markdown und Text öffnen und schreiben: einen Absatz antippen zum Bearbeiten, oben Titel, Überschrift, Liste oder Zitat wählen), Tabellen (Excel .xlsx und .csv mit Formeln wie SUMME (SUM), MITTELWERT (AVERAGE), WENN (IF), SVERWEIS (VLOOKUP): Zelle antippen, oben in die Zeile schreiben, Enter geht eine Zelle nach unten; beim Schreiben einer Formel eine Zelle antippen fügt ihren Bezug ein) und der Code-Editor (jede Text- oder Code-Datei mit Farben, Zeilennummern, Suchen und Ersetzen auch mit regulären Ausdrücken, Rückgängig, Reitern, Kodierungen wie UTF-8 und Windows-1256). Änderungen werden beim Verlassen gespeichert. Der Assistent kann diese Dateien für dich lesen, anlegen und bearbeiten: frag ihn oder tippe in der App auf den Funken.",
      "يحتوي Ultar على تطبيقات مكتبية خاصة به مدمجة في النظام دون برامج لينكس أو ويندوز: قارئ PDF (قراءة وتكبير وبحث وعرض النص)، المستندات (فتح ملفات Word ‏.docx وMarkdown والنصوص وكتابتها: انقر فقرة لتحريرها واختر في الأعلى العنوان أو العنوان الفرعي أو القائمة أو الاقتباس)، الجداول (ملفات Excel ‏.xlsx و‎.csv مع صيغ مثل SUM وAVERAGE وIF وVLOOKUP: انقر خلية واكتب في الشريط العلوي، Enter ينتقل للأسفل؛ أثناء كتابة صيغة انقر خلية لإدراج مرجعها) ومحرر الأكواد (أي ملف نصي أو برمجي مع الألوان وأرقام الأسطر والبحث والاستبدال بالتعابير النمطية والتراجع والتبويبات والترميزات مثل UTF-8 وWindows-1256). تُحفظ التغييرات عند المغادرة. يستطيع المساعد قراءة هذه الملفات وإنشاءها وتعديلها لك: اسأله أو انقر زر الشرارة داخل التطبيق."),
    t("clock", "Clock",
      "Set alarms, run a timer and use the stopwatch here. Add an alarm with a time; it rings even when the screen is off. The timer counts down to zero and alerts you. Turn an alarm off with its switch without deleting it.",
      "Hier stellst du Wecker, einen Timer und die Stoppuhr. Füge einen Wecker mit Uhrzeit hinzu; er klingelt auch bei ausgeschaltetem Bildschirm. Der Timer zählt auf null herunter und meldet sich. Einen Wecker mit dem Schalter ausschalten, ohne ihn zu löschen.",
      "اضبط المنبّهات وشغّل مؤقّتاً واستخدم ساعة الإيقاف هنا. أضِف منبّهاً بوقت؛ يرنّ حتى لو كانت الشاشة مطفأة. المؤقّت يعدّ تنازلياً إلى الصفر وينبّهك. أطفئ منبّهاً بمفتاحه دون حذفه."),
    t("calc", "Calculator",
      "A simple calculator for everyday sums. Tap the digits and operators; the result shows as you go. Use the clear key to start over. For loans, percentages or unit bases, see the Tools app.",
      "Ein einfacher Rechner für den Alltag. Tippe Ziffern und Rechenzeichen; das Ergebnis erscheint laufend. Mit der Löschtaste fängst du neu an. Für Kredite, Prozente oder Zahlensysteme siehe die Werkzeuge-App.",
      "حاسبة بسيطة للعمليات اليومية. انقر الأرقام والعمليات؛ تظهر النتيجة أثناء الإدخال. استخدم زرّ المسح للبدء من جديد. للقروض أو النِّسب أو أنظمة الأعداد، راجع تطبيق الأدوات."),
    t("browser", "Browser",
      "Open web pages here. Type an address or a search and go. You can also open pages the assistant prepares. Use the back gesture to return to the previous page.",
      "Hier öffnest du Webseiten. Gib eine Adresse oder Suche ein und los. Du kannst auch Seiten öffnen, die der Assistent vorbereitet. Mit der Zurück-Geste gelangst du zur vorherigen Seite.",
      "افتح صفحات الويب هنا. أدخِل عنواناً أو بحثاً وانطلق. يمكنك أيضاً فتح الصفحات التي يُجهّزها المساعد. استخدم إيماءة الرجوع للعودة إلى الصفحة السابقة."),

    // ---- System & Werkzeuge --------------------------------------------
    s("store", "Packages",
      "Install software from Ubuntu's package sources. Search for a package, open it and install; updates come the same way. Removing a package frees its space. This needs the compatibility layer to be ready.",
      "Installiere Software aus den Ubuntu-Paketquellen. Suche ein Paket, öffne es und installiere; Aktualisierungen kommen genauso. Ein Paket entfernen gibt seinen Platz frei. Dafür muss die Kompatibilitätsschicht bereit sein.",
      "ثبّت البرامج من مصادر حزم Ubuntu. ابحث عن حزمة وافتحها وثبّتها؛ وتأتي التحديثات بالطريقة نفسها. إزالة حزمة تحرّر مساحتها. يتطلّب ذلك جاهزية طبقة التوافق."),
    s("terminal", "Terminal",
      "A command line in the Ubuntu area of the device. Type commands and see their output. It is meant for advanced tasks; be careful with commands that change or delete files. The assistant can also run commands for you, with your approval.",
      "Eine Kommandozeile im Ubuntu-Bereich des Geräts. Tippe Befehle und sieh die Ausgabe. Für fortgeschrittene Aufgaben gedacht; Vorsicht bei Befehlen, die Dateien ändern oder löschen. Der Assistent kann Befehle auch für dich ausführen – mit deiner Zustimmung.",
      "سطر أوامر في منطقة Ubuntu بالجهاز. اكتب الأوامر وشاهِد مخرجاتها. مُخصّص للمهام المتقدّمة؛ انتبه للأوامر التي تغيّر الملفّات أو تحذفها. يمكن للمساعد أيضاً تنفيذ الأوامر نيابةً عنك بموافقتك."),
    s("ubuntuapps", "Ubuntu apps",
      "Graphical programs installed from Ubuntu packages appear here. Tap one to launch it in a window. Install more from the Packages screen. If an app is missing, it may need a package or a driver first.",
      "Grafische Programme aus Ubuntu-Paketen erscheinen hier. Tippe eines an, um es im Fenster zu starten. Weitere installierst du über den Paket-Bildschirm. Fehlt eine App, braucht sie vielleicht erst ein Paket oder einen Treiber.",
      "تظهر هنا البرامج الرسومية المثبّتة من حزم Ubuntu. انقر واحداً لتشغيله في نافذة. ثبّت المزيد من شاشة الحزم. إذا كان تطبيق مفقوداً، فقد يحتاج أولاً إلى حزمة أو برنامج تشغيل."),
    s("monitor", "Monitor",
      "See how the device is doing in real time: processor load, memory, temperature and network. Use it to spot what is slowing the device or draining the battery. The Tools app can end a heavy program.",
      "Sieh in Echtzeit, wie es dem Gerät geht: Prozessorlast, Speicher, Temperatur und Netzwerk. So erkennst du, was das Gerät bremst oder den Akku leert. Über die Werkzeuge-App kannst du ein forderndes Programm beenden.",
      "راقِب حالة الجهاز لحظياً: حِمل المعالج والذاكرة والحرارة والشبكة. استخدمه لاكتشاف ما يُبطئ الجهاز أو يستنزف البطارية. يمكن لتطبيق الأدوات إنهاء برنامج ثقيل."),
    s("privacy", "Privacy",
      "Control what apps may use and block ads. Switch the camera, microphone and location off system-wide when you want to be sure nothing uses them. Ad blocking speeds up pages and saves data. Settings here apply across the whole device.",
      "Steuere, was Apps nutzen dürfen, und blockiere Werbung. Schalte Kamera, Mikrofon und Ort systemweit aus, wenn du sichergehen willst, dass nichts sie nutzt. Werbeblockade beschleunigt Seiten und spart Daten. Die Einstellungen gelten fürs ganze Gerät.",
      "تحكّم بما يُسمَح للتطبيقات باستخدامه واحجب الإعلانات. أطفئ الكاميرا والميكروفون والموقع على مستوى النظام عندما تريد التأكّد أنّ لا شيء يستخدمها. حجب الإعلانات يُسرّع الصفحات ويوفّر البيانات. تنطبق هذه الإعدادات على الجهاز كله."),
    s("security", "Security",
      "Protect the device with a screen lock and encryption. A PIN or fingerprint keeps others out; encryption keeps your data unreadable if the device is lost. Set these up before you store anything private. Never share your PIN.",
      "Schütze das Gerät mit Bildschirmsperre und Verschlüsselung. Eine PIN oder der Fingerabdruck hält andere fern; Verschlüsselung macht deine Daten bei Verlust unlesbar. Richte das ein, bevor du Privates speicherst. Gib deine PIN nie weiter.",
      "احمِ الجهاز بقفل الشاشة والتشفير. رمز PIN أو البصمة يمنع الآخرين؛ والتشفير يجعل بياناتك غير قابلة للقراءة إن ضاع الجهاز. اضبط ذلك قبل تخزين أي شيء خاص. لا تُشارك رمز PIN أبداً."),
    s("sensors", "Sensors",
      "See the device's sensors live: motion, rotation, light, compass and proximity. It is useful to check that a sensor works, for example after a repair. The Tools app uses these for the level, compass and qibla.",
      "Sieh die Sensoren des Geräts live: Bewegung, Drehung, Licht, Kompass und Nähe. Nützlich, um zu prüfen, ob ein Sensor funktioniert, etwa nach einer Reparatur. Die Werkzeuge-App nutzt sie für Wasserwaage, Kompass und Qibla.",
      "شاهِد مستشعرات الجهاز لحظياً: الحركة والدوران والضوء والبوصلة والقرب. مفيد للتحقّق من عمل مستشعر، مثلاً بعد إصلاح. يستخدمها تطبيق الأدوات للميزان والبوصلة والقبلة."),
    s("gnss", "Location (GPS)",
      "Shows your position from the satellites and which ones are in view. A fix can take a minute outdoors with a clear sky. Apps use this location only when you allow it in Privacy. Turn it off to save battery.",
      "Zeigt deine Position aus den Satelliten und welche in Sicht sind. Im Freien unter klarem Himmel dauert ein Fix eine Minute. Apps nutzen diesen Ort nur, wenn du es unter Datenschutz erlaubst. Zum Akkusparen ausschalten.",
      "يعرض موقعك من الأقمار الصناعية وأيّها في المدى. قد يستغرق التثبيت دقيقة في الخارج تحت سماء صافية. تستخدم التطبيقات هذا الموقع فقط عندما تسمح بذلك في الخصوصية. أطفئه لتوفير البطارية."),
    t("telegram", "Telegram",
      "Chat on Telegram from the device. Sign in with your number the first time, then your chats appear. Messages sync with your other devices.",
      "Chatte über Telegram vom Gerät aus. Melde dich beim ersten Mal mit deiner Nummer an, dann erscheinen deine Chats. Nachrichten gleichen sich mit deinen anderen Geräten ab.",
      "تحدّث عبر Telegram من الجهاز. سجّل الدخول برقمك في المرة الأولى، ثم تظهر محادثاتك. تتزامن الرسائل مع أجهزتك الأخرى."),
    t("spotify", "Spotify",
      "Play music from Spotify. Sign in, then search or open your playlists. Playback controls also appear in the quick bar while music is playing.",
      "Spiele Musik über Spotify. Melde dich an, dann suche oder öffne deine Playlists. Steuerung erscheint auch in der Schnellleiste, während Musik läuft.",
      "شغّل الموسيقى من Spotify. سجّل الدخول ثم ابحث أو افتح قوائم التشغيل. تظهر عناصر التحكّم أيضاً في الشريط السريع أثناء تشغيل الموسيقى."),
    s("account", "Account",
      "Your Ultar account ties the device to your number and the cloud store. Sign in to sync and to reach cloud features. You can sign out at any time; your local data stays on the device.",
      "Dein Ultar-Konto verbindet das Gerät mit deiner Nummer und dem Cloud-Store. Melde dich an, um zu synchronisieren und Cloud-Funktionen zu nutzen. Du kannst dich jederzeit abmelden; deine lokalen Daten bleiben auf dem Gerät.",
      "يربط حساب Ultar الجهاز برقمك ومتجر السحابة. سجّل الدخول للمزامنة والوصول إلى ميزات السحابة. يمكنك تسجيل الخروج في أي وقت؛ تبقى بياناتك المحلية على الجهاز."),
    s("update", "System update",
      "Keep Ultar OS current here. The device checks for a new version and installs it; some updates arrive fully offline from a USB stick. Your data and settings are kept. Charge the device before a large update.",
      "Halte Ultar OS hier aktuell. Das Gerät sucht eine neue Version und installiert sie; manche Updates kommen komplett offline vom USB-Stick. Deine Daten und Einstellungen bleiben erhalten. Lade das Gerät vor einem großen Update.",
      "حافِظ على تحديث Ultar OS هنا. يبحث الجهاز عن نسخة جديدة ويثبّتها؛ وبعض التحديثات تصل دون إنترنت من ذاكرة USB. تبقى بياناتك وإعداداتك محفوظة. اشحن الجهاز قبل تحديث كبير."),

    // ---- Werkzeuge-App --------------------------------------------------
    t("tools", "Tools",
      "A collection of small, handy tools: compass, spirit level, qibla, prayer times, calculators (percent, loan, currency, number bases), password generator, SOS light, focus mode, countdown, to-do list and more. Tap any tool to open it. Sensor tools need the device's sensors.",
      "Eine Sammlung kleiner, praktischer Werkzeuge: Kompass, Wasserwaage, Qibla, Gebetszeiten, Rechner (Prozent, Kredit, Währung, Zahlensysteme), Kennwort-Generator, SOS-Licht, Fokusmodus, Countdown, Aufgabenliste und mehr. Tippe ein Werkzeug an. Sensor-Werkzeuge brauchen die Sensoren des Geräts.",
      "مجموعة أدوات صغيرة مفيدة: بوصلة، ميزان ماء، قبلة، أوقات الصلاة، حاسبات (نسبة، قرض، عملة، أنظمة أعداد)، مولّد كلمات مرور، ضوء استغاثة، وضع التركيز، عدّ تنازلي، قائمة مهام والمزيد. انقر أي أداة لفتحها. تحتاج أدوات المستشعرات إلى مستشعرات الجهاز."),
    t("compass", "Compass",
      "Points to magnetic north using the device's compass. Hold the device flat and turn slowly; move it in a figure-eight once to calibrate if it drifts. Stay away from metal and magnets for an accurate reading.",
      "Zeigt mit dem Kompass des Geräts nach magnetisch Nord. Halte das Gerät flach und drehe langsam; bei Abweichung einmal eine Acht in die Luft zeichnen zum Kalibrieren. Für eine genaue Anzeige Metall und Magnete meiden.",
      "تشير إلى الشمال المغناطيسي باستخدام بوصلة الجهاز. أمسِك الجهاز مستوياً وأدِرْه ببطء؛ وإن انحرف، ارسم به الرقم ثمانية في الهواء مرة للمعايرة. ابتعد عن المعادن والمغناطيس لقراءة دقيقة."),
    t("qibla", "Qibla",
      "Shows the direction of the Kaaba in Mecca for prayer. It uses the compass and your location, so allow location and hold the device flat. Turn until the marker lines up with the top. Calibrate the compass if the direction wavers.",
      "Zeigt die Richtung der Kaaba in Mekka fürs Gebet. Es nutzt Kompass und Standort, also Ort erlauben und das Gerät flach halten. Drehe, bis die Markierung oben steht. Bei schwankender Richtung den Kompass kalibrieren.",
      "تعرض اتجاه الكعبة في مكة للصلاة. تستخدم البوصلة وموقعك، لذا اسمح بالموقع وأمسِك الجهاز مستوياً. أدِرْه حتى تنطبق العلامة على الأعلى. عايِر البوصلة إذا اضطرب الاتجاه."),
    t("level", "Spirit level",
      "Turns the device into a spirit level. Lay it on a surface to check if it is flat, or hold it against a wall for vertical. The bubble and the angle in degrees show how far off level you are.",
      "Macht das Gerät zur Wasserwaage. Lege es auf eine Fläche, um zu prüfen, ob sie eben ist, oder halte es an eine Wand für senkrecht. Die Libelle und der Winkel in Grad zeigen die Abweichung.",
      "يحوّل الجهاز إلى ميزان ماء. ضعه على سطح للتأكّد من استوائه، أو أسنِده إلى جدار للعمودي. تُظهر الفقاعة والزاوية بالدرجات مقدار الميل."),
    t("prayer", "Prayer times",
      "Shows the five daily prayer times and the Hijri date for your city. Pick your city and calculation method; the times update for the day. Allow location for the most accurate times.",
      "Zeigt die fünf täglichen Gebetszeiten und das Hijri-Datum für deine Stadt. Wähle Stadt und Berechnungsmethode; die Zeiten gelten für den Tag. Für genaueste Zeiten den Standort erlauben.",
      "يعرض أوقات الصلوات الخمس وتاريخ الهجري لمدينتك. اختر مدينتك وطريقة الحساب؛ وتتحدّث الأوقات لليوم. اسمح بالموقع للحصول على أدقّ الأوقات."),
    t("password", "Password",
      "Generates strong, random passwords. Set the length and which characters to include, then tap to create one; tap the result to copy it. Longer passwords with mixed characters are harder to guess. Nothing is stored or sent.",
      "Erzeugt starke Zufallskennwörter. Stelle Länge und erlaubte Zeichen ein, tippe zum Erzeugen; das Ergebnis antippen zum Kopieren. Längere Kennwörter mit gemischten Zeichen sind schwerer zu erraten. Es wird nichts gespeichert oder gesendet.",
      "يولّد كلمات مرور قوية وعشوائية. اضبط الطول والأحرف المسموحة، ثم انقر للإنشاء؛ وانقر النتيجة لنسخها. كلمات المرور الأطول بأحرف مختلطة أصعب في التخمين. لا يُخزَّن شيء ولا يُرسَل."),
    t("loan", "Loan calculator",
      "Works out the monthly payment of a loan from the amount, the interest rate and the term. Enter the three values and read the payment and total cost. Use it to compare offers before you decide.",
      "Berechnet die monatliche Rate eines Kredits aus Betrag, Zinssatz und Laufzeit. Gib die drei Werte ein und lies Rate und Gesamtkosten ab. Nutze es, um Angebote vor der Entscheidung zu vergleichen.",
      "يحسب القسط الشهري لقرض من المبلغ ونسبة الفائدة والمدة. أدخِل القيم الثلاث واقرأ القسط والتكلفة الإجمالية. استخدمه لمقارنة العروض قبل أن تقرّر."),
    t("currency", "Currency",
      "Converts between currencies using rates you set. Enter an amount and pick the two currencies. Update the rate yourself so it stays current; the tool does its own arithmetic, not a live feed.",
      "Rechnet zwischen Währungen mit Kursen, die du einstellst. Gib einen Betrag ein und wähle die zwei Währungen. Aktualisiere den Kurs selbst, damit er stimmt; das Werkzeug rechnet selbst, ohne Live-Daten.",
      "يحوّل بين العملات بأسعار تحدّدها أنت. أدخِل مبلغاً واختر العملتين. حدّث السعر بنفسك ليبقى حديثاً؛ تحسب الأداة بنفسها دون بثّ مباشر."),
    t("sos", "SOS light",
      "Flashes the torch in the SOS pattern to signal for help. Tap to start and again to stop. It is loud and bright, so use it only in a real emergency; the Emergency screen also holds your contacts and medical note.",
      "Blinkt die Taschenlampe im SOS-Muster, um Hilfe zu signalisieren. Antippen zum Starten, erneut zum Stoppen. Es ist auffällig – nur im echten Notfall nutzen; der Notfall-Bildschirm enthält auch deine Kontakte und den medizinischen Hinweis.",
      "يُومِض المصباح بنمط SOS لطلب المساعدة. انقر للبدء ومرة أخرى للإيقاف. إنه لافت وساطع، فاستخدمه فقط في حالة طارئة حقيقية؛ وتحتوي شاشة الطوارئ أيضاً على جهات اتصالك وملاحظتك الطبية."),
    t("focus", "Focus",
      "Focus mode mutes notifications and dims distractions for a set time, so you can work or rest. Start it with a duration; the device stays quiet until it ends or you stop it. Alarms and calls you allow still come through.",
      "Der Fokusmodus schaltet Benachrichtigungen stumm und dämpft Ablenkungen für eine gewählte Zeit, damit du arbeiten oder ruhen kannst. Starte ihn mit einer Dauer; das Gerät bleibt still, bis sie endet oder du stoppst. Erlaubte Wecker und Anrufe kommen weiter durch.",
      "يكتم وضع التركيز الإشعارات ويخفّف المشتّتات لمدة محدّدة، لتعمل أو تستريح. ابدأه بمدة؛ يبقى الجهاز هادئاً حتى تنتهي أو توقفه. تظل المنبّهات والمكالمات التي تسمح بها تصلك."),
    t("countdown", "Countdown",
      "Counts down to a date or event you set, showing the days left. Use it for a trip, a deadline or a birthday. It keeps counting even when the app is closed.",
      "Zählt zu einem Datum oder Ereignis herunter und zeigt die verbleibenden Tage. Nutze es für eine Reise, eine Frist oder einen Geburtstag. Es zählt weiter, auch wenn die App geschlossen ist.",
      "يعدّ تنازلياً إلى تاريخ أو حدث تحدّده، ويعرض الأيام المتبقية. استخدمه لرحلة أو موعد نهائي أو عيد ميلاد. يستمرّ العدّ حتى لو أُغلِق التطبيق."),
    t("todo", "To-do list",
      "A simple checklist. Add a task, tap it to mark it done, and hold to remove it. Finished tasks move aside so the open ones stay in front. Good for a quick list you do not want to forget.",
      "Eine einfache Checkliste. Füge eine Aufgabe hinzu, tippe sie zum Erledigen an, halte sie zum Entfernen. Erledigte rücken zur Seite, damit die offenen vorn bleiben. Gut für eine schnelle Liste, die du nicht vergessen willst.",
      "قائمة تحقّق بسيطة. أضِف مهمة، انقرها لتمييزها كمنجزة، واضغط مطوّلاً لإزالتها. تنزاح المهام المنجزة جانباً لتبقى المفتوحة في المقدّمة. مناسبة لقائمة سريعة لا تريد نسيانها."),
    t("reminders", "Reminders",
      "Reminds you of something at a time you choose. Add a reminder with a note and a time; it alerts you even when the screen is off. Use it for medicine, a call or an appointment.",
      "Erinnert dich zu einer gewählten Zeit an etwas. Füge eine Erinnerung mit Notiz und Zeit hinzu; sie meldet sich auch bei ausgeschaltetem Bildschirm. Nutze sie für Medikamente, einen Anruf oder einen Termin.",
      "يذكّرك بشيء في وقت تختاره. أضِف تذكيراً بملاحظة ووقت؛ ينبّهك حتى والشاشة مطفأة. استخدمه للدواء أو مكالمة أو موعد."),
    s("emergency", "Emergency",
      "Your emergency information in one place: emergency contacts and a short medical note (blood type, allergies). It can be reached quickly so help knows whom to call. Fill it in before you need it, and keep it short and clear.",
      "Deine Notfall-Infos an einem Ort: Notfallkontakte und ein kurzer medizinischer Hinweis (Blutgruppe, Allergien). Schnell erreichbar, damit Helfer wissen, wen sie anrufen. Trage es ein, bevor du es brauchst, und halte es kurz und klar.",
      "معلومات الطوارئ في مكان واحد: جهات اتصال الطوارئ وملاحظة طبية قصيرة (فصيلة الدم، الحساسية). يمكن الوصول إليها بسرعة ليعرف المسعِف بمن يتّصل. املأها قبل أن تحتاجها، وأبقِها قصيرة وواضحة."),

    // ---- Übersicht, Pflege, Modi ---------------------------------------
    s("recents", "Recent apps",
      "The third navigation button shows the apps you used lately, newest first. Tap one to jump back to it. Hold an app to pin it on top of everything; hold Home to release a pinned app.",
      "Die dritte Navigationstaste zeigt deine zuletzt genutzten Apps, neueste zuerst. Tippe eine an, um zurückzuspringen. Eine App halten, um sie über allem anzuheften; Start halten, um eine angeheftete App zu lösen.",
      "يعرض زرّ التنقّل الثالث التطبيقات التي استخدمتها مؤخراً، الأحدث أولاً. انقر واحداً للعودة إليه. اضغط مطوّلاً على تطبيق لتثبيته فوق كل شيء؛ واضغط مطوّلاً على «الرئيسية» لتحرير تطبيق مثبّت."),
    s("clipboard", "Clipboard",
      "Keeps the recent things you copied, so you can paste an older one. Tap an entry to copy it again; hold to remove it. It helps when you copy several items in a row.",
      "Bewahrt die zuletzt kopierten Dinge auf, damit du auch Älteres einfügen kannst. Tippe einen Eintrag an, um ihn erneut zu kopieren; halten zum Entfernen. Praktisch, wenn du mehrere Dinge nacheinander kopierst.",
      "يحتفظ بما نسخته مؤخراً، لتتمكّن من لصق عنصر أقدم. انقر عنصراً لنسخه ثانيةً؛ واضغط مطوّلاً لإزالته. مفيد عندما تنسخ عدة عناصر تباعاً."),
    s("care", "Device care",
      "A quick check-up of the device: battery health, free storage and what to clean up. Follow the suggestions to free space and keep things smooth. It never deletes anything without your tap.",
      "Ein schneller Gesundheits-Check des Geräts: Akkuzustand, freier Speicher und was sich bereinigen lässt. Folge den Vorschlägen, um Platz zu schaffen und alles flüssig zu halten. Es löscht nie etwas ohne deinen Tipp.",
      "فحص سريع لحالة الجهاز: صحة البطارية والمساحة الحرة وما يمكن تنظيفه. اتّبع الاقتراحات لتحرير مساحة وإبقاء الأداء سلساً. لا يحذف شيئاً أبداً دون نقرة منك."),
    s("usage", "Screen time",
      "Shows how long you used the device and which apps most. Use it to notice habits and to set limits if you want less screen time. The numbers are kept on the device only.",
      "Zeigt, wie lange du das Gerät genutzt hast und welche Apps am meisten. Nutze es, um Gewohnheiten zu sehen und bei Bedarf Grenzen zu setzen. Die Zahlen bleiben nur auf dem Gerät.",
      "يعرض مدة استخدامك للجهاز وأكثر التطبيقات استخداماً. استخدمه لملاحظة عاداتك ولضبط حدود إن أردت وقتاً أقلّ أمام الشاشة. تبقى الأرقام على الجهاز فقط."),
    s("batthist", "Battery history",
      "Shows how the battery level changed over the day and the estimated time left. Use it to see what drained the battery and plan a charge. A steep drop usually means a heavy app or poor signal.",
      "Zeigt, wie sich der Akkustand über den Tag verändert hat, und die geschätzte Restlaufzeit. So erkennst du, was den Akku geleert hat, und planst das Laden. Ein steiler Abfall heißt meist: fordernde App oder schlechtes Signal.",
      "يعرض كيف تغيّر مستوى البطارية خلال اليوم والوقت المتبقّي المقدَّر. استخدمه لمعرفة ما استنزف البطارية ولتخطيط الشحن. الهبوط الحادّ يعني عادةً تطبيقاً ثقيلاً أو إشارة ضعيفة."),
    s("storagex", "Storage analysis",
      "Breaks down what fills the storage: photos, apps, downloads and caches. Tap a group to see and clear what you no longer need. Clearing caches is safe and frees space quickly.",
      "Schlüsselt auf, was den Speicher füllt: Fotos, Apps, Downloads und Zwischenspeicher. Tippe eine Gruppe an, um Unnötiges zu sehen und zu löschen. Zwischenspeicher zu leeren ist sicher und schafft schnell Platz.",
      "يوضّح ما يملأ التخزين: الصور والتطبيقات والتنزيلات والذاكرة المؤقتة. انقر مجموعة لرؤية ما لم تعُد تحتاجه وحذفه. مسح الذاكرة المؤقتة آمن ويحرّر مساحة بسرعة."),
    s("modes", "Modes & automation",
      "Set up modes that change the device on their own: a night mode that dims and silences, a scheduled restart, automatic dark theme and more. Turn on the ones you want and set their times. They run quietly in the background.",
      "Richte Modi ein, die das Gerät selbst umstellen: ein Nachtmodus, der dimmt und stummschaltet, ein geplanter Neustart, automatisches dunkles Design und mehr. Schalte die gewünschten ein und stelle ihre Zeiten. Sie laufen leise im Hintergrund.",
      "اضبط أوضاعاً تغيّر الجهاز تلقائياً: وضع ليلي يخفّف الإضاءة ويكتم الصوت، وإعادة تشغيل مجدولة، ومظهر داكن تلقائي والمزيد. فعّل ما تريد واضبط أوقاته. تعمل بهدوء في الخلفية."),
    s("blocked", "Blocked numbers",
      "Numbers you block never ring and their messages are kept aside. Add a number here, or block one from the call log. Remove a number from the list to let it through again.",
      "Gesperrte Nummern klingeln nie, ihre Nachrichten werden beiseitegelegt. Füge hier eine Nummer hinzu oder sperre eine aus der Anrufliste. Entferne eine Nummer aus der Liste, um sie wieder durchzulassen.",
      "الأرقام التي تحظرها لا ترنّ أبداً وتُوضَع رسائلها جانباً. أضِف رقماً هنا، أو احظر واحداً من سجلّ المكالمات. أزِل رقماً من القائمة للسماح له مجدداً."),

    // ---- Gerät-Einstellungen -------------------------------------------
    s("display", "Display",
      "Adjust brightness, text size, screen timeout and the dark theme. Lower brightness and a shorter timeout save battery. Automatic brightness follows the light sensor. Night light warms the colours in the evening.",
      "Stelle Helligkeit, Textgröße, Bildschirm-Zeitlimit und das dunkle Design ein. Weniger Helligkeit und ein kürzeres Zeitlimit sparen Akku. Automatische Helligkeit folgt dem Lichtsensor. Das Nachtlicht wärmt abends die Farben.",
      "اضبط السطوع وحجم النص ومهلة إطفاء الشاشة والمظهر الداكن. السطوع المنخفض والمهلة الأقصر يوفّران البطارية. السطوع التلقائي يتبع مستشعر الضوء. الضوء الليلي يُدفئ الألوان مساءً."),
    s("sound", "Sound",
      "Set the volume and the sound profile. Turn the ringer and alerts up or down, or switch to silent. Sound profiles let you save settings for, say, home and work. Focus mode can silence everything on a schedule.",
      "Stelle Lautstärke und Tonprofil ein. Klingelton und Hinweise lauter oder leiser, oder auf stumm. Tonprofile speichern Einstellungen z. B. für zu Hause und die Arbeit. Der Fokusmodus kann nach Plan alles stummschalten.",
      "اضبط مستوى الصوت وملف الصوت. ارفع نغمة الرنين والتنبيهات أو اخفضها، أو حوّل إلى الصامت. تتيح ملفّات الصوت حفظ إعدادات للمنزل والعمل مثلاً. يمكن لوضع التركيز كتم كل شيء وفق جدول."),
    s("time", "Date & time",
      "Set the date, time and time zone, or let the network set them automatically. Automatic time keeps the clock right across time zones. Choose the 12- or 24-hour format here too.",
      "Stelle Datum, Uhrzeit und Zeitzone ein oder lass sie vom Netz automatisch setzen. Automatische Zeit hält die Uhr über Zeitzonen hinweg korrekt. Auch das 12- oder 24-Stunden-Format wählst du hier.",
      "اضبط التاريخ والوقت والمنطقة الزمنية، أو دع الشبكة تضبطها تلقائياً. الوقت التلقائي يُبقي الساعة صحيحة عبر المناطق الزمنية. اختر تنسيق 12 أو 24 ساعة هنا أيضاً."),
    s("lang", "Language",
      "Choose the system language: Arabic, English or German. The whole interface changes at once, and Arabic lays out from right to left. The assistant also replies in your language.",
      "Wähle die Systemsprache: Arabisch, Englisch oder Deutsch. Die ganze Oberfläche wechselt sofort, und Arabisch läuft von rechts nach links. Auch der Assistent antwortet in deiner Sprache.",
      "اختر لغة النظام: العربية أو الإنجليزية أو الألمانية. تتغيّر الواجهة كلها فوراً، وتُعرَض العربية من اليمين إلى اليسار. يردّ المساعد أيضاً بلغتك."),
    s("system", "System",
      "Shows the device and Ultar OS: name, version, memory and storage. From here you reach backup, reset and the device name. Check the version here before reporting a problem.",
      "Zeigt Gerät und Ultar OS: Name, Version, Speicher und Ablage. Von hier erreichst du Sicherung, Zurücksetzen und den Gerätenamen. Prüfe hier die Version, bevor du ein Problem meldest.",
      "يعرض الجهاز ونظام Ultar OS: الاسم والإصدار والذاكرة والتخزين. من هنا تصل إلى النسخ الاحتياطي وإعادة الضبط واسم الجهاز. تحقّق من الإصدار هنا قبل الإبلاغ عن مشكلة."),
    s("hardware", "Hardware & drivers",
      "Lists the device's hardware and whether each part has a working driver. A part marked without a driver may not work until its driver or package is installed. Useful after a repair or a fresh install.",
      "Listet die Hardware des Geräts und ob jedes Teil einen funktionierenden Treiber hat. Ein als ohne Treiber markiertes Teil funktioniert evtl. erst, wenn sein Treiber oder Paket installiert ist. Nützlich nach Reparatur oder Neuinstallation.",
      "يسرد عتاد الجهاز وما إذا كان لكل جزء برنامج تشغيل يعمل. قد لا يعمل جزء مُعلَّم «دون برنامج تشغيل» حتى يُثبَّت تعريفه أو حزمته. مفيد بعد إصلاح أو تثبيت جديد."),
    s("storage", "Storage",
      "Shows how much space is used and free. For a detailed breakdown and cleanup, open Storage analysis in the Tools app. Move photos or remove unused apps when space runs low.",
      "Zeigt, wie viel Speicher belegt und frei ist. Für eine genaue Aufschlüsselung und Bereinigung öffne die Speicher-Analyse in der Werkzeuge-App. Verschiebe Fotos oder entferne ungenutzte Apps, wenn es eng wird.",
      "يعرض مقدار المساحة المستخدمة والحرة. للحصول على تفصيل وتنظيف، افتح «تحليل التخزين» في تطبيق الأدوات. انقل الصور أو أزِل التطبيقات غير المستخدمة عند قلة المساحة."),
    s("services", "Services",
      "The background services that keep the system running — network, radio, sensors and more. You can restart one that misbehaves. Stopping a service may switch off a feature, so change these only if you know why.",
      "Die Hintergrunddienste, die das System am Laufen halten – Netzwerk, Funk, Sensoren und mehr. Einen hakelnden Dienst kannst du neu starten. Einen Dienst zu stoppen kann eine Funktion abschalten – ändere das nur, wenn du weißt warum.",
      "الخدمات الخلفية التي تُبقي النظام يعمل — الشبكة والراديو والمستشعرات والمزيد. يمكنك إعادة تشغيل خدمة مضطربة. إيقاف خدمة قد يُطفئ ميزة، فلا تغيّرها إلا إذا عرفت السبب."),
];

fn li(lang: &str) -> usize {
    match lang {
        "de" => 1,
        "ar" => 2,
        _ => 0,
    }
}

/// Thema nach Kennung.
pub fn get(id: &str) -> Option<&'static Topic> {
    TOPICS.iter().find(|t| t.id == id)
}

/// Hilfetext eines Themas in der Sprache.
pub fn body(id: &str, lang: &str) -> Option<&'static str> {
    get(id).map(|t| t.body[li(lang)])
}

/// Sprachindex in `Topic::body` (0=en, 1=de, 2=ar) – öffentlich für die Oberfläche.
pub fn li_pub(lang: &str) -> usize {
    li(lang)
}

/// Erste Zeile/Satz eines Textes, getrimmt (für die Themenliste).
pub fn first_line(s: &str) -> &str {
    first_sentence(s).trim()
}

/// Erster Satz des englischen Textes (für Katalog/Kurzfassung).
fn first_sentence(s: &str) -> &str {
    match s.find(". ") {
        Some(i) => &s[..=i],
        None => s,
    }
}

/// Kompakter Katalog für den Systemprompt des Agenten: eine Zeile je Thema,
/// Titel (englisch) + erster Satz. So weiß das Modell, was jeder Teil tut.
pub fn digest() -> String {
    let mut out = String::with_capacity(4096);
    for t in TOPICS {
        out.push_str("- ");
        out.push_str(t.title);
        out.push_str(": ");
        out.push_str(first_sentence(t.body[0]).trim());
        out.push('\n');
    }
    out
}

/// Vollständige Wissensbasis als Markdown nach `dir` schreiben: ein Index plus
/// eine Datei je Thema (alle drei Sprachen). Basis für den lokalen Agenten und
/// externe Werkzeuge. Fehler werden still verschluckt (best effort).
pub fn export(dir: &str) {
    if std::fs::create_dir_all(dir).is_err() {
        return;
    }
    let mut index = String::from("# Ultar OS – Hilfe / Help\n\nKM Code Craft (Mohamad Anas Kara Mohamad)\n\n");
    for t in TOPICS {
        index.push_str(&format!("- [{}]({}.md) — {}\n", t.title, t.id, first_sentence(t.body[0]).trim()));
        let doc = format!(
            "# {title}\n\nid: `{id}`\n\n## English\n{en}\n\n## Deutsch\n{de}\n\n## العربية\n{ar}\n",
            title = t.title,
            id = t.id,
            en = t.body[0],
            de = t.body[1],
            ar = t.body[2],
        );
        let _ = std::fs::write(format!("{dir}/{}.md", t.id), doc);
    }
    let _ = std::fs::write(format!("{dir}/index.md"), index);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_unique_and_complete() {
        for t in TOPICS {
            assert!(!t.id.is_empty());
            assert!(!t.body[0].is_empty() && !t.body[1].is_empty() && !t.body[2].is_empty(), "leer: {}", t.id);
            assert_eq!(TOPICS.iter().filter(|x| x.id == t.id).count(), 1, "doppelt: {}", t.id);
        }
    }

    #[test]
    fn digest_covers_all() {
        let d = digest();
        assert_eq!(d.lines().count(), TOPICS.len());
        assert!(d.contains("Mobile network"));
    }

    #[test]
    fn lookup_and_lang() {
        assert!(body("compass", "ar").unwrap().contains("بوصلة"));
        assert!(body("compass", "de").unwrap().contains("Kompass"));
        assert_eq!(body("nope", "en"), None);
    }
}
