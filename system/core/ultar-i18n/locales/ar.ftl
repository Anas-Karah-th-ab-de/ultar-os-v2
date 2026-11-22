# Ultar OS – base catalog, Arabic (reference language).
# Every key here must exist in en.ftl and de.ftl with the same placeholders (checked by tests and ultar-i18n-check).


# System
system.name = Ultar OS
system.vendor = KM Code Craft
system.boot.starting = جارٍ بدء تشغيل Ultar OS…
system.boot.menu = قائمة الإقلاع
system.boot.rescue = وضع الإنقاذ
system.boot.console = طرفية الطوارئ
system.boot.failed = تعذّر بدء تشغيل النظام.
lang.name.ar = العربية
lang.name.en = الإنجليزية
lang.name.de = الألمانية

# Common actions
action.ok = موافق
action.cancel = إلغاء
action.back = رجوع
action.next = التالي
action.yes = نعم
action.no = لا
action.done = تم
action.close = إغلاق
action.save = حفظ
action.save_as = حفظ باسم
action.open = فتح
action.delete = حذف
action.remove = إزالة
action.add = إضافة
action.edit = تعديل
action.rename = إعادة تسمية
action.copy = نسخ
action.cut = قص
action.paste = لصق
action.share = مشاركة
action.search = بحث
action.search_again = بحث من جديد
action.retry = إعادة المحاولة
action.apply = تطبيق
action.install = تثبيت
action.uninstall = إلغاء التثبيت
action.update = تحديث
action.download = تنزيل
action.restart = إعادة التشغيل
action.shut_down = إيقاف التشغيل
action.lock = قفل
action.unlock = فتح القفل
action.connect = اتصال
action.disconnect = قطع الاتصال
action.continue = متابعة
action.skip = تخطي
action.finish = إنهاء
action.select = اختيار
action.select_all = تحديد الكل
action.refresh = تحديث القائمة
action.enable = تفعيل
action.disable = تعطيل
action.show = إظهار
action.hide = إخفاء
action.send = إرسال
action.help = مساعدة
action.undo = تراجع
action.redo = إعادة
action.set_up = إعداد
action.allow = سماح
action.deny = رفض
action.start = تشغيل
action.stop = إيقاف
action.erase = مسح

# States
state.on = مُفعّل
state.off = متوقف
state.enabled = مُفعّل
state.disabled = معطّل
state.connected = متصل
state.disconnected = غير متصل
state.connecting = جارٍ الاتصال…
state.running = يعمل
state.stopped = متوقف
state.loading = جارٍ التحميل…
state.please_wait = يرجى الانتظار
state.installing = جارٍ التثبيت…
state.downloading = جارٍ التنزيل…
state.searching = جارٍ البحث…
state.installed = مثبّت
state.not_installed = غير مثبّت
state.available = متاح
state.unavailable = غير متاح
state.recommended = موصى به
state.unknown = غير معروف
state.saved = تم الحفظ
state.not_saved = غير محفوظ
state.ready = جاهز
state.failed = فشل
state.set_up = مُعدّ
state.not_set_up = غير مُعدّ
state.empty = فارغ

# Installer
installer.welcome = مرحباً بك في Ultar OS
installer.welcome_sub = اختر لغة التثبيت
installer.step.language = اللغة
installer.step.disk = القرص
installer.step.confirm = التأكيد
installer.step.install = التثبيت
installer.step.done = انتهى
installer.step_of = الخطوة {current} من {total}
installer.page_of = صفحة {current} من {total}
installer.disk.title = أين نثبّت Ultar OS؟
installer.disk.subtitle = اختر القرص. سيُمسح كل محتواه.
installer.disk.none = لا يوجد قرص مناسب. وصّل قرصاً ثم ابحث من جديد.
installer.confirm.title = تأكيد التثبيت
installer.confirm.warning = ستُمسح كل البيانات على هذا القرص نهائياً:
installer.confirm.button = مسح وتثبيت
installer.progress.title = جارٍ تثبيت Ultar OS
installer.progress.wait = لا تطفئ الجهاز.
installer.phase.partition = كتابة جدول الأقسام
installer.phase.copy = النسخ
installer.phase.verify = التحقق
installer.verified = تم التحقق من كل البيانات.
installer.done.title = تم تثبيت Ultar OS
installer.done.text = أعد التشغيل الآن. سيبدأ النظام المثبّت حتى لو بقي وسيط التثبيت موصولاً.
installer.to_console = إلى الطرفية
installer.devices.title = الأجهزة المكتشفة
installer.devices.subtitle = فحص Ultar العتاد والشبكة قبل التثبيت.
installer.devices.all = كل الأجهزة
installer.network.connect = الاتصال بالشبكة
installer.network.offline = غير متصل بالشبكة
installer.network.no_adapter = لا توجد بطاقة شبكة لها تعريف
installer.internet.ok = الإنترنت متاح
installer.internet.none = لا يوجد اتصال بالإنترنت
installer.error.title = فشل التثبيت
installer.hint = تنقّل بالأسهم أو Tab، واختر بـ Enter. الفأرة تعمل أيضاً.
installer.source = المصدر
installer.via_framebuffer = عبر ذاكرة الشاشة

