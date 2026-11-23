# ultar-update – العربية (المرجع)

update.state.idle = النظام محدَّث
update.state.checking = جارٍ البحث عن تحديثات…
update.state.available = يتوفّر تحديث للنظام
update.state.downloading = جارٍ تنزيل التحديث…
update.state.paused = توقّف التنزيل مؤقتًا ويمكن متابعته
update.state.staged = التحديث جاهز للتثبيت
update.state.applying = جارٍ تثبيت التحديث…
update.state.reboot_required = أعد تشغيل الجهاز لإتمام التحديث
update.state.verifying = جارٍ التحقق من النسخة الجديدة بعد إعادة التشغيل…

update.error.io = تعذّر القراءة من وسيط التخزين أو الكتابة عليه
update.error.malformed = ملف التحديث تالف أو ليس ملف تحديث لنظام ألتار
update.error.no_trusted_keys = لا توجد على الجهاز مفاتيح موثوقة للتحقق من التحديثات
update.error.unknown_key = التحديث موقَّع بمفتاح غير موثوق على هذا الجهاز
update.error.bad_signature = توقيع التحديث غير صحيح؛ لم يُثبَّت شيء
update.error.expired = انتهت صلاحية هذا التحديث؛ ابحث عن تحديث أحدث
update.error.wrong_arch = هذا التحديث مُعدّ لنوع معالج آخر
update.error.wrong_edition = هذا التحديث مُعدّ لإصدار آخر من النظام
update.error.wrong_device = هذا التحديث غير مخصَّص لهذا الجهاز
update.error.wrong_channel = هذا التحديث من قناة غير القناة المختارة
update.error.payload_set = محتويات التحديث لا تطابق تقسيم هذا الجهاز
update.error.rollback_refused = رُفض التحديث لأنه أقدم من مستوى الأمان الحالي للجهاز
update.error.needs_intermediate = يلزم تثبيت تحديث وسيط قبل هذا التحديث
update.error.up_to_date = هذه النسخة مثبَّتة بالفعل
update.error.downgrade_refused = هذه النسخة أقدم من النسخة العاملة
update.error.hash_mismatch = البيانات المنزَّلة لا تطابق التحديث الموقَّع؛ حُذفت وسيُعاد تنزيلها
update.error.read_back = لم تُحفظ البيانات على وسيط التخزين كما كُتبت؛ بقي النظام الحالي كما هو
update.error.size_exceeded = حجم ملف التحديث أكبر من الحجم الموقَّع؛ رُفض الملف
update.error.truncated = انقطع التنزيل قبل اكتماله ويمكن متابعته
update.error.rotation_refused = تعذّر قبول مفاتيح التحديث الجديدة
update.error.state_corrupt = بيانات الحماية المحفوظة على الجهاز تالفة؛ أُوقفت التحديثات حفاظًا على الأمان
update.error.no_space = لا توجد مساحة كافية لتنزيل التحديث
update.error.not_staged = لا يوجد تحديث منزَّل جاهز للتثبيت
update.error.busy = توجد عملية تحديث جارية
update.error.cancelled = أُلغيت العملية
update.error.interrupted = انقطعت العملية
update.error.no_bootable_slot = لا توجد نسخة نظام قابلة للتشغيل
update.error.boot_control = تعذّرت قراءة بيانات الإقلاع
update.error.previous_unavailable = لا توجد نسخة سابقة يمكن تشغيلها
update.error.no_update = لا يوجد تحديث لهذا الجهاز
update.error.metered = الاتصال الحالي محدود البيانات؛ اسمح بالتنزيل عبره أو اتصل بشبكة أخرى
update.error.permission_denied = هذا الإجراء يتطلب صلاحيات المسؤول
update.error.auth_required = هذه القناة تتطلب تسجيل الدخول
update.error.not_entitled = حسابك لا يملك حق الوصول إلى هذه القناة
update.error.unknown_channel = قناة التحديث غير موجودة على الخادم
update.error.rate_limited = الخادم مشغول الآن؛ حاول لاحقًا
update.error.network = تعذّر الاتصال بخادم التحديثات
update.error.unhealthy = لم تعمل النسخة الجديدة كما يجب؛ أعد التشغيل للعودة إلى النسخة السابقة
update.error.invalid_argument = قيمة غير صالحة
update.error.boot_failed = لم تبدأ النسخة الجديدة؛ عاد الجهاز تلقائيًا إلى النسخة السابقة

