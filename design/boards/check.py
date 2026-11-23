# Self-checks of the generated artboards. Any failure makes the build fail.
import glob, json, os, re, sys
from html.parser import HTMLParser
import gen
from actions import ACTIONS
T = gen.TOK
SPACE = set(T['space']['values']); RADIUS = set(T['radius'].values()); SIZES = set(T['type']['sizes']); WEIGHTS = set(T['type']['weights'])
LH = {str(T['type']['line_height_latin']), str(T['type']['line_height_arabic'])}
BORDER = {T['size']['border'], 2 * T['size']['border'], T['size']['focus_ring']}
ICON = {T['size']['icon_small'], T['size']['icon']}
BASE = T['space']['values'][0]
FAMILIES = {gen.FF_TEXT, gen.FF_TITLE, gen.FF_MONO}
VOID = {'meta', 'link', 'input'}
SPACING = re.compile(r'^(padding|margin|gap|row-gap|column-gap)(-|$)')
EDGE = re.compile(r'^(border|outline)')
BOX = re.compile(r'^(width|height|min-width|min-height|max-width|max-height|top|bottom|left|right|inset|inset-inline|inset-inline-start|inset-inline-end|flex-basis|grid-template-columns)$')
PX = re.compile(r'(-?\d+(?:\.\d+)?)px'); HEX = re.compile(r'#[0-9A-Fa-f]{3,8}\b'); RGBA = re.compile(r'rgba?\([^)]*\)')
AR = re.compile(r'[؀-ۿ]'); LATD = re.compile(r'[A-Za-z0-9]'); INDIC = re.compile(r'[٠-٩۰-۹]')
BANNED_AR = [r'Wi-Fi', r'تعريف الشاشات', r'تكرار \(مرآة\)', r'التعريفات والعتاد', r'الحاسوب(?![يةً])', r'نوفمبر', r'متوقف', r'الذكاء المحلي', r'بلا إعادة تثبيت', r'متصلة داخلياً', r'>كشف<', r'انتهى', r'\[الحجم\]', r'\[المساحة\]', r'\[size\]', r'المكتب والوسائط', r'إيقاف التشغيل(?! الطارئ)']