# Settings panels
settings.title = الإعدادات
settings.network = الشبكة
settings.wifi = واي فاي
settings.bluetooth = بلوتوث
settings.mobile_data = بيانات الجوّال
settings.airplane_mode = وضع الطيران
settings.hotspot = نقطة اتصال
settings.vpn = VPN
settings.display.title = الشاشة
settings.display.brightness = السطوع
settings.display.wallpaper = الخلفية
settings.display.theme = المظهر
settings.display.theme.dark = داكن
settings.display.theme.light = فاتح
settings.display.rotation = تدوير الشاشة
settings.sound.title = الصوت
settings.sound.volume = مستوى الصوت
settings.notifications = الإشعارات
settings.battery = البطارية
settings.storage = التخزين
settings.security = الأمان
settings.privacy = الخصوصية
settings.location = الموقع
settings.language = اللغة
settings.keyboard = لوحة المفاتيح
settings.date_time = التاريخ والوقت
settings.date_time.week_start = بداية الأسبوع
settings.date_time.hijri = إظهار التاريخ الهجري
settings.apps = التطبيقات
settings.accounts = الحسابات
settings.updates = التحديثات
settings.drivers = التعريفات
settings.cells = الخلايا والحاويات
settings.assistant = المساعد
settings.fingerprint = بصمة الإصبع
settings.face_unlock = فتح القفل بالوجه
settings.connected_devices = الأجهزة المتصلة
settings.about = حول الجهاز

# Hardware classes and driver states
hw.class.net = شبكة سلكية
hw.class.wifi = شبكة لاسلكية
hw.class.bluetooth = بلوتوث
hw.class.modem = مودم الجوّال
hw.class.storage = التخزين
hw.class.usb = USB
hw.class.gpu = الرسوميات
hw.class.display = الشاشة
hw.class.audio = الصوت
hw.class.camera = الكاميرا
hw.class.input = أجهزة الإدخال
hw.class.touchscreen = شاشة اللمس
hw.class.sensors = المستشعرات
hw.class.gnss = تحديد الموقع (GNSS)
hw.class.fingerprint = قارئ البصمة
hw.class.battery = البطارية
hw.class.printer = الطابعات
hw.class.bridge = الشرائح والجسور
hw.class.other = أجهزة أخرى
hw.state.driver_ok = التعريف يعمل
hw.state.driver_missing = لا يوجد تعريف بعد
hw.state.driver_loading = جارٍ تحميل التعريف…
hw.state.driver_failed = فشل تحميل التعريف
hw.state.firmware_missing = ملف البرنامج الثابت مفقود
hw.state.not_present = الجهاز غير موجود
hw.state.disabled = الجهاز معطّل
hw.device_named = {class}: {name}

# Errors
error.generic = حدث خطأ.
error.title = خطأ
error.not_found = العنصر غير موجود.
error.file_not_found = الملف «{name}» غير موجود.
error.permission_denied = ليست لديك صلاحية لهذا الإجراء.
error.no_space = لا توجد مساحة تخزين كافية.
error.no_network = لا يوجد اتصال بالشبكة.
error.timeout = انتهت مهلة العملية.
error.invalid_input = القيمة المدخلة غير صحيحة.
error.already_exists = «{name}» موجود مسبقاً.
error.read_failed = تعذّرت القراءة من «{name}».
error.write_failed = تعذّرت الكتابة إلى «{name}».
error.open_failed = تعذّر فتح «{name}».
error.verify_failed = فشل التحقق من البيانات.
error.signature_invalid = التوقيع غير صالح.
error.unsupported = هذا غير مدعوم على هذا الجهاز.
error.busy = الجهاز مشغول. حاول بعد قليل.
error.cancelled = أُلغيت العملية.
error.driver_missing = لا يوجد تعريف لهذا الجهاز: {name}
error.unknown_command = أمر غير معروف: {name}
error.what_to_do = ما العمل؟
error.try_again = حاول مرة أخرى.

