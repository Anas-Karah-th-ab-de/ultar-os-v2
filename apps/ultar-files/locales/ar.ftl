# ultar-files – نصوص تطبيق الملفات (العربية هي المرجع)
app.files.name = الملفات
app.files.desc = تصفّح مجلداتك وملفاتك وسلة المحذوفات وذاكرات USB.
files.places = الأماكن
files.place.home = المجلد الرئيسي
files.place.documents = المستندات
files.place.downloads = التنزيلات
files.place.pictures = الصور
files.place.music = الموسيقى
files.place.videos = الفيديو
files.place.desktop = سطح المكتب
files.place.trash = سلة المحذوفات
files.place.drive = ذاكرة {name}
files.eject = إخراج ذاكرة {name}
files.back = رجوع
files.forward = إلى الأمام
files.search = ابحث في {place}
files.view_mode = طريقة العرض
files.items = العناصر
files.desk.name = سطح المكتب
files.count = {count ->
    [zero] لا عناصر
    [one] عنصر واحد
    [two] عنصران
    [few] {count} عناصر
    [many] {count} عنصرًا
   *[other] {count} عنصر
}
files.new_folder_name = مجلد جديد
files.cmd.open = فتح
files.cmd.copy = نسخ
files.cmd.cut = قص
files.cmd.paste = لصق
files.cmd.rename = إعادة التسمية
files.cmd.trash = نقل إلى سلة المحذوفات
files.cmd.properties = الخصائص
files.cmd.restore = استعادة
files.cmd.purge = حذف نهائي
files.cmd.empty_trash = إفراغ السلة
files.cmd.new_folder = مجلد جديد
files.cmd.select_all = تحديد الكل
files.cmd.undo = تراجع
files.cmd.eject = إخراج
files.col.name = الاسم
files.col.size = الحجم
files.col.modified = آخر تعديل
files.col.kind = النوع
files.kind.folder = مجلد
files.kind.text = مستند
files.kind.sheet = جدول بيانات
files.kind.image = صورة
files.kind.audio = ملف صوتي
files.kind.video = فيديو
files.kind.archive = أرشيف
files.kind.web = صفحة ويب
files.kind.link = اختصار
files.kind.other = ملف
files.empty.folder = هذا المجلد فارغ
files.empty.search = لا نتائج لهذا البحث
files.empty.trash = سلة المحذوفات فارغة
files.cancel = إلغاء
files.close = إغلاق
files.rename.title = إعادة تسمية «{name}»
files.rename.field = الاسم الجديد
files.empty_trash.title = إفراغ سلة المحذوفات؟
files.empty_trash.message = {count ->
    [zero] لا شيء في السلة.
    [one] سيُحذف عنصر واحد نهائيًا ولا يمكن استعادته.
    [two] سيُحذف عنصران نهائيًا ولا يمكن استعادتهما.
    [few] ستُحذف {count} عناصر نهائيًا ولا يمكن استعادتها.
    [many] سيُحذف {count} عنصرًا نهائيًا ولا يمكن استعادتها.
   *[other] سيُحذف {count} عنصر نهائيًا ولا يمكن استعادتها.
}
files.purge.title = {count ->
    [one] حذف العنصر نهائيًا؟
    [two] حذف العنصرين نهائيًا؟
   *[other] حذف {count} عناصر نهائيًا؟
}
files.purge.message = لا يمكن التراجع عن الحذف النهائي.
files.properties.title = خصائص «{name}»
files.prop.kind = النوع
files.prop.size = الحجم
files.prop.modified = آخر تعديل
files.prop.place = المكان
files.prop.mode = الأذونات
files.prop.origin = المكان الأصلي
files.conflict.title = يوجد عنصر باسم «{name}» من قبل
files.conflict.message = اختر ما يحدث للعناصر التي لها الاسم نفسه.
files.conflict.skip = تخطٍّ
files.conflict.keep_both = الاحتفاظ بالاثنين
files.conflict.replace = استبدال
files.busy.title = الذاكرة قيد الاستخدام
files.busy.message = ما زال تطبيق يستخدم ملفات على هذه الذاكرة. أغلق الملفات ثم حاول مرة أخرى.
files.busy.retry = حاول مرة أخرى
files.job.progress = تقدّم العملية
files.job.cancel = إيقاف
files.notice.copied = {count ->
    [one] نُسخ «{name}»
    [two] نُسخ عنصران
   *[other] نُسخت {count} عناصر
}
files.notice.moved = {count ->
    [one] نُقل «{name}»
    [two] نُقل عنصران
   *[other] نُقلت {count} عناصر
}
files.notice.trashed = {count ->
    [one] نُقل «{name}» إلى سلة المحذوفات
    [two] نُقل عنصران إلى سلة المحذوفات
   *[other] نُقلت {count} عناصر إلى سلة المحذوفات
}
files.notice.restored = {count ->
    [one] استُعيد «{name}»
   *[other] استُعيدت {count} عناصر
}
files.notice.purged = حُذفت العناصر نهائيًا
files.notice.emptied = أُفرغت سلة المحذوفات
files.notice.undone = تم التراجع
files.notice.cancelled = أُوقفت العملية
files.notice.ejected = يمكنك الآن نزع ذاكرة {name} بأمان
files.notice.no_app = لا يوجد تطبيق لفتح «{name}»
files.error.not_found = العنصر غير موجود
files.error.exists = يوجد عنصر بهذا الاسم
files.error.permission = لا تملك إذنًا لهذا المكان
files.error.link = هذا اختصار ولا يُفتح من هنا
files.error.not_folder = هذا ليس مجلدًا
files.error.no_space = لا توجد مساحة كافية
files.error.not_empty = المجلد ليس فارغًا
files.error.bad_name = اسم غير صالح
files.error.no_area = هذا المكان غير متاح للتطبيق
files.error.into_itself = لا يمكن وضع مجلد داخل نفسه
files.error.too_deep = المجلد عميق أو كبير جدًا
files.error.bad_record = سجل سلة المحذوفات تالف
files.error.busy = العنصر قيد الاستخدام
files.error.cancelled = أُوقفت العملية
files.error.io = تعذّرت القراءة أو الكتابة
files.error.service = الخدمة غير متاحة الآن
files.error.refused = رُفض الطلب
files.error.folder_hint = اختر مكانًا آخر من الشريط الجانبي.
files.hint.permission = النظام يمنع هذا التطبيق من الكتابة أو القراءة هنا. اطلب الإذن من مدير الجهاز أو اختر مكانًا آخر.
files.hint.no_space = احذف ملفات لا تحتاجها أو أفرغ سلة المحذوفات ثم حاول مرة أخرى.
files.hint.link = الاختصارات لا تُتبع حفاظًا على أمان ملفاتك.
files.hint.no_area = لم يُمنح التطبيق الوصول إلى هذا المكان.
files.hint.name = اختر اسمًا آخر بلا شرطة مائلة وبطول معقول.
files.hint.retry = حاول مرة أخرى بعد قليل.
files.thumb.unsupported = لا معاينة لهذا النوع
files.thumb.too_large = الصورة أكبر من أن تُعايَن
files.thumb.damaged = الصورة تالفة
