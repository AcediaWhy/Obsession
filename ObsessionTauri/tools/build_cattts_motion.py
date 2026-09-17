"""Group original SVG paths for a reversible motion study. No simplification.

The source art is a posterised vector trace: 1448 sibling <path> elements, each carrying
its own scale(0.5 0.5), drawn back-to-front. Nothing here redraws or simplifies geometry.
Two structural liberties are taken, both reversible:

  * cat and mouse silhouettes are split at joint roots, with small hidden overlaps;
  * everything else is only *wrapped* in <g> elements, never reordered.

That second rule matters. The cat's exposed paw (1334) and mouse (4) are drawn early and are
deliberately overlapped by foliage painted later, so hoisting them next to the head would
float them on top of the leaves. Instead each fragment is wrapped where it already sits and
every cat group gets the same breathing transform while leaving paint order untouched.
Joints have independent nested transforms. The mouse is independent of cat breathing.
"""
from pathlib import Path
import xml.etree.ElementTree as ET
import copy
import random
import re
import sys

root_dir = Path(__file__).resolve().parents[1]
source = Path(sys.argv[1]) if len(sys.argv) > 1 else Path.home() / 'Downloads/cattts.svg'
ns = 'http://www.w3.org/2000/svg'
ET.register_namespace('', ns)
ET.register_namespace('xlink', 'http://www.w3.org/1999/xlink')
svg = ET.fromstring(source.read_bytes())
paths = svg.findall(f'{{{ns}}}path')

CAT_INK = '#49281D'
assert len(paths) == 1448, 'Source changed: review path selections before rebuilding.'
assert all(paths[i].get('fill') == '#F9E7DB' for i in [1151, 1153])
# This ink identifies animal silhouettes, but not their ownership: 4 is the mouse,
# 1121 is the main cat, and 1334 is the exposed paw below the foreground leaf.
assert [i for i, p in enumerate(paths) if p.get('fill') == CAT_INK] == [4, 1121, 1334]
assert all(p.get('transform') == 'scale(0.5 0.5)' for p in paths), 'Pivots assume a uniform half scale.'

NUM = re.compile(r'-?\d*\.?\d+(?:e-?\d+)?')


def numbers(text):
    return [float(v) for v in NUM.findall(text)]


def bbox(idx_range):
    """Bounding box of paths[idx_range] in screen units (the viewBox is half the source)."""
    xs, ys = [], []
    for i in idx_range:
        v = numbers(paths[i].get('d'))
        xs += v[0::2]
        ys += v[1::2]
    return min(xs) / 2, min(ys) / 2, max(xs) / 2, max(ys) / 2


def union(*boxes):
    return (min(b[0] for b in boxes), min(b[1] for b in boxes),
            max(b[2] for b in boxes), max(b[3] for b in boxes))


# ---------------------------------------------------------------- original single-arc calibration
# Retained from the earlier study; the active multi-joint rig is partition_outline below.

def parse_path(d):
    """Flatten a closed M/L/C/Z outline into a start point and one segment per vertex."""
    segs, start = [], None
    for cmd, arg in re.findall(r'([MLCZ])([^MLCZ]*)', d):
        a = numbers(arg)
        if cmd == 'M':
            start = (a[0], a[1])
        elif cmd == 'C':
            segs += [('C', tuple(a[k:k + 6])) for k in range(0, len(a), 6)]
        elif cmd == 'L':
            segs += [('L', tuple(a[k:k + 2])) for k in range(0, len(a), 2)]
    return start, segs


def fmt(v):
    return f'{v:.6f}'.rstrip('0').rstrip('.') or '0'


def dump_path(start, segs):
    out = [f'M{fmt(start[0])} {fmt(start[1])}']
    out += [cmd + ' '.join(fmt(x) for x in a) for cmd, a in segs]
    return ''.join(out) + 'Z'


# Root of the left ear, read off the silhouette's upper-edge profile: the ear is a narrow
# spike rising ~80 units above the surrounding fur, and these two points sit where the
# profile breaks on either side of it.
EAR_ROOT_HINT = ((892.0, 690.0), (1052.0, 668.0))

start, segs = parse_path(paths[1121].get('d'))
verts = [start] + [s[1][-2:] for s in segs]
assert verts[-1] == start, 'Cat silhouette is not a closed loop; the ear cut needs one.'
verts = verts[:-1]
loop = len(verts)