update.history.staged = نُزّل التحديث وتم التحقق منه
update.history.applied = ثُبّت التحديث وينتظر إعادة التشغيل
update.history.installed = اكتمل تحديث النظام
update.history.rolled_back = عاد النظام إلى النسخة السابقة
update.history.failed = فشلت عملية التحديث
update.history.previous_selected = اختير تشغيل النسخة السابقة
update.history.cancelled = أُلغي التحديث
update.history.keys_rotated = حُدّثت مفاتيح التحقق من التحديثات

update.notes.generic = تحسينات وإصلاحات للنظام

update.cli.usage = الاستخدام: uupdate [--socket مسار] [--lang ar|en|de] [--no-wait] الأمر
        الأوامر: status · check · download [--metered] · apply · cancel · history [عدد]
        policy · policy set مفتاح=قيمة… · offline مسار [--allow-downgrade] · previous
        verify ملف.uup --keys مجلد [--state مجلد]
update.cli.current = النسخة العاملة: {version} (الخانة {slot})
update.cli.channel = القناة: {channel}
update.cli.pending = النسخة المنتظرة: {version}
update.cli.previous = النسخة السابقة: {version} (يمكن تشغيلها)
update.cli.previous_none = لا توجد نسخة سابقة قابلة للتشغيل
update.cli.floor = مستوى الأمان: {index}
update.cli.critical = تحديث أمني مهم
update.cli.progress = {percent}٪
update.cli.accepted = قُبل الطلب
update.cli.no_bus = تعذّر الاتصال بخدمة التحديث
update.cli.timeout = لم تردّ خدمة التحديث في الوقت المحدد
update.cli.verify_ok = الملف سليم وموقَّع: النسخة {version}
update.cli.history_empty = السجل فارغ
update.cli.history_line = {time} · {what} · {version}
update.cli.policy.automatic = التحديث التلقائي: {value}
update.cli.policy.window = النافذة الزمنية (بالتوقيت العالمي): {start}–{end}
update.cli.policy.window_any = النافذة الزمنية: في أي وقت
update.cli.policy.metered = التنزيل عبر اتصال محدود: {value}
update.cli.policy.rate = حد السرعة: {rate} ك.بايت/ث
update.cli.policy.rate_unlimited = حد السرعة: بلا حد
update.cli.yes = نعم
update.cli.no = لا

update.daemon.usage = الاستخدام: ultar-updated --config ملف [--socket مسار] [--lang ar|en|de]
update.daemon.config = ملف الإعداد غير صالح (السطر {line}): {detail}
update.daemon.bus = تعذّر الاتصال بناقل النظام
update.daemon.ready = خدمة التحديث جاهزة

update.sign.usage = الاستخدام: uupdate-sign الأمر [خيارات]
        keygen --secret ملف --public ملف
        manifest --out ملف --version س.ص.ع --rollback-index رقم --channel قناة --edition إصدار --arch معمارية
                 --devices قائمة --min-version س.ص.ع --notes-key مفتاح --created ثوانٍ --expires ثوانٍ [--critical]
                 --payload دور=ملف[:جذر-verity]…
        sign --manifest ملف --key ملف-سري --out ملف
        rotate --serial رقم --created ثوانٍ --key ملف-سري --out ملف --public ملف…
        bundle --signed ملف --out ملف.uup [--rotation ملف]… --payload دور=ملف…
        verify --bundle ملف.uup --keys مجلد
update.sign.wrote = كُتب الملف: {path}
update.sign.verified = التوقيع صحيح: النسخة {version}، مؤشر التراجع {index}
update.sign.key_id = معرّف المفتاح: {id}
update.sign.failed = فشلت العملية: {detail}