class P(HTMLParser):
    def __init__(self, name, theme):
        super().__init__(convert_charrefs=True)
        self.name = name; self.th = theme; self.err = []; self.stack = []; self.ids = {}; self.bdi = 0; self.btn = None; self.clicks = 0; self.in_style = False; self.in_script = False
        self.colors = {v.lower() for k, v in vars(theme).items() if isinstance(v, str) and v.startswith('#')}
        self.focusables = []
        st = T['state']
        self.overlays = {gen.rgba(c, st[k]) for c in self.colors for k in ('hover_overlay', 'pressed_overlay')}
    def e(self, msg): self.err.append(msg)
    def style(self, tag, css):
        for decl in css.split(';'):
            if ':' not in decl: continue
            prop, val = decl.split(':', 1); prop = prop.strip(); val = val.strip()
            for h in HEX.findall(val):
                if h.lower() not in self.colors: self.e('colour %s not a token of theme %s (%s)' % (h, self.th.name, prop))
            for r in RGBA.findall(val):
                if r != self.th.scrim and r not in self.th.shadow and r not in self.overlays: self.e('rgba %s is neither the scrim, the shadow nor a state overlay' % r)
            if prop == 'box-shadow':
                if val != self.th.shadow: self.e('shadow "%s" is not the one elevation' % val)
                continue
            if prop == 'opacity' and val != gen.DIM.split(': ')[1]: self.e('opacity %s is not state.disabled_opacity' % val)
            if prop == 'transform' and val != 'scaleX(-1)': self.e('transform %s' % val)
            if prop == 'font-weight' and int(val) not in WEIGHTS: self.e('font-weight %s' % val)
            if prop == 'line-height' and val not in LH: self.e('line-height %s' % val)
            if prop == 'font-family' and val not in FAMILIES: self.e('font-family %s' % val)
            for n in PX.findall(val):
                n = float(n)
                if prop == 'font-size': ok = n in SIZES
                elif prop == 'border-radius': ok = n in RADIUS
                elif SPACING.match(prop): ok = n in SPACE
                elif EDGE.match(prop): ok = n in BORDER
                elif BOX.match(prop): ok = n in BORDER or (n >= 0 and n % BASE == 0)
                else: ok = False
                if not ok: self.e('%s: %gpx is not derivable from the tokens (<%s>)' % (prop, n, tag))
    def handle_starttag(self, tag, attrs):
        a = dict(attrs)
        if tag not in VOID: self.stack.append(tag)
        if tag == 'style': self.in_style = True
        if tag == 'script': self.in_script = True
        if tag == 'bdi' or tag == 'kbd': self.bdi += 1
        if 'id' in a:
            if a['id'] in self.ids: self.e('duplicate id %s' % a['id'])
            self.ids[a['id']] = 1
        for ref in ('aria-labelledby', 'aria-describedby', 'for'):
            if ref in a: self.focusables.append(('ref', a[ref]))
        if 'style' in a: self.style(tag, a['style'])
        if tag == 'button' and 'style' in a:
            for dim in re.findall(r'(?:^|; )(?:min-)?(?:height|width): (\d+)px', a['style']):
                if int(dim) < T['size']['target_pointer_min']: self.e('button target %spx is below the pointer minimum' % dim)
        if tag == 'svg':
            w, h = int(a['width']), int(a['height'])
            if 'data-illus' in a:
                if w % BASE or h % BASE: self.e('illustration size %dx%d off the grid' % (w, h))
            elif w not in ICON or h != w: self.e('icon size %dx%d (allowed %s)' % (w, h, sorted(ICON)))
            if a.get('stroke-width') != str(T['size']['stroke']): self.e('stroke width %s' % a.get('stroke-width'))
            for k in ('stroke', 'fill'):
                v = a.get(k, 'none')
                if v not in ('none', 'currentColor') and v.lower() not in self.colors: self.e('svg %s %s' % (k, v))
            if a.get('aria-hidden') != 'true': self.e('svg without aria-hidden')
            n = a.get('data-icon'); flipped = 'scaleX(-1)' in a.get('style', '')
            if n in gen.ICONS and flipped != (gen.ICONS[n]['mirror'] and self.rtl): self.e('icon %s mirrored wrongly for this direction' % n)
            if n not in gen.ICONS and flipped: self.e('pictogram or mark must never be mirrored')
        clickable = tag in ('button', 'a', 'input') or a.get('role') == 'slider'
        if clickable:
            self.clicks += 1
            if a.get('data-act') not in ACTIONS: self.e('<%s> without a defined action (%s)' % (tag, a.get('aria-label') or a.get('id')))
        elif 'data-act' in a: self.e('data-act on a non-interactive <%s>' % tag)
        if tag == 'button':
            if self.btn is not None: self.e('button inside button')
            self.btn = dict(label=a.get('aria-label', ''), text='')
        if tag == 'input' and not (a.get('aria-label') or a.get('id')): self.e('input without a name')
        if tag == 'input' and a.get('id'): self.focusables.append(('input', a['id']))
        if tag == 'label': self.focusables.append(('label', a.get('for')))
        if a.get('role') == 'slider' and not a.get('aria-label'): self.e('slider without a name')
        if a.get('role') in ('switch', 'checkbox', 'radio') and 'aria-checked' not in a: self.e('%s without aria-checked' % a['role'])
        if a.get('role') == 'switch' and not a.get('aria-label'): self.e('switch without aria-label')
    def handle_startendtag(self, tag, attrs):
        self.handle_starttag(tag, attrs)
        if tag not in VOID: self.handle_endtag(tag)
    def handle_endtag(self, tag):
        if tag in VOID: return
        if not self.stack or self.stack[-1] != tag: self.e('closing </%s> but open is <%s>' % (tag, self.stack[-1] if self.stack else None)); return
        self.stack.pop()
        if tag == 'style': self.in_style = False
        if tag == 'script': self.in_script = False
        if tag in ('bdi', 'kbd'): self.bdi -= 1
        if tag == 'button':
            if not (self.btn['label'].strip() or self.btn['text'].strip()): self.e('button without text or aria-label')
            self.btn = None
    def handle_data(self, data):
        if self.in_script: return
        if self.in_style:
            if PX.search(data): self.e('px value in <style>')
            for h in HEX.findall(data):
                if h.lower() not in self.colors: self.e('colour %s in <style>' % h)
            return
        if self.btn is not None: self.btn['text'] += data
        if INDIC.search(data): self.e('Arabic-Indic digit in "%s"' % data.strip())
        if self.rtl and self.bdi == 0 and AR.search(data) and LATD.search(data) and 'title' not in self.stack: self.e('Latin or digits outside <bdi> in Arabic text: "%s"' % data.strip())
        if not self.rtl and AR.search(data): self.e('Arabic text on an English board: "%s"' % data.strip())