def nearest_vertex(target):
    return min(range(loop), key=lambda k: (verts[k][0] - target[0]) ** 2 + (verts[k][1] - target[1]) ** 2)


def walk(i, j):
    """Segments walking the closed loop from vertex i round to vertex j."""
    out, k = [], i
    while k != j:
        out.append(segs[k])
        k = (k + 1) % loop
    return out


cut_a, cut_b = (nearest_vertex(t) for t in EAR_ROOT_HINT)
for got, want in zip((verts[cut_a], verts[cut_b]), EAR_ROOT_HINT):
    drift = ((got[0] - want[0]) ** 2 + (got[1] - want[1]) ** 2) ** 0.5
    assert drift < 40, f'Ear root moved: nearest vertex to {want} is {got}, {drift:.0f} away.'

ear_segs, head_segs = walk(cut_a, cut_b), walk(cut_b, cut_a)
assert len(ear_segs) + len(head_segs) == loop


def outline_box(first, walked):
    xs, ys = [first[0]], [first[1]]
    for _, a in walked:
        xs += list(a[0::2])
        ys += list(a[1::2])
    return min(xs) / 2, min(ys) / 2, max(xs) / 2, max(ys) / 2


ear_box, head_box = outline_box(verts[cut_a], ear_segs), outline_box(verts[cut_b], head_segs)
# The ear has to be the small arc that reaches highest; if the cut ever lands elsewhere the
# two arcs swap roles and the assert below is what tells us.
assert ear_box[1] < head_box[1], 'Ear arc is not the topmost one — check the cut points.'
assert ear_box[2] - ear_box[0] < 120, f'Ear arc is too wide ({ear_box[2] - ear_box[0]:.0f} units).'

EAR_PIVOT = ((verts[cut_a][0] + verts[cut_b][0]) / 4, (verts[cut_a][1] + verts[cut_b][1]) / 4)


def clone(i, cls, d=None):
    el = copy.deepcopy(paths[i])
    el.set('class', cls)
    if d:
        el.set('d', d)
    return el


cat_head = clone(1121, 'cat-head', dump_path(verts[cut_b], head_segs))
cat_ear = clone(1121, 'cat-ear-blade', dump_path(verts[cut_a], ear_segs))
# A still copy of the same wedge sits under the ear. As the ear tips, the gap opening at its
# root is already filled with fur ink, so the join reads as a bend rather than a tear. Same
# trick as the leaf contact shadows below.
cat_ear_root = copy.deepcopy(cat_ear)
cat_ear_root.set('class', 'cat-ear-root')

# ---------------------------------------------------------------- group plan

# (first, last, class, wants a contact shadow)
LEAVES = [
    (337, 480, 'leaf-west', True),
    (1154, 1261, 'leaf-upper', True),
    (1262, 1293, 'leaf-east', True),
    (1294, 1319, 'leaf-crown', False),
    (1320, 1333, 'leaf-lower', True),
    # 1335..1348 are small marks around the exposed paw, far from blade 1320.
    # Keep their paint order and exclude them from the leaf's large edge pivot.
    (1349, 1447, 'leaf-cheek', False),
]
# Leaves whose blade runs off the frame hinge on that edge; leaves sitting wholly inside the
# picture have no visible stem, so they drift instead of swinging (see the stylesheet).
INNER_LEAVES = {'leaf-crown', 'leaf-cheek'}
CAT_FRAGMENTS = [(1121, 1121), (1150, 1153), (1334, 1334)]


def edge_pivot(box):
    """Hinge a leaf on whichever frame edge it runs off, opposite its own centre."""
    x0, y0, x1, y1 = box
    cx, cy = (x0 + x1) / 2, (y0 + y1) / 2
    touching = {'left': x0 <= 1, 'right': x1 >= 1023, 'top': y0 <= 1, 'bottom': y1 >= 1023}
    if touching['right'] and cx > 512:
        return 1024, cy
    if touching['left'] and cx < 512:
        return 0, cy
    if touching['bottom']:
        return cx, 1024
    if touching['top']:
        return cx, 0
    return cx, y1          # no edge to hinge on: pivot at the foot of the blade


leaf_spans = {}
for first, last, cls, _ in LEAVES:
    leaf_spans.setdefault(cls, []).append(range(first, last + 1))
