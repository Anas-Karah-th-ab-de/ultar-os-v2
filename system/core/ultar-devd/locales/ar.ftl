# ultar-devd / udevctl – Arabic (reference language).
# Positional arguments of bus texts are named {a}, {b} in order.

devd.phase.detected = تم اكتشافه
devd.phase.probing = جارٍ الفحص
devd.phase.active = يعمل
devd.phase.no-driver = بلا تعريف
devd.phase.failed = تعذّر تشغيله
devd.phase.removed = أُزيل

devd.reason.no-package = لا يوجد في المستودع تعريف يناسب هذا الجهاز.
devd.reason.kernel-only = يعمل بتعريف النواة المدمج «{a}»؛ لا يحتاج إلى حزمة تعريف.
devd.reason.not-loadable = حزمة التعريف تحتاج إلى تحميل وحدة في النواة، وهذه النواة لا تدعم ذلك بعد.
devd.reason.not-bound = النواة لم تربط تعريفها «{a}» بهذا الجهاز عند الإقلاع، ولا يمكن تفعيله أثناء التشغيل.
devd.reason.package-missing = ملف حزمة التعريف غير موجود على هذا النظام.
devd.reason.rejected-detail = رُفضت حزمة التعريف: {a}
devd.reason.probe-failed = فشل تعريف النواة في تهيئة الجهاز: {a}
devd.reason.load.package-rejected = رُفضت حزمة التعريف لأن توقيعها أو محتواها غير سليم.
devd.reason.load.incompatible-kernel = التعريف لا يناسب النواة الحالية.
devd.reason.load.init-failed = فشل التعريف في تهيئة الجهاز.
devd.reason.load.timeout = انتهت المهلة أثناء تحميل التعريف.
devd.reason.load.unsupported = لا يمكن استخدام هذا التعريف مع النواة الحالية.
devd.reason.health.no-response = الجهاز لا يستجيب مع هذا التعريف.
devd.reason.health.self-test-failed = فشل الفحص الذاتي للجهاز مع هذا التعريف.
devd.reason.health.timeout = انتهت المهلة أثناء فحص الجهاز.
devd.reason.health.unstable = الجهاز يعمل بشكل متقطّع مع هذا التعريف.
devd.reason.unknown = لا يوجد تعريف يعمل لهذا الجهاز.
devd.detail.wrong-package = الملف يحتوي تعريفًا أو إصدارًا غير المسجَّل في الفهرس.
devd.detail.bad-activation-record = سجل التفعيل داخل الحزمة غير صالح.

devd.error.unknown-device = لا يوجد جهاز بالعنوان {a}.
devd.error.not-a-candidate = التعريف {b} لا يناسب الجهاز {a} أو غير موجود في المستودع.
devd.error.quarantined = التعريف {b} محجور للجهاز {a}. أزل الحجر أولًا.
devd.error.already-active = التعريف {a} هو المستخدم حاليًا.
devd.error.nothing-to-roll-back = لا يوجد تعريف سابق للجهاز {a} للرجوع إليه.
devd.error.wrong-phase = لا يمكن تنفيذ العملية والجهاز {a} في الحالة: {b}.
devd.error.unsupported = لا يمكن تبديل الجهاز {a} إلى التعريف {b}: النواة الحالية لا تتيح تفعيل تعريفاتها المدمجة أو إيقافها أثناء التشغيل.
devd.error.transition = خطأ داخلي في خدمة الأجهزة.
devd.error.bad-request = الطلب غير صالح.

devd.notice.keys-missing = لا توجد مفاتيح موثوقة، لذلك لا يمكن التحقق من أي تعريف.
devd.notice.keys-unreadable = تعذّرت قراءة ملف المفاتيح {a} وتم تجاهله.
devd.notice.depot-unreadable = تعذّر استخدام مستودع التعريفات: {a}
devd.notice.index-older = مستودع التعريفات أقدم من المستخدم حاليًا (الرقم {a}): لن يُستخدم.
devd.notice.hardware-unreadable = تعذّرت قراءة قائمة الأجهزة: {a}
devd.notice.state-recovered = ملف الحالة كان تالفًا ({a})؛ بدأت الخدمة بحالة فارغة واحتُفظ بالملف التالف.
devd.notice.state-unavailable = تعذّر حفظ الاختيارات ({a})؛ تبقى سارية حتى إيقاف التشغيل.
devd.notice.publish-failed = تعذّر نشر حالة الأجهزة على ناقل النظام؛ قد لا تعلم الخدمات الأخرى بآخر التغييرات.