def skeleton(s):
    """The fixed file skeleton, with everything variable removed."""
    s = re.sub(r'<div dir=.*?\n</div>\n</x-dc>', '<ROOT></x-dc>', s, flags=re.S)
    s = re.sub(r'<html lang="[a-z]+">', '<html>', s); s = re.sub(r'<title>.*?</title>', '<title>', s); s = re.sub(r'<link [^>]*>', '<link>', s)
    s = re.sub(r'<style>.*?</style>', '<style>', s, flags=re.S); s = re.sub(r'"width":\d+,"height":\d+', 'WH', s)
    return s
def _lum(h):
    v = [int(h[i:i + 2], 16) / 255 for i in (1, 3, 5)]
    v = [x / 12.92 if x <= 0.03928 else ((x + 0.055) / 1.055) ** 2.4 for x in v]
    return 0.2126 * v[0] + 0.7152 * v[1] + 0.0722 * v[2]
def ratio(a, b):
    la, lb = sorted((_lum(a), _lum(b)), reverse=True)
    return (la + 0.05) / (lb + 0.05)
def contrast():
    """Every foreground/background pair the generator uses: text 4.5:1, graphics and control borders 3:1."""
    bad = []; worst = {}
    for th in gen.THEMES.values():
        grounds = [th.ground, th.bar, th.surface, th.raised, th.hover]
        pairs = [(fg, bg, 4.5, n) for n, fg in (('text', th.text), ('text_muted', th.mute), ('accent_text', th.acctext)) for bg in grounds]
        pairs += [(fg, bg, 4.5, n) for n, fg in (('info', th.info), ('success', th.ok), ('warning', th.warn), ('danger', th.danger)) for bg in (th.surface, th.raised)]
        pairs += [(th.onacc, th.acc, 4.5, 'on_accent/accent')]
        pairs += [(th.ctl, bg, 3.0, 'control_border') for bg in (th.ground, th.bar, th.surface, th.raised)]
        pairs += [(th.focus, bg, 3.0, 'focus ring') for bg in grounds]
        pairs += [(th.acctext, bg, 3.0, 'accent_text graphic') for bg in grounds + [th.line]]
        for fg, bg, need, n in pairs:
            r = ratio(fg, bg); worst[(th.name, n)] = min(worst.get((th.name, n), 99), r)
            if r < need: bad.append('contrast %s %s on %s = %.2f (< %s) in theme %s' % (n, fg, bg, r, need, th.name))
    return bad, worst