# Both halves of a split leaf must share one pivot, or they tear apart as they swing.
leaf_pivot = {cls: edge_pivot(union(*(bbox(r) for r in spans)))
              for cls, spans in leaf_spans.items()}

cat_box = union(*(bbox(range(f, l + 1)) for f, l in CAT_FRAGMENTS))
CAT_PIVOT = ((cat_box[0] + cat_box[2]) / 2, cat_box[3])       # breathe from the ground up

members, replacements = set(), {}


def origin(x, y):
    return f'transform-origin:{x:g}px {y:g}px'


def group(cls, pivot, children):
    g = ET.Element(f'{{{ns}}}g', {'class': cls, 'style': origin(*pivot)})
    g.extend(children)
    return g


def partition_outline(index, specs):
    """Cut disjoint silhouette arcs; extend their roots inside the remaining body.

    Hints and pivots use viewBox units. Original Bezier segments stay untouched.
    The 5-unit overlap hides the seam while the parts rotate a few degrees.
    """
    first, segments = parse_path(paths[index].get('d'))
    points = [first] + [segment[1][-2:] for segment in segments]
    assert points[-1] == first
    points.pop()
    count = len(points)
    occupied, cuts, parts = set(), {}, {}
    for name, hint_a, hint_b, pivot, inward in specs:
        ends = [min(range(count), key=lambda k: (points[k][0]/2-h[0])**2 +
                    (points[k][1]/2-h[1])**2) for h in (hint_a, hint_b)]
        a, b = ends
        if (b-a) % count > count/2:
            a, b = b, a
        arc_indices = [(a+j) % count for j in range((b-a) % count)]
        assert not occupied.intersection(arc_indices), f'Overlapping cuts: {name}'
        occupied.update(arc_indices)
        arc = [segments[k] for k in arc_indices]
        # Both seam ends continue inward, entirely underneath the body outline.
        dx, dy = inward[0]*2, inward[1]*2
        arc += [('L', (points[b][0]+dx, points[b][1]+dy)),
                ('L', (points[a][0]+dx, points[a][1]+dy))]
        parts[name] = group(name, pivot, [clone(index, 'joint-blade', dump_path(points[a], arc))])
        cuts[a] = b
    start_index = next(k for k in range(count) if k not in occupied)
    body_segments, k = [], start_index
    while True:
        if k in cuts:
            k = cuts[k]
            body_segments.append(('L', points[k]))
        else:
            body_segments.append(segments[k])
            k = (k+1) % count
        if k == start_index:
            break
    return clone(index, 'joint-body', dump_path(points[start_index], body_segments)), parts


# Semantic cuts are reviewed on artifacts/cattts-parts-map.svg. Path 4 is the
# mouse, not a cat paw: sharing an ink is not sufficient to share a motion group.
cat_core, cat_joints = partition_outline(1121, [
    ('cat-ear--left', (472,374), (525,350), (494,364), (0,6)),
    ('cat-ear--right', (575,362), (646,415), (612,389), (-4,4)),
    ('cat-paw--right', (667,494), (521,525), (592,508), (0,-6)),
    ('cat-paw--left', (495,524), (370,493), (435,509), (0,-6)),
])
mouse_core, mouse_joints = partition_outline(4, [
    ('mouse-ear--left', (503,725), (520,715), (510,722), (1,4)),
    ('mouse-ear--right', (529,726), (524,754), (526,740), (-4,0)),
])

# The original silhouette paints through a narrow collar at each ear root.
# Its clipped copy remains still underneath the ear while the outer tip turns.
# Clip coordinates are in the path's local units (twice the viewBox coordinates).
defs = svg.find(f'{{{ns}}}defs')
assert defs is not None
ear_backing = []
for name, polygon in [
    ('left', '932,720 1038,672 1058,718 952,766'),
    ('right', '1170,696 1244,750 1212,788 1134,740'),
]:
    clip_id = f'cattts-{name}-ear-root'
    clip = ET.SubElement(defs, f'{{{ns}}}clipPath',
                         {'id': clip_id, 'clipPathUnits': 'userSpaceOnUse'})
    ET.SubElement(clip, f'{{{ns}}}polygon', {'points': polygon})
    backing = clone(1121, 'cat-ear-root')
    backing.set('clip-path', f'url(#{clip_id})')
    ear_backing.append(backing)

