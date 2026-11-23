# ultar-cloud – رسائل المتجر والتحديثات والحساب. العربية هي المرجع.

# المتجر
cloud.store.sign_in_required = سجّل الدخول للحصول على هذا التطبيق.
cloud.store.not_included = هذا التطبيق غير مشمول في صلاحية وصولك.
cloud.store.not_found = هذا التطبيق غير موجود في المتجر.
cloud.store.state.free = مجاني
cloud.store.state.premium_sign_in = مميز – يلزم تسجيل الدخول
cloud.store.state.premium_not_included = مميز – غير مشمول في وصولك
cloud.store.state.unlocked = متاح لك
cloud.store.publisher.verified = ناشر موثّق
cloud.store.empty = لا توجد تطبيقات مطابقة.
cloud.store.installed = تم تثبيت {name} بالإصدار {version}.

# تحديثات النظام
cloud.update.sign_in_required = سجّل الدخول لاستخدام قناة التحديث هذه.
cloud.update.not_included = قناة التحديث هذه غير مشمولة في صلاحية وصولك.
cloud.update.not_found = قناة التحديث هذه غير موجودة.
cloud.update.available = يتوفر تحديث: الإصدار {version} (القناة {channel}).
cloud.update.none = النظام محدّث، لا يوجد تحديث جديد.

# أخطاء عامة
cloud.error.auth_required = يلزم تسجيل الدخول لهذه العملية.
cloud.error.not_entitled = حسابك لا يملك صلاحية الوصول إلى هذا.
cloud.error.not_found = العنصر المطلوب غير موجود.
cloud.error.rate_limited = {seconds ->
    [zero] طلبات كثيرة جدًا. حاول مرة أخرى الآن.
    [one] طلبات كثيرة جدًا. حاول مرة أخرى بعد ثانية واحدة.
    [two] طلبات كثيرة جدًا. حاول مرة أخرى بعد ثانيتين.
    [few] طلبات كثيرة جدًا. حاول مرة أخرى بعد {seconds} ثوانٍ.
    [many] طلبات كثيرة جدًا. حاول مرة أخرى بعد {seconds} ثانية.
   *[other] طلبات كثيرة جدًا. حاول مرة أخرى بعد {seconds} ثانية.
}
cloud.error.invalid_request = رفض الخادم الطلب لأنه غير صالح.
cloud.error.invalid_credentials = اسم المستخدم أو كلمة المرور غير صحيحة.
cloud.error.username_taken = اسم المستخدم هذا مستخدم من قبل.
cloud.error.server = حدث خطأ في الخادم. حاول مرة أخرى لاحقًا.
cloud.error.offline = تعذّر الوصول إلى الخادم. تحقق من اتصالك بالإنترنت.
cloud.error.invalid_response = ردّ الخادم غير مفهوم.
cloud.error.io = تعذّرت الكتابة على وحدة التخزين.
cloud.error.integrity = الملف الذي تم تنزيله تالف أو غير مكتمل. حاول مرة أخرى.
cloud.error.package_rejected = رُفضت الحزمة: التوقيع مفقود أو غير موثوق. لم يُثبَّت شيء.
cloud.error.not_signed_in = لم تسجّل الدخول على هذا الجهاز.
cloud.error.vpn = تعذّر تجهيز مفاتيح الشبكة الخاصة.

# هوية الجهاز
cloud.device.challenge_rejected = لم يقبل الخادم فحص الأمان. حاول مرة أخرى.
cloud.device.signature_rejected = لم يقبل الخادم إثبات هوية هذا الجهاز.
cloud.device.taken = هذا الجهاز مسجَّل من قبل في حساب آخر.
cloud.device.retired = أُزيل هذا الجهاز من الحساب. أعد ضبط هوية الجهاز لتسجيله من جديد، وسيحصل على رقم جديد.
cloud.device.limit = بلغ حسابك الحد الأقصى لعدد الأجهزة. أزِل جهازًا أولًا.
cloud.device.unknown = الخادم لا يعرف هذا الجهاز.
cloud.device.update_required = حدِّث النظام على هذا الجهاز قبل تسجيله.
cloud.device.legacy_not_eligible = تعذّر على هذا الجهاز استلام التسجيل السابق.
cloud.device.cooldown_active = {seconds ->
    [zero] يمكن نقل الرقم الآن.
    [one] يمكن نقل الرقم بعد ثانية واحدة.
    [two] يمكن نقل الرقم بعد ثانيتين.
    [few] يمكن نقل الرقم بعد {seconds} ثوانٍ.
    [many] يمكن نقل الرقم بعد {seconds} ثانية.
   *[other] يمكن نقل الرقم بعد {seconds} ثانية.
}
cloud.device.recovery_pending = يوجد طلب استبدال جهاز قيد الانتظار لهذا الحساب.
cloud.device.recovery_unknown = لا يوجد طلب استبدال جهاز قيد الانتظار.
cloud.device.server_mismatch = فحص الأمان يخصّ خادمًا آخر أو حسابًا آخر. لم يُوقَّع أي شيء.
cloud.device.key_unusable = تعذّر استخدام مفتاح الجهاز. لم يُغيَّر المفتاح.
cloud.device.not_registered = هذا الجهاز غير مسجَّل بعد.
cloud.device.state.active = مسجَّل
cloud.device.state.legacy = مسجَّل قبل اعتماد مفاتيح الأجهزة – حدِّث هذا الجهاز
cloud.device.this_device = هذا الجهاز
cloud.device.none = لا توجد أجهزة مسجَّلة.
cloud.device.removed = أُزيل الجهاز.
cloud.device.renamed = تم تغيير اسم الجهاز.
cloud.device.reset = حُذفت هوية الجهاز. سجِّل الجهاز من جديد ليحصل على رقم جديد.

# سطر الأوامر
cloud.cli.signed_in = تم تسجيل الدخول.
cloud.cli.number = رقم ألتار: {number}
cloud.cli.vpn_ready = تم تجهيز الشبكة الخاصة.
cloud.cli.server = الخادم: {server}
cloud.cli.account.signed_in = الحساب: مسجّل الدخول
cloud.cli.account.signed_out = الحساب: غير مسجّل (المتجر والتحديثات يعملان بدون حساب)
cloud.cli.unknown_command = أمر غير معروف: {command}
cloud.cli.usage = الاستخدام: ucloud register|login <مستخدم> <كلمة-المرور> · number · devices · device-rename <معرّف> <اسم> · device-remove <معرّف> · device-reset · vpn · store [بحث] · app <معرّف> · install <معرّف> · update [قناة] · whoami