def run(layout):
    fails = 0; report = []
    bad, worst = contrast(); fails += len(bad); report += bad
    ref = skeleton(open('/home/claude/design/project/Main.dc.html', encoding='utf-8').read())
    files = sorted(glob.glob(os.path.join(gen.ROOT, '*.dc.html')))
    names = {os.path.basename(f) for f in files}
    total = 0
    for f in files:
        name = os.path.basename(f); b = gen.boards[name]; s = open(f, encoding='utf-8').read()
        p = P(name, gen.THEMES[b['theme']]); p.rtl = b['rtl']; p.feed(s); p.close()
        if p.stack: p.e('unclosed tags: %s' % p.stack)
        ids = set(p.ids)
        for kind, i in p.focusables:
            if kind in ('ref', 'label') and i not in ids: p.e('reference to missing id %s' % i)
        if '{{' in s: p.e('"{{" in file')
        if skeleton(s) != ref: p.e('file skeleton differs from the round-1 skeleton')
        root = re.search(r'<div dir="(rtl|ltr)" style="width: (\d+)px; height: (\d+)px; box-sizing: border-box; position: relative; overflow: hidden;', s)
        if not root or (int(root.group(2)), int(root.group(3))) != (b['w'], b['h']) or (root.group(1) == 'rtl') != b['rtl']: p.e('root div does not match the board size or direction')
        if '"$preview":{"width":%d,"height":%d}' % (b['w'], b['h']) not in s: p.e('data-props size differs')
        if b['rtl']:
            for w in BANNED_AR:
                if re.search(w, s): p.e('banned wording: %s' % w)
            if name.startswith('Installer') and 'رجوع' in s: p.e('installer uses رجوع instead of السابق')
        if p.clicks != len(gen.CLICKS[name]): p.e('click register (%d) and HTML (%d) differ' % (len(gen.CLICKS[name]), p.clicks))
        total += p.clicks
        for m in sorted(set(p.err)): report.append('%s: %s' % (name, m))
        fails += len(set(p.err))
    # sources: no literal visual value in the board scripts
    for f in sorted(glob.glob(os.path.join(gen.HERE, 'b[0-9]*.py'))):
        src = open(f, encoding='utf-8').read()
        for pat, what in ((r'\d\s*px', 'px literal'), (r'#[0-9A-Fa-f]{6}\b', 'hex colour'), (r'rgba?\(', 'rgb()'), (r'opacity', 'opacity'), (r'font-size: \d', 'font size literal'), (r'(?<![\w-])\d+(\.\d+)?(rem|em|pt)\b', 'unit literal')):
            for m in re.finditer(pat, src):
                fails += 1; report.append('%s: %s in source: %s' % (os.path.basename(f), what, src[max(0, m.start() - 20):m.end() + 5].replace('\n', ' ')))
    # layout: keys = files, no overlap, 80 in a row, 120 between rows
    if set(layout) != names: fails += 1; report.append('layout keys differ from the board files: %s' % sorted(set(layout) ^ names))
    L = list(layout.items())
    for i, (a, A) in enumerate(L):
        for bname, B in L[i + 1:]:
            if A['x'] < B['x'] + B['w'] and B['x'] < A['x'] + A['w'] and A['y'] < B['y'] + B['h'] and B['y'] < A['y'] + A['h']:
                fails += 1; report.append('layout overlap: %s / %s' % (a, bname))
    rows = {}
    for n, B in layout.items(): rows.setdefault(B['y'], []).append(B)
    ys = sorted(rows)
    for y in ys:
        r = sorted(rows[y], key=lambda B: B['x'])
        for A, B in zip(r, r[1:]):
            if B['x'] - (A['x'] + A['w']) != 80: fails += 1; report.append('row gap is not 80 at y=%d' % y)
    for y1, y2 in zip(ys, ys[1:]):
        if y2 - (y1 + max(B['h'] for B in rows[y1])) != 120: fails += 1; report.append('row distance is not 120 after y=%d' % y1)
    for m in report: print('FAIL', m)
    print('checks: contrast, targets, icon mirroring by direction, token scan, tag stack, duplicate ids, button names, actions, "{{", skeleton, Arabic islands/digits/wording, source literals, layout  ->  %d boards, %d clickable elements, %d failures' % (len(files), total, fails))
    return 1 if fails else 0