mouse_backing = []
for name, polygon in [
    ('left', '992,1422 1026,1402 1048,1444 1014,1464'),
    ('right', '1092,1426 1096,1520 1028,1520 1024,1428'),
]:
    clip_id = f'cattts-mouse-{name}-ear-root'
    clip = ET.SubElement(defs, f'{{{ns}}}clipPath',
                         {'id': clip_id, 'clipPathUnits': 'userSpaceOnUse'})
    ET.SubElement(clip, f'{{{ns}}}polygon', {'points': polygon})
    backing = clone(4, 'mouse-ear-root')
    backing.set('clip-path', f'url(#{clip_id})')
    mouse_backing.append(backing)


for first, last, cls, shadow in LEAVES:
    span = range(first, last + 1)
    out = []
    if shadow:
        # Flat dark silhouette remains under the leaf, acting as a contact shadow
        # as its edge lifts. No duplicated veins or textures in the exposed gap.
        flat = clone(first, 'leaf-contact-shadow')
        flat.set('fill', '#9B5435')
        out.append(flat)
    kind = 'leaf-inner' if cls in INNER_LEAVES else 'leaf-hinged'
    out.append(group(f'moving-leaf {kind} {cls}', leaf_pivot[cls], [paths[i] for i in span]))
    members.update(paths[i] for i in span)
    replacements[paths[first]] = out

for first, last in CAT_FRAGMENTS:
    span = range(first, last + 1)
    if first == 1121:
        body = [*ear_backing, *cat_joints.values(), cat_core]
    elif first == 1150:
        # The gaze shifts both eyes together — moving one alone reads as a squint. The nose
        # rides along, which is what a small turn of the muzzle would do anyway.
        eyes = {1151: 'cat-eye cat-eye--left', 1153: 'cat-eye cat-eye--right'}
        face = [group(eyes[i], ((bbox([i])[0] + bbox([i])[2]) / 2, (bbox([i])[1] + bbox([i])[3]) / 2), [paths[i]])
                if i in eyes else paths[i] for i in range(1151, 1154)]
        body = [group('cat-face', CAT_PIVOT, face)]
        body.insert(0, group('cat-ear--right', (612,389), [paths[1150]]))
    elif first == 1334:
        body = [group('cat-paw--peek', (542,610), [paths[1334]])]
    else:
        body = [paths[i] for i in span]
    members.update(paths[i] for i in span)
    replacements[paths[first]] = [group('cat-part', CAT_PIVOT, body)]

members.add(paths[4])
replacements[paths[4]] = [group('mouse-part', (521,762),
                                [*mouse_backing, *mouse_joints.values(), mouse_core])]
for i in [5,6,7,9]:
    members.add(paths[i])
    replacements[paths[i]] = [group('mouse-part', (521,762),
                                   [group('mouse-ear--left', (510,722), [paths[i]])])]
for i in [8,10]:
    members.add(paths[i])
    replacements[paths[i]] = [group('mouse-part', (521,762), [paths[i]])]

# These four highlights touch the exposed paw silhouette. They share its small
# shift and breath, while the surrounding leaf flecks remain in the scenery.
for i in [1335,1336,1337,1344]:
    members.add(paths[i])
    replacements[paths[i]] = [group('cat-part', CAT_PIVOT,
                                   [group('cat-paw--peek', (542,610), [paths[i]])])]

children = list(svg)
for child in children:
    svg.remove(child)
for child in children:
    if child in replacements:
        svg.extend(replacements[child])
    elif child not in members:
        svg.append(child)

# ---------------------------------------------------------------- passenger check

# The bug this guards against: 1334 is the cat's chest but it lives inside the lower leaf's
# index range, so it silently swung with the leaf. Any cat ink caught inside a leaf group,
# or any cat ink left unwrapped, is that same mistake coming back.
stray = []
for leaf in svg.iter(f'{{{ns}}}g'):
    cls = leaf.get('class', '')
    if 'moving-leaf' not in cls:
        continue
    for p in leaf.iter(f'{{{ns}}}path'):
        if p.get('fill') == CAT_INK:
            stray.append(f"  {CAT_INK} path riding inside '{cls}': {p.get('d')[:38]}…")
wrapped = sum(1 for g in svg.iter(f'{{{ns}}}g')
              if 'cat-part' in g.get('class', '')
              for p in g.iter(f'{{{ns}}}path') if p.get('fill') == CAT_INK)
