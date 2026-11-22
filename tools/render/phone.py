"""KM One – realistische Produktbilder mit Blender (Cycles).
Aufruf: python3 phone.py <szene> <ausgabe.png> <breite> <höhe> <samples>
Szenen: hero, back, front, side
Maße in Dezimetern: 78 × 165 × 11,5 mm → 0,78 × 1,65 × 0,115
"""
import math
import sys

import bpy
import bmesh
from mathutils import Euler, Vector

SCENE, OUT, RW, RH, SAMPLES = sys.argv[1], sys.argv[2], int(sys.argv[3]), int(sys.argv[4]), int(sys.argv[5])
DIR = "/home/claude/render/"
W, H, T = 0.78, 1.65, 0.115

bpy.ops.wm.read_factory_settings(use_empty=True)
sc = bpy.context.scene


# ------------------------------------------------------------ Materialien
def mat(name, color, rough=0.5, metal=0.0, coat=0.0, emit=None, estr=0.0, alpha=1.0, trans=0.0, ior=1.45, noise=0.0):
    m = bpy.data.materials.new(name)
    m.use_nodes = True
    nt = m.node_tree
    p = nt.nodes["Principled BSDF"]
    p.inputs["Base Color"].default_value = (*color, 1)
    p.inputs["Roughness"].default_value = rough
    p.inputs["Metallic"].default_value = metal
    p.inputs["Coat Weight"].default_value = coat
    p.inputs["IOR"].default_value = ior
    p.inputs["Transmission Weight"].default_value = trans
    if emit:
        p.inputs["Emission Color"].default_value = (*emit, 1)
        p.inputs["Emission Strength"].default_value = estr
    if noise > 0:
        # feine Körnung (gestrahlte/gummierte Oberfläche)
        nz = nt.nodes.new("ShaderNodeTexNoise")
        nz.inputs["Scale"].default_value = 900.0
        nz.inputs["Detail"].default_value = 6.0
        bump = nt.nodes.new("ShaderNodeBump")
        bump.inputs["Strength"].default_value = noise
        bump.inputs["Distance"].default_value = 0.0004
        nt.links.new(nz.outputs["Fac"], bump.inputs["Height"])
        nt.links.new(bump.outputs["Normal"], p.inputs["Normal"])
    return m


def img_mat(name, path, emit=0.0, metal=0.0, rough=0.4, use_alpha=False, tint=None):
    m = bpy.data.materials.new(name)
    m.use_nodes = True
    nt = m.node_tree
    p = nt.nodes["Principled BSDF"]
    tex = nt.nodes.new("ShaderNodeTexImage")
    tex.image = bpy.data.images.load(path)
    tex.interpolation = "Cubic"
    nt.links.new(tex.outputs["Color"], p.inputs["Base Color"])
    p.inputs["Metallic"].default_value = metal
    p.inputs["Roughness"].default_value = rough
    if emit > 0:
        nt.links.new(tex.outputs["Color"], p.inputs["Emission Color"])
        p.inputs["Emission Strength"].default_value = emit
    if use_alpha:
        nt.links.new(tex.outputs["Alpha"], p.inputs["Alpha"])
        m.blend_method = "HASHED" if hasattr(m, "blend_method") else None
    return m


C_FRAME_D = mat("frame", (0.016, 0.017, 0.020), rough=0.42, metal=0.0, noise=0.05)
C_RUBBER_D = mat("rubber", (0.022, 0.023, 0.026), rough=0.78, noise=0.15)
C_BACK_D = mat("back", (0.030, 0.032, 0.037), rough=0.62, noise=0.25)
C_ISLAND = mat("island", (0.006, 0.006, 0.007), rough=0.18, coat=1.0)
C_GLASS = mat("lensglass", (0.0, 0.0, 0.0), rough=0.02, coat=1.0, ior=1.6)
C_LENS = mat("lens", (0.01, 0.015, 0.035), rough=0.05, metal=0.6)
C_RING = mat("ring", (0.35, 0.35, 0.36), rough=0.25, metal=1.0)
C_FLASH = mat("flash", (0.9, 0.85, 0.7), rough=0.3, emit=(1.0, 0.95, 0.8), estr=0.4)
C_SCREW = mat("screw", (0.30, 0.30, 0.31), rough=0.35, metal=1.0)
C_GOLD = mat("gold", (0.80, 0.66, 0.34), rough=0.28, metal=1.0)
C_ORANGE = mat("sos", (0.95, 0.30, 0.04), rough=0.55, noise=0.2)
C_BTN = mat("btn", (0.04, 0.042, 0.046), rough=0.4, metal=0.3)
C_DOTOFF_D = mat("dotoff", (0.012, 0.012, 0.014), rough=0.25, coat=0.5)
C_DOTON_D = mat("doton", (0.85, 0.75, 0.40), rough=0.3, emit=(1.0, 0.82, 0.38), estr=6.0)
C_HOLE = mat("hole", (0.0, 0.0, 0.0), rough=0.9)
C_TEXT = mat("text", (0.20, 0.20, 0.21), rough=0.5)
C_WIN_D = mat("window", (0.05, 0.053, 0.06), rough=0.12, coat=1.0)
C_BEZEL = mat("bezel", (0.0, 0.0, 0.0), rough=0.08, coat=1.0)