devd.store.problem.header = ترويسة الملف مفقودة أو غير صالحة
devd.store.problem.length = الملف مقطوع
devd.store.problem.checksum = المجموع الاختباري لا يطابق
devd.store.problem.body = المحتوى غير صالح
devd.store.problem.version = الملف من إصدار أحدث
devd.store.problem.too-large = الملف أكبر من الحد المسموح
devd.store.unavailable = مكان التخزين غير متاح
devd.store.corrupt = المحتوى المخزَّن غير مفهوم
devd.store.write-failed = فشلت الكتابة

hal.hardware.no-source = لا النواة ولا النظام يوفّران وصفًا للأجهزة
hal.hardware.io = تعذّرت قراءة وصف الأجهزة
hal.hardware.too-many-devices = عدد الأجهزة أكبر من المعقول

devd.capability.no-driver = العتاد موجود لكن لا يوجد تعريف يعمل له.

devd.start.unknown-kernel = تعذّر تحديد النواة العاملة. استخدم ‎--kernel-abi‎.
devd.start.hardware = تعذّرت قراءة وصف النظام: {a}

devd.cli.usage = الاستخدام: udevctl [--bus مقبس] [--lang ar|en|de] list | switch <عنوان> <تعريف> <إصدار> | rollback <عنوان> | rescan [عنوان] | clear-quarantine [عنوان [تعريف إصدار]] | shutdown
devd.cli.daemon-usage = الاستخدام: ultar-devd --depot <مجلد> --keys <مجلد> --state <مجلد> --bus <مقبس> (--policy <ملف> | --allow-all) [--token-file <ملف> | --token-fd <رقم>] [--log-socket <مقبس>] [--allow-root] [--root <مجلد>] [--kernel-abi <اسم>] [--once] [--lang ar|en|de]
devd.cli.bus-failed = تعذّر الاتصال بخدمة الأجهزة: {a}
devd.cli.no-devices = لا توجد أجهزة.
devd.cli.device = {address}  {id}  [{phase}]
devd.cli.label = الوصف: {a}
devd.cli.driver = التعريف: {a} {b}
devd.cli.alternative = بديل: {a} {b}
devd.cli.alternative-quarantined = بديل محجور: {a} {b} ({c})
devd.cli.previous = السابق: {a}
devd.cli.reason = السبب: {a}
devd.cli.notice = تنبيه: {a}
devd.cli.kernel-switch-unsupported = ملاحظة: النواة الحالية لا تتيح تبديل تعريفاتها المدمجة أثناء التشغيل.
devd.cli.done = تمّ.
devd.cli.restored = فشل التعريف المطلوب وأُعيد التعريف السابق.
devd.cli.refused = رُفض الطلب: {a}
devd.cli.released = {count ->
    [zero] لا يوجد حجر لإزالته.
    [one] أُزيل حجر واحد.
    [two] أُزيل حجران.
    [few] أُزيلت {count} حالات حجر.
    [many] أُزيلت {count} حالة حجر.
   *[other] أُزيلت {count} حالة حجر.
}
devd.cli.added = ظهر: {a}
devd.cli.removed = اختفى: {a}
devd.cli.shutdown = أُوقفت خدمة الأجهزة.

devd.daemon.started = خدمة الأجهزة تعمل: {devices} جهاز، منها {active} بتعريف يعمل.
devd.daemon.stopped = أُوقفت خدمة الأجهزة.
devd.daemon.bus-lost = انقطع الاتصال بناقل النظام: {a}
devd.daemon.policy-required = يجب تحديد ملف الصلاحيات (--policy) أو --allow-all.
devd.daemon.policy-conflict = لا يجوز الجمع بين --policy و --allow-all.
devd.daemon.policy-rejected = ملف الصلاحيات غير صالح: {a}
devd.daemon.token-rejected = تعذّرت قراءة الرمز السري: {a}
devd.daemon.root-refused = لن تعمل خدمة الأجهزة بصلاحيات الجذر دون --allow-root.
devd.daemon.hardening-failed = تعذّر تطبيق قيود الحماية الذاتية: {a}

# ultar-devd-seat
seat.usage = الاستخدام: ultar-devd-seat --config ملف (--state مجلد | --once) [--root مجلد] [--log-socket مسار] [--lang ar|en|de]
seat.verdict = ملفات أجهزة مكان العمل: فُحص {nodes}، أوضاع الأجهزة: {modes}
seat.state-failed = تعذّرت كتابة ملف الحالة: {a}
seat.hardening-failed = تعذّر على البرنامج تقييد نفسه: {a}
seat.config.read = تعذّرت قراءة إعدادات مكان العمل: {a}
seat.config.syntax = إعدادات مكان العمل غير صالحة: {a}
seat.config.bound = قيمة في إعدادات مكان العمل خارج الحدود: {a}
seat.no-events = أحداث الأجهزة غير متاحة ({a})؛ لن تُعالَج الأجهزة المضافة لاحقًا.
seat.stopped = توقّف.