# Four cat joints, remaining body, exposed paw and two narrow seam copies.
EXPECTED_CAT_INK = 8
if stray or wrapped != EXPECTED_CAT_INK:
    raise SystemExit('Cat fragments are grouped wrong:\n' + '\n'.join(stray) +
                     ('' if wrapped == EXPECTED_CAT_INK else
                      f'\n  {wrapped} of {EXPECTED_CAT_INK} {CAT_INK} paths ended up in a cat group.'))

# All uncut original curves must survive exactly once. This catches dropped eye,
# ear-colour and paw details even when their paint is not CAT_INK.
from collections import Counter
actual_curves = Counter((p.get('d'), p.get('fill'), p.get('transform'))
                        for p in svg.iter(f'{{{ns}}}path'))
for i, p in enumerate(paths):
    if i in (4, 1121):
        continue
    key = (p.get('d'), p.get('fill'), p.get('transform'))
    assert actual_curves[key] >= 1, f'Original path {i} was lost during grouping.'
assert len(mouse_joints) == 2

# ---------------------------------------------------------------- markup

svg.set('class', 'cattts-art')
svg.set('role', 'img')
svg.set('aria-label', 'Чёрный кот среди осенних листьев')
title = ET.Element(f'{{{ns}}}title')
title.text = 'Чёрный кот среди осенних листьев'
svg.insert(0, title)
markup = ET.tostring(svg, encoding='unicode')

# Drifting leaves are decorative, so their scatter is baked at build time from a fixed seed:
# the page stays identical between rebuilds and needs no JS to place them.
rng = random.Random(20260911)
drift_leaves = '\n '.join(
    '<i style="--x:{x}%;--size:{size}px;--duration:{dur}s;--delay:-{delay}s;--drift:{drift}px;--spin:{spin}deg"></i>'.format(
        x=rng.randrange(4, 92), size=rng.randrange(11, 23), dur=round(rng.uniform(15.0, 27.0), 1),
        delay=round(rng.uniform(0.0, 27.0), 1), drift=rng.randrange(-90, 90), spin=rng.randrange(320, 660))
    for _ in range(7))

html = '''<!doctype html>
<html lang="ru"><head><meta charset="UTF-8"/><meta name="viewport" content="width=device-width,initial-scale=1"/>
<title>Кот в листве — анимация SVG</title><link rel="stylesheet" href="/src/styles/catttsMotionLab.css"/>
</head><body>
<header><div><span class="eyebrow">OBSESSION / MOTION STUDY</span><h1>Осенняя прогулка</h1></div><span class="caption">Исходный SVG · без сжатия</span></header>
<main><div class="art-frame">''' + markup + '''
 <div class="sun-patch" aria-hidden="true"></div>
 <div class="drift-leaves" aria-hidden="true">
 ''' + drift_leaves + '''
 </div></div></main>
<footer>
 <button type="button" id="pause" aria-pressed="false">Пауза</button>
 <button type="button" id="blink">Моргнуть</button>
 <label class="wind">Ветер <input id="wind" type="range" min="0" max="2" step="0.1" value="1"/><output id="wind-value">1×</output></label>
 <label><input id="sun" type="checkbox" checked/> Свет</label>
 <label><input id="drift" type="checkbox" checked/> Листопад</label>
 <label><input id="original" type="checkbox"/> Без анимации · сравнить</label>
</footer><script type="module" src="/src/catttsMotionLab.js"></script>
</body></html>'''
(root_dir / 'cattts-motion-lab.html').write_text(html, encoding='utf-8')

print(f'Cut {len(cat_joints)} cat joints and {len(mouse_joints)} mouse ears with overlapping roots.')
print(f'Cat in {len(CAT_FRAGMENTS)} fragments, breathing from {CAT_PIVOT[0]:.0f},{CAT_PIVOT[1]:.0f}')
for cls, spans in leaf_spans.items():
    px, py = leaf_pivot[cls]
    kind = 'drift' if cls in INNER_LEAVES else 'hinge'
    print(f'  {cls:<11} {kind:<5} pivot {px:>4.0f},{py:<4.0f} '
          f'{sum(len(r) for r in spans):>3} paths in {len(spans)} span(s)')
print(f'Prepared cat ears and paws, mouse ears and {len(leaf_spans)} leaves; source curves retained.')