# ------------------------------------------------------------ Geometrie
def rrect_pts(w, h, r, seg=14):
    pts = []
    for cx, cy, a0 in ((w / 2 - r, h / 2 - r, 0), (-w / 2 + r, h / 2 - r, 90), (-w / 2 + r, -h / 2 + r, 180), (w / 2 - r, -h / 2 + r, 270)):
        for i in range(seg + 1):
            a = math.radians(a0 + 90 * i / seg)
            pts.append((cx + r * math.cos(a), cy + r * math.sin(a)))
    return pts


def link(obj, parent):
    bpy.context.collection.objects.link(obj)
    obj.parent = parent
    return obj


def slab(name, w, h, r, z0, z1, material, parent, bevel=0.0, x=0.0, y=0.0, seg=14):
    """Abgerundete Platte (Grundriss w×h, Eckradius r) von z0 bis z1."""
    me = bpy.data.meshes.new(name)
    bm = bmesh.new()
    pts = rrect_pts(w, h, r, seg)
    bot = [bm.verts.new((x + px, y + py, z0)) for px, py in pts]
    top = [bm.verts.new((x + px, y + py, z1)) for px, py in pts]
    bm.faces.new(list(reversed(bot)))
    bm.faces.new(top)
    n = len(pts)
    for i in range(n):
        bm.faces.new((bot[i], bot[(i + 1) % n], top[(i + 1) % n], top[i]))
    bm.to_mesh(me)
    bm.free()
    ob = bpy.data.objects.new(name, me)
    ob.data.materials.append(material)
    link(ob, parent)
    if bevel > 0:
        md = ob.modifiers.new("bevel", "BEVEL")
        md.width = bevel
        md.segments = 5
        md.limit_method = "ANGLE"
        md.angle_limit = math.radians(50)
        md.harden_normals = True
    for f in ob.data.polygons:
        f.use_smooth = True
    return ob