# Editions and form factors
edition.title = الإصدار
edition.phone = إصدار الهاتف
edition.desktop = إصدار سطح المكتب
edition.server = إصدار الخادم
form.phone = هاتف
form.tablet = جهاز لوحي
form.laptop = حاسوب محمول
form.desktop = حاسوب مكتبي
form.server = خادم
form.embedded = جهاز مدمج

# Units ({value} is already formatted with Western digits)
unit.size.b = {value} بايت
unit.size.kb = {value} كيلوبايت
unit.size.mb = {value} ميغابايت
unit.size.gb = {value} غيغابايت
unit.size.tb = {value} تيرابايت
unit.percent = {value}%
unit.speed.kbps = {value} كيلوبت/ث
unit.speed.mbps = {value} ميغابت/ث
unit.celsius = {value}°م
unit.mah = {value} ملي أمبير ساعة
unit.hz = {value} هرتز
unit.pixels = {width}×{height} بكسل

# Counts (plural forms; Arabic needs all six)
count.items = {count ->
    [zero] لا عناصر
    [one] عنصر واحد
    [two] عنصران
    [few] {count} عناصر
    [many] {count} عنصراً
   *[other] {count} عنصر
}
count.files = {count ->
    [zero] لا ملفات
    [one] ملف واحد
    [two] ملفان
    [few] {count} ملفات
    [many] {count} ملفاً
   *[other] {count} ملف
}
count.apps = {count ->
    [zero] لا تطبيقات
    [one] تطبيق واحد
    [two] تطبيقان
    [few] {count} تطبيقات
    [many] {count} تطبيقاً
   *[other] {count} تطبيق
}
count.devices = {count ->
    [zero] لا أجهزة
    [one] جهاز واحد
    [two] جهازان
    [few] {count} أجهزة
    [many] {count} جهازاً
   *[other] {count} جهاز
}
count.updates = {count ->
    [zero] لا تحديثات
    [one] تحديث واحد
    [two] تحديثان
    [few] {count} تحديثات
    [many] {count} تحديثاً
   *[other] {count} تحديث
}
count.seconds = {count ->
    [zero] لا ثوانٍ
    [one] ثانية واحدة
    [two] ثانيتان
    [few] {count} ثوانٍ
    [many] {count} ثانية
   *[other] {count} ثانية
}
count.minutes = {count ->
    [zero] لا دقائق
    [one] دقيقة واحدة
    [two] دقيقتان
    [few] {count} دقائق
    [many] {count} دقيقة
   *[other] {count} دقيقة
}
count.hours = {count ->
    [zero] لا ساعات
    [one] ساعة واحدة
    [two] ساعتان
    [few] {count} ساعات
    [many] {count} ساعة
   *[other] {count} ساعة
}
count.days = {count ->
    [zero] لا أيام
    [one] يوم واحد
    [two] يومان
    [few] {count} أيام
    [many] {count} يوماً
   *[other] {count} يوم
}
time.minutes_ago = {count ->
    [zero] الآن
    [one] قبل دقيقة واحدة
    [two] قبل دقيقتين
    [few] قبل {count} دقائق
    [many] قبل {count} دقيقة
   *[other] قبل {count} دقيقة
}
time.now = الآن

# Calendar

# Gregorian months; Arabic uses the Levantine names (Syria)
cal.month.1 = كانون الثاني
cal.month.2 = شباط
cal.month.3 = آذار
cal.month.4 = نيسان
cal.month.5 = أيار
cal.month.6 = حزيران
cal.month.7 = تموز
cal.month.8 = آب
cal.month.9 = أيلول
cal.month.10 = تشرين الأول
cal.month.11 = تشرين الثاني
cal.month.12 = كانون الأول

# Gregorian months, alternative Arabic names (Egypt, Gulf)
cal.month.alt.1 = يناير
cal.month.alt.2 = فبراير
cal.month.alt.3 = مارس
cal.month.alt.4 = أبريل
cal.month.alt.5 = مايو
cal.month.alt.6 = يونيو
cal.month.alt.7 = يوليو
cal.month.alt.8 = أغسطس
cal.month.alt.9 = سبتمبر
cal.month.alt.10 = أكتوبر
cal.month.alt.11 = نوفمبر
cal.month.alt.12 = ديسمبر

