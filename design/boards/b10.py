# Assistant · cloud model as the last way: AssistantCloud (dark Arabic, light English) and AssistantCloudKey
# (the key is typed hidden). Only bricks of gen.py, only tokens.
# Contract: system/core/ultar-bus/src/messages/llm_cloud.rs · notes: docs/integration/ultar-llm-cloud.md
from gen import *
from b3 import settings
W, H = 360, 225

def order(n, title, text, now=False):
    mark = badge(L('يُستعمل الآن', 'In use now'), 'ok', 'check') if now else ''
    return row(f'{n} · ' + title, text, mark, last=(n == 4))

def cloud_page(stored=True):
    how = card(cardhead(L('بماذا يفكّر المساعد', 'What the helper thinks with'), L('بهذا الترتيب. ما على أجهزتك يأتي أولاً دائماً.', 'In this order. What is on your own devices always comes first.'))
        + order(1, L('نموذج هذا الجهاز', 'The model of this device'), L('لا نموذج على هذا الجهاز', 'No model on this device'))
        + order(2, L('نموذج جهاز مقترن يعيره', 'A model a paired device lends'), L('لا جهاز مقترن يعير نموذجه الآن', 'No paired device lends its model now'))
        + order(3, L('النموذج السحابي', 'The cloud model'), L('فقط إن غاب الاثنان وأدخلت مفتاحك', 'Only when both are missing and you entered your key'), now=stored)
        + order(4, L('أوامر ثابتة ومواضيع المساعدة', 'Fixed commands and help topics'), L('دون أي نموذج', 'Without any model'), now=not stored))
    provider = row(L('المزوّد', 'Provider'), L('Claude أو خادم تختاره بواجهة chat-completions', 'Claude, or a server of your choice with the chat-completions interface'), select('Claude', L('المزوّد', 'Provider'), 'assistant.cloud.provider'), icon='cloud')
    state = L('محفوظ · قبِله المزوّد', 'Stored · the provider accepted it') if stored else L('لا مفتاح', 'No key')
    key = row(L('المفتاح', 'Key'), state, button(L('استبدال المفتاح', 'Replace key') if stored else L('إدخال المفتاح', 'Enter key'), 'assistant.cloud.key'), icon='lock')
    model = row(L('النموذج', 'Model'), L('النماذج التي يسمح بها مفتاحك. النماذج الأكبر أغلى لكل طلب.', 'Models your key may use. Bigger models cost more per request.'), select('Claude Haiku 5.5', L('النموذج', 'Model'), 'assistant.cloud.model')) if stored else ''
    forget = button(L('نسيان المفتاح', 'Forget key'), 'assistant.cloud.clear', 'danger') if stored else ''
    leaves = row(L('يُرسَل طلبك وقائمة أدوات المساعد إلى المزوّد', "Your request and the list of the helper's tools are sent to the provider"),
                 L('يحاسبك المزوّد على كل طلب عبر حسابك؛ هذا النظام لا يضيف شيئاً ولا يرى الفاتورة.', 'The provider bills your account for every request; this system adds nothing and sees no bill.'), forget, last=True, icon='info')
    cloud = card(cardhead(L('النموذج السحابي، كخيار أخير', 'Cloud model, as the last way'), L('بمفتاحك الخاص وعلى حسابك. دون مفتاح لا يغادر الجهازَ شيء.', 'With your own key, at your own cost. Without a key nothing leaves the device.'))
        + provider + key + model + leaves)
    return h1(L('المساعد', 'Assistant')) + how + cloud

begin('AssistantCloud.dc.html')
out('Ultar OS – المساعد: النموذج السحابي', W, H, settings('spark', cloud_page()), 'Einstellungen · Assistent: Cloud-Modell als letzter Weg (Reihenfolge, Anbieter, Schlüssel, Modell, Kosten) · dunkel · Arabisch', 10)
begin('AssistantCloudLight.dc.html', 'light', False)
out('Ultar OS – Assistant: cloud model', W, H, settings('spark', cloud_page(False)), 'Einstellungen · Assistent: Cloud-Modell, noch kein Schlüssel · hell · Englisch', 10)

begin('AssistantCloudKey.dc.html')
entry = f'<div style="padding: {s2} 0">{field("المفتاح", "assistant.cloud.key.field", value="••••••••••••••••", hint="من حسابك لدى المزوّد", kind="password", direction="ltr", error="رفض المزوّد هذا المفتاح. لم يُحفظ شيء.")}</div>'
key = dialog('مفتاح النموذج السحابي', 'الصق المفتاح من حسابك لدى المزوّد. يُسأل المزوّد مرة واحدة للتحقق. يبقى المفتاح على هذا الجهاز مختوماً ولا يُعرض مرة أخرى.',
             button('إلغاء', 'assistant.cloud.key.cancel') + button('حفظ', 'assistant.cloud.key.save', 'primary'), live=entry)
out('Ultar OS – المساعد: إدخال المفتاح', W, H, settings('spark', cloud_page(False), overlay=key), 'Assistent · Cloud-Modell: Schlüssel verdeckt eingeben (Fehlerzustand: vom Anbieter abgelehnt)', 10)