def disc(name, rad, z0, z1, material, parent, x, y, bevel=0.0, seg=48):
    return slab(name, 2 * rad, 2 * rad, rad * 0.999, z0, z1, material, parent, bevel, x, y, seg=seg // 4)


def decal(name, w, h, z, material, parent, x=0.0, y=0.0, flip=False, r=0.0):
    """Bildfläche mit UV über das Rechteck (für Logo und Bildschirm)."""
    me = bpy.data.meshes.new(name)
    bm = bmesh.new()
    pts = rrect_pts(w, h, r, 10) if r > 0 else [(w / 2, h / 2), (-w / 2, h / 2), (-w / 2, -h / 2), (w / 2, -h / 2)]
    vs = [bm.verts.new((x + px, y + py, z)) for px, py in pts]
    f = bm.faces.new(list(reversed(vs)) if flip else vs)
    uv = bm.loops.layers.uv.new()
    for lp in f.loops:
        co = lp.vert.co
        u = (co.x - x) / w + 0.5
        v = (co.y - y) / h + 0.5
        lp[uv].uv = ((1 - u) if flip else u, v)
    bm.to_mesh(me)
    bm.free()
    ob = bpy.data.objects.new(name, me)
    ob.data.materials.append(material)
    link(ob, parent)
    return ob


def text(body, size, x, y, z, material, parent, font=None, align="CENTER", rot_z=0.0, flip=False):
    cu = bpy.data.curves.new("txt", "FONT")
    cu.body = body
    cu.size = size
    cu.align_x = align
    cu.align_y = "CENTER"
    cu.extrude = 0.0006
    if font:
        cu.font = bpy.data.fonts.load(font)
    ob = bpy.data.objects.new("txt", cu)
    ob.data.materials.append(material)
    link(ob, parent)
    ob.location = (x, y, z)
    ob.rotation_euler = (0, math.pi if flip else 0, rot_z)
    return ob


def phone(screen_img, back_state="signal", v=None):
    v = v or {}
    C_RUBBER = v.get("rubber", C_RUBBER_D)
    C_FRAME = v.get("frame", C_FRAME_D)
    C_BACK = v.get("back", C_BACK_D)
    C_DOTON = v.get("doton", C_DOTON_D)
    C_DOTOFF = v.get("dotoff", C_DOTOFF_D)
    C_WIN = v.get("win", C_WIN_D)
    root = bpy.data.objects.new("phone", None)
    bpy.context.collection.objects.link(root)
    z = T / 2
    # Rahmen aus gummiertem Kunststoff (umlaufend) + Körper
    rim = slab("rim", W, H, 0.10, -z, z, C_RUBBER, root, bevel=0.022)
    body = slab("body", W - 0.03, H - 0.03, 0.088, -z - 0.001, z + 0.001, C_FRAME, root, bevel=0.01)
    # Eck-Puffer: an den vier Ecken etwas dicker
    for sx in (-1, 1):
        for sy in (-1, 1):
            o = slab("bump", 0.21, 0.21, 0.104, -z - 0.0015, z + 0.0035, C_RUBBER, root, bevel=0.02,
                     x=sx * (W / 2 - 0.099), y=sy * (H / 2 - 0.099))
            o.scale = (1.0, 1.0, 1.0)
    # Rückseite (oben = +Z)
    slab("backpanel", W - 0.075, H - 0.075, 0.075, z, z + 0.004, C_BACK, root, bevel=0.003)
    zb = z + 0.004
    # Kamera-Insel oben links (von hinten gesehen)
    ix, iy = -W / 2 + 0.205, H / 2 - 0.25
    slab("island", 0.27, 0.40, 0.085, zb, zb + 0.014, C_ISLAND, root, bevel=0.006, x=ix, y=iy)
    zi = zb + 0.014
    for k, ly in enumerate((iy + 0.095, iy - 0.075)):
        disc("ring", 0.068, zi, zi + 0.01, C_RING, root, ix - 0.02, ly, bevel=0.004)
        disc("lens", 0.054, zi + 0.002, zi + 0.0115, C_LENS, root, ix - 0.02, ly)
        disc("glass", 0.056, zi + 0.0115, zi + 0.0125, C_GLASS, root, ix - 0.02, ly)
    disc("flash", 0.02, zi, zi + 0.004, C_FLASH, root, ix + 0.085, iy + 0.12)
    disc("mic", 0.007, zi, zi + 0.0005, C_HOLE, root, ix + 0.085, iy + 0.04)
    # Fenster unter der Kamera (wie im Entwurf): leicht helleres Glas mit goldenen Antennen-Leiterbahnen
    wy = H / 2 - 0.62
    slab("window", W - 0.13, 0.30, 0.05, zb, zb + 0.0015, C_WIN, root, bevel=0.0012, x=0.0, y=wy)
    zw = zb + 0.0015
    ww = W - 0.13
    def trace(x0, y0, x1, y1):
        cx, cy = (x0 + x1) / 2, (y0 + y1) / 2
        lw = abs(x1 - x0) + 0.006
        lh = abs(y1 - y0) + 0.006
        slab("trace", lw, lh, 0.0029, zw, zw + 0.0006, C_GOLD, root, x=cx, y=cy, seg=2)
    for sx in (-1, 1):
        xo = sx * (ww / 2 - 0.04)
        xi = sx * (ww / 2 - 0.12)
        xm = sx * (ww / 2 - 0.07)
        top = wy + 0.11
        trace(xo, top, xi, top)
        trace(xi, top, xi, top - 0.06)
        trace(xi, top - 0.06, xm, top - 0.06)
        trace(xm, top - 0.06, xm, top - 0.12)
        trace(xm, top - 0.12, xi, top - 0.12)
        trace(xo, wy - 0.12, sx * (ww / 2 - 0.16), wy - 0.12)
    # Punktmatrix mittig im Fenster
    pat = matrix(back_state)
    sp = 0.019
    mx0, my0 = -7 * sp, wy + 3 * sp
    for r in range(7):
        for c in range(15):
            m = C_DOTON if pat[r][c] else C_DOTOFF
            if back_state == "sos" and pat[r][c]:
                m = C_SOSDOT
            disc("dot", 0.0068, zw - 0.0005, zw + 0.0012, m, root, mx0 + c * sp, my0 - r * sp, seg=24)
    text("50 MP  ·  OIS", 0.024, ix + 0.255, iy + 0.13, zb + 0.0002, C_TEXT, root, font="/usr/share/fonts/opentype/inter/Inter-Medium.otf", align="LEFT")
    text("ULTRAWEIT", 0.024, ix + 0.255, iy + 0.09, zb + 0.0002, C_TEXT, root, font="/usr/share/fonts/opentype/inter/Inter-Medium.otf", align="LEFT")
    # Schrauben
    for sx, sy in ((1, 1), (1, -1), (-1, -1), (-1, 1)):
        if sx == -1 and sy == 1:
            continue
        disc("screw", 0.014, zb, zb + 0.002, C_SCREW, root, sx * (W / 2 - 0.085), sy * (H / 2 - 0.085), bevel=0.0015)
    disc("screw", 0.014, zb, zb + 0.002, C_SCREW, root, ix + 0.02, iy - 0.255, bevel=0.0015)
    # Logo (echtes KM-Code-Craft-Logo) mittig
    decal("logo", 0.50, 0.50 * 218 / 520, zb + 0.0004, M_LOGO, root, 0.0, -0.19)
    text("ULTAR OS  ·  IP68", 0.030, 0.0, -H / 2 + 0.17, zb + 0.0002, C_TEXT, root, font="/usr/share/fonts/opentype/inter/Inter-Medium.otf")
    decal("artext", 0.20, 0.04, zb + 0.0003, M_AR, root, 0.0, -H / 2 + 0.12)
    # Vorderseite (unten = −Z): Glas, Bildschirm
    slab("glass", W - 0.06, H - 0.06, 0.08, -z - 0.003, -z, C_BEZEL, root, bevel=0.002)
    sw, sh = W - 0.105, H - 0.105
    decal("screen", sw, sh, -z - 0.0032, screen_mat(screen_img), root, flip=True, r=0.06)
    disc("punch", 0.016, -z - 0.0036, -z - 0.0033, C_HOLE, root, 0.0, H / 2 - 0.10)
    # Seitentasten: von vorn gesehen rechts = −X
    def key(name, y, length, m, side=-1, depth=0.012, thick=0.045):
        o = slab(name, depth * 2, length, 0.012, -thick / 2, thick / 2, m, root, bevel=0.004, x=side * (W / 2 + 0.002), y=y)
        return o
    key("vol", 0.32, 0.20, C_BTN)
    key("power", 0.06, 0.11, C_BTN)
    key("km", -0.12, 0.085, C_GOLD)
    key("sos", 0.25, 0.13, C_ORANGE, side=1, depth=0.010)
    # Stabantenne oben (von hinten rechts)
    ant = bpy.data.objects.new("ant", None)
    link(ant, root)
    a = slab("antenna", 0.034, 0.10, 0.0169, -0.017, 0.017, C_FRAME, ant, bevel=0.006, x=W / 2 - 0.16, y=H / 2 + 0.03)
    slab("antcap", 0.036, 0.03, 0.0179, -0.018, 0.018, C_GOLD, ant, bevel=0.006, x=W / 2 - 0.16, y=H / 2 + 0.085)
    # unten: USB-C, Ladekontakte
    # Kanten-Teile (Öffnungen per Boolean in Rahmen und Körper geschnitten)
    def edge_part(name, w, h, r, x, ye, depth, m, top=False, cut=False):
        """Teil an der oberen/unteren Kante: Fläche w (x) × h (z), Tiefe entlang y."""
        o = slab(name, w, h, r, -depth / 2, depth / 2, m, root, seg=8)
        o.rotation_euler = (math.radians(90), 0, 0)
        o.location = (x, ye, 0.0)
        if cut:
            o.hide_render = True
            o.display_type = "WIRE"
            for tgt in (rim,):
                md = tgt.modifiers.new("cut_" + name, "BOOLEAN")
                md.operation = "DIFFERENCE"
                md.object = o
                md.solver = "EXACT"
        return o
    yb = -H / 2
    # USB-C
    edge_part("usb_cut", 0.090, 0.032, 0.0159, 0.0, yb, 0.06, C_HOLE, cut=True)
    edge_part("usb_in", 0.084, 0.026, 0.0129, 0.0, yb + 0.026, 0.01, C_HOLE)
    edge_part("usb_tongue", 0.06, 0.007, 0.003, 0.0, yb + 0.02, 0.012, C_GOLD)
    # Lautsprecher (6 Löcher) und Mikrofon
    for i in range(6):
        edge_part(f"spk{i}", 0.014, 0.014, 0.0069, -0.27 + i * 0.026, yb, 0.03, C_HOLE, cut=True)
        edge_part(f"spkb{i}", 0.014, 0.014, 0.0069, -0.27 + i * 0.026, yb + 0.012, 0.004, C_HOLE)
    edge_part("mic_cut", 0.008, 0.008, 0.0039, 0.10, yb, 0.03, C_HOLE, cut=True)
    # Ladekontakte für die Halterung (vergoldet, bündig)
    for i in range(4):
        edge_part(f"pin{i}", 0.014, 0.014, 0.0069, 0.17 + i * 0.026, yb - 0.0005, 0.004, C_GOLD)
    # oben: Antennenbuchse mit Gummikappe, Mikrofon
    yt = H / 2
    edge_part("sma_cap", 0.05, 0.05, 0.0249, -0.20, yt + 0.003, 0.012, C_RUBBER)
    edge_part("sma_ring", 0.054, 0.054, 0.0269, -0.20, yt + 0.0005, 0.004, C_RING)
    edge_part("mic2_cut", 0.008, 0.008, 0.0039, 0.02, yt, 0.03, C_HOLE, cut=True)
    # SIM-Schublade (rechte Seite von hinten = +X): Fuge und Loch
    tray = slab("tray", 0.006, 0.16, 0.002, -0.016, 0.016, C_HOLE, root, x=W / 2 + 0.0005, y=-0.18)
    for tgt in (rim, body):
        for o2 in bpy.context.view_layer.objects:
            o2.select_set(False)
        bpy.context.view_layer.objects.active = tgt
        tgt.select_set(True)
        bpy.ops.object.shade_auto_smooth(angle=math.radians(40))
    tray2 = slab("trayface", 0.004, 0.152, 0.002, -0.0135, 0.0135, C_RUBBER, root, x=W / 2 + 0.002, y=-0.18)
    return root


def matrix(name):
    g = [[0] * 15 for _ in range(7)]

    def put(rows, x0, y0):
        for y, row in enumerate(rows):
            for x, ch in enumerate(row):
                if ch == "#" and 0 <= y0 + y < 7 and x0 + x < 15:
                    g[y0 + y][x0 + x] = 1

    S = ["###", "#..", "###", "..#", "###"]
    O = ["###", "#.#", "#.#", "#.#", "###"]
    if name == "sos":
        put(S, 2, 1)
        put(O, 6, 1)
        put(S, 10, 1)
    elif name == "msg":
        put(["###########", "##.......##", "#.#.....#.#", "#..#...#..#", "#...#.#...#", "#....#....#", "###########"], 2, 0)
    else:
        for i, h in enumerate((2, 3, 4, 5, 7)):
            for y in range(7 - h, 7):
                g[y][1 + i * 3] = 1
                g[y][2 + i * 3] = 1
    return g


C_SOSDOT = mat("sosdot", (1.0, 0.25, 0.2), rough=0.3, emit=(1.0, 0.22, 0.15), estr=7.0)
M_AR = img_mat("ar", DIR + "ar_text.png", rough=0.5, use_alpha=True)
M_AR.node_tree.nodes["Principled BSDF"].inputs["Base Color"].default_value = (0.2, 0.2, 0.21, 1)
for _l in list(M_AR.node_tree.links):
    if _l.to_socket.name == "Base Color":
        M_AR.node_tree.links.remove(_l)
M_LOGO = img_mat("logo", DIR + "logo.png", metal=0.85, rough=0.32, use_alpha=True)

_screens = {}


def screen_mat(path):
    if path not in _screens:
        _screens[path] = img_mat("screen", path, emit=1.6, rough=0.05)
        p = _screens[path].node_tree.nodes["Principled BSDF"]
        p.inputs["Coat Weight"].default_value = 1.0
        p.inputs["Base Color"].default_value = (0, 0, 0, 1)
        # Grundfarbe schwarz, Bild nur als Leuchten (wie ein echtes Display)
        for l in list(_screens[path].node_tree.links):
            if l.to_socket == p.inputs["Base Color"]:
                _screens[path].node_tree.links.remove(l)
    return _screens[path]


# ------------------------------------------------------------ Studio
def studio(floor_color=(0.012, 0.012, 0.014)):
    world = bpy.data.worlds.new("w")
    world.use_nodes = True
    world.node_tree.nodes["Background"].inputs["Color"].default_value = (0.004, 0.004, 0.005, 1)
    world.node_tree.nodes["Background"].inputs["Strength"].default_value = 1.0
    sc.world = world
    # Hohlkehle: Boden geht weich in die Rückwand über (Fotostudio)
    me = bpy.data.meshes.new("sweep")
    bm = bmesh.new()
    prof = [(y, 0.0) for y in [-30 + i for i in range(31)]]
    R = 4.0
    for i in range(1, 25):
        a = math.radians(90 * i / 24)
        prof.append((1.0 + R * math.sin(a), R - R * math.cos(a)))
    prof += [(1.0 + R, R + k) for k in range(1, 12)]
    rows = []
    for y, zz in prof:
        rows.append((bm.verts.new((-30, y, zz)), bm.verts.new((30, y, zz))))
    for i in range(len(rows) - 1):
        bm.faces.new((rows[i][0], rows[i][1], rows[i + 1][1], rows[i + 1][0]))
    bm.to_mesh(me)
    bm.free()
    fl = bpy.data.objects.new("sweep", me)
    bpy.context.collection.objects.link(fl)
    for f in fl.data.polygons:
        f.use_smooth = True
    fl.data.materials.append(mat("floor", floor_color, rough=0.34))

    def area(name, loc, rot, size, power, color=(1, 1, 1), shape="RECTANGLE", sy=None):
        ld = bpy.data.lights.new(name, "AREA")
        ld.energy = power
        ld.size = size
        ld.shape = shape
        if sy:
            ld.size_y = sy
        ld.color = color
        ob = bpy.data.objects.new(name, ld)
        bpy.context.collection.objects.link(ob)
        ob.location = loc
        ob.rotation_euler = rot
        return ob

    area("key", (-3.2, -3.0, 4.2), Euler((math.radians(50), 0, math.radians(-45))), 3.0, 900, (1.0, 0.97, 0.92))
    area("rim", (2.6, 1.6, 2.6), Euler((math.radians(-60), 0, math.radians(135))), 0.6, 450, (1.0, 0.88, 0.62), "RECTANGLE", 4.0)
    area("fill", (3.5, -3.5, 1.6), Euler((math.radians(75), 0, math.radians(45))), 3.0, 140, (0.85, 0.9, 1.0))
    area("top", (0, 0, 6), Euler((0, 0, 0)), 4.0, 160)


def camera(loc, look, lens=85):
    cd = bpy.data.cameras.new("cam")
    cd.lens = lens
    cd.dof.use_dof = True
    cd.dof.aperture_fstop = 8.0
    ob = bpy.data.objects.new("cam", cd)
    bpy.context.collection.objects.link(ob)
    ob.location = loc
    d = Vector(look) - Vector(loc)
    ob.rotation_euler = d.to_track_quat("-Z", "Y").to_euler()
    cd.dof.focus_distance = d.length
    sc.camera = ob
    return ob


def stand(p, x, y, yaw_deg, show_back=True, lean=6):
    """Telefon aufrecht hinstellen; show_back: Rückseite zur Kamera (−Y)."""
    p.rotation_mode = "XYZ"
    # Grundlage: Rückseite (+Z) zeigt nach −Y, oben (+Y) zeigt nach +Z
    base = Euler((math.radians(90 + lean), 0, 0), "XYZ")
    if not show_back:
        base = Euler((math.radians(-90 - lean), math.radians(180), 0), "XYZ")
    m = base.to_matrix().to_4x4()
    from mathutils import Matrix
    m = Matrix.Rotation(math.radians(yaw_deg), 4, "Z") @ m
    p.matrix_world = m
    p.location = (x, y, H / 2 + 0.003)


# ------------------------------------------------------------ Ausführungen
WUESTE = {
    "rubber": mat("w_rubber", (0.33, 0.25, 0.14), rough=0.75, noise=0.15),
    "frame": mat("w_frame", (0.42, 0.33, 0.20), rough=0.5, noise=0.05),
    "back": mat("w_back", (0.46, 0.37, 0.23), rough=0.6, noise=0.25),
    "win": mat("w_win", (0.52, 0.43, 0.29), rough=0.15, coat=1.0),
    "doton": mat("w_doton", (0.02, 0.02, 0.02), rough=0.3),
    "dotoff": mat("w_dotoff", (0.38, 0.30, 0.19), rough=0.3),
}
SIGNAL = {
    "rubber": mat("s_rubber", (0.85, 0.12, 0.0), rough=0.7, noise=0.15),
}

# ------------------------------------------------------------ Szenen
studio()
if SCENE == "hero":
    b = phone(DIR + "screen_de.png", "signal")
    stand(b, 0.55, 0.35, 28, show_back=True)
    f = phone(DIR + "screen_de.png")
    stand(f, -0.48, -0.15, -18, show_back=False)
    camera((0.2, -6.4, 1.6), (0.03, 0.05, 0.86), lens=70)
elif SCENE == "back":
    b = phone(DIR + "screen_de.png", "msg")
    stand(b, 0.0, 0.0, 24, show_back=True, lean=4)
    camera((1.6, -4.6, 1.9), (0.0, 0.0, 0.86), lens=85)
elif SCENE == "front":
    f = phone(DIR + "screen_ar.png")
    stand(f, 0.0, 0.0, -14, show_back=False, lean=4)
    camera((-1.2, -4.8, 1.5), (0.0, 0.0, 0.86), lens=85)
elif SCENE == "colors":
    for x, var, st, yaw in ((-1.05, None, "signal", 12), (0.0, WUESTE, "msg", 0), (1.05, SIGNAL, "sos", -12)):
        p = phone(DIR + "screen_de.png", st, var)
        stand(p, x, 0.0, yaw, show_back=True, lean=4)
    camera((0.0, -7.4, 1.5), (0.0, 0.0, 0.88), lens=70)
elif SCENE == "keys":
    f = phone(DIR + "screen_de.png", "signal")
    stand(f, 0.0, 0.0, -112, show_back=False, lean=2)
    camera((-0.45, -3.6, 1.25), (0.0, 0.0, 0.86), lens=80)
elif SCENE == "bottom":
    p = phone(DIR + "screen_de.png", "signal")
    p.location = (0.0, 0.0, T / 2 + 0.004)
    p.rotation_euler = (0, 0, math.radians(-8))
    camera((0.35, -2.25, 0.32), (0.0, -0.78, 0.04), lens=90)
elif SCENE == "top":
    p = phone(DIR + "screen_de.png", "signal")
    p.location = (0.0, 0.0, T / 2 + 0.004)
    p.rotation_euler = (0, 0, math.radians(180 + 10))
    camera((-0.15, -2.5, 0.42), (0.0, -0.78, 0.05), lens=72)
elif SCENE == "sos":
    b = phone(DIR + "screen_de.png", "sos")
    stand(b, 0.0, 0.0, -32, show_back=True, lean=4)
    camera((-1.9, -4.4, 1.3), (0.0, 0.0, 0.9), lens=95)

sc.render.engine = "CYCLES"
sc.cycles.device = "CPU"
sc.cycles.samples = SAMPLES
sc.cycles.use_denoising = True
sc.cycles.max_bounces = 8
sc.render.resolution_x = RW
sc.render.resolution_y = RH
sc.render.resolution_percentage = 100
sc.view_settings.view_transform = "AgX"
sc.view_settings.look = "AgX - Medium High Contrast"
sc.render.film_transparent = False
sc.render.filepath = OUT
bpy.ops.render.render(write_still=True)
print("FERTIG", OUT)