# Hijri months
cal.hijri.month.1 = محرّم
cal.hijri.month.2 = صفر
cal.hijri.month.3 = ربيع الأول
cal.hijri.month.4 = ربيع الآخر
cal.hijri.month.5 = جمادى الأولى
cal.hijri.month.6 = جمادى الآخرة
cal.hijri.month.7 = رجب
cal.hijri.month.8 = شعبان
cal.hijri.month.9 = رمضان
cal.hijri.month.10 = شوّال
cal.hijri.month.11 = ذو القعدة
cal.hijri.month.12 = ذو الحجة
cal.day.sun = الأحد
cal.day.mon = الاثنين
cal.day.tue = الثلاثاء
cal.day.wed = الأربعاء
cal.day.thu = الخميس
cal.day.fri = الجمعة
cal.day.sat = السبت
cal.day.short.sun = أحد
cal.day.short.mon = اثنين
cal.day.short.tue = ثلاثاء
cal.day.short.wed = أربعاء
cal.day.short.thu = خميس
cal.day.short.fri = جمعة
cal.day.short.sat = سبت
cal.format.numeric = {day}/{month}/{year}
cal.format.long = {day} {month} {year}
cal.format.long_weekday = {weekday}، {day} {month} {year}
cal.format.hijri = {day} {month} {year} هـ
cal.format.time12 = {time} {period}
cal.time.am = ص
cal.time.pm = م
cal.today = اليوم
cal.yesterday = أمس
cal.tomorrow = غداً
cal.gregorian = ميلادي
cal.hijri = هجري

# Catalog checker (ultar-i18n-check)
check.usage = الاستخدام: ultar-i18n-check <المجلد>
check.ok = ملفات الترجمة مكتملة ومتطابقة.
check.checked = الملفات المفحوصة: {files}
check.missing = {file}: المفتاح «{key}» مفقود
check.extra = {file}: المفتاح «{key}» غير موجود في الملف المرجعي {reference}
check.placeholders = {file}: متغيرات المفتاح «{key}» تختلف عن المرجع (ناقص: {missing}؛ زائد: {unexpected})
check.plurals = {file}: المفتاح «{key}» تنقصه صيغ الجمع: {categories}
check.parse = {file}: السطر {line}: {reason}
check.no_reference = الملف المرجعي {reference} غير موجود في {dir}
check.no_catalogs = لا توجد ملفات ‎.ftl في {dir}
check.io = تعذّرت قراءة {path}
check.summary = {count ->
    [zero] لم يُعثر على مشكلات
    [one] عُثر على مشكلة واحدة
    [two] عُثر على مشكلتين
    [few] عُثر على {count} مشكلات
    [many] عُثر على {count} مشكلة
   *[other] عُثر على {count} مشكلة
}

# Messages of the i18n system itself
i18n.parse.invalid_key = اسم المفتاح غير صالح
i18n.parse.missing_equals = علامة = مفقودة
i18n.parse.duplicate_key = المفتاح مكرر
i18n.parse.empty_value = النص فارغ
i18n.parse.unclosed_brace = قوس مفتوح لم يُغلق
i18n.parse.unexpected_closing_brace = قوس إغلاق بلا قوس فتح
i18n.parse.invalid_placeholder = متغير غير صالح
i18n.parse.unclosed_literal = نص بين علامتي تنصيص لم يُغلق
i18n.parse.missing_default_variant = لا توجد صيغة افتراضية (معلّمة بنجمة)
i18n.parse.multiple_default_variants = أكثر من صيغة افتراضية
i18n.parse.duplicate_variant = صيغة مكررة
i18n.parse.invalid_variant = صيغة غير صالحة
i18n.parse.orphan_continuation = سطر تكملة بلا مفتاح قبله
i18n.parse.nesting_too_deep = تداخل الصيغ أعمق من المسموح
i18n.parse.invalid_namespace = اسم النطاق غير صالح
i18n.number.empty = لم يُدخل أي رقم
i18n.number.invalid = ليس عدداً صحيحاً
i18n.number.overflow = العدد أكبر من المسموح
i18n.date.invalid_month = الشهر غير صحيح
i18n.date.invalid_day = اليوم غير صحيح
i18n.date.before_hijri_epoch = التاريخ قبل بداية التقويم الهجري
i18n.date.year_out_of_range = السنة خارج النطاق المدعوم
i18n.error.unknown_lang = لغة غير مدعومة
