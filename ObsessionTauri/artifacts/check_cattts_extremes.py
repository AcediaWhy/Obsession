"""Render stress poses for joint/paint-order review without touching the source art."""
from pathlib import Path
import xml.etree.ElementTree as E
import re, copy

base = Path(__file__).resolve().parents[1]
text = (base/'cattts-motion-lab.html').read_text(encoding='utf-8')
ns = 'http://www.w3.org/2000/svg'
E.register_namespace('', ns)
svg = E.fromstring(re.search(r'<svg[\s\S]*?</svg>', text)[0])
source = E.parse(Path.home()/'Downloads/cattts.svg').getroot()
original_cat = source.findall(f'{{{ns}}}path')[1121]
original_mouse = source.findall(f'{{{ns}}}path')[4]
angles = {'cat-ear--right':11, 'cat-ear--left':-8,
          'cat-paw--right':-1.2, 'cat-paw--left':1.2,
          'mouse-ear--left':-18, 'mouse-ear--right':17,
          'leaf-upper':2, 'leaf-lower':2.1, 'leaf-west':2.1, 'leaf-east':2}
for phase in [0,1,-1]:
    frame = copy.deepcopy(svg)
    defs = frame.find(f'{{{ns}}}defs')
    clip = E.SubElement(defs, f'{{{ns}}}clipPath', {'id':'right-ear-root-test','clipPathUnits':'userSpaceOnUse'})
    E.SubElement(clip, f'{{{ns}}}polygon', {'points':'1170,696 1244,750 1212,788 1134,740'})
    left_clip = E.SubElement(defs, f'{{{ns}}}clipPath', {'id':'left-ear-root-test','clipPathUnits':'userSpaceOnUse'})
    E.SubElement(left_clip, f'{{{ns}}}polygon', {'points':'932,720 1038,672 1058,718 952,766'})
    for name,polygon in [('left','992,1422 1026,1402 1048,1444 1014,1464'),
                         ('right','1092,1426 1096,1520 1028,1520 1024,1428')]:
        mouse_clip = E.SubElement(defs, f'{{{ns}}}clipPath', {'id':name+'-mouse-root-test','clipPathUnits':'userSpaceOnUse'})
        E.SubElement(mouse_clip, f'{{{ns}}}polygon', {'points':polygon})
    for node in frame.iter(f'{{{ns}}}g'):
        if node.get('class') == 'cat-part' and node.find(f'.//{{{ns}}}g[@class="cat-ear--right"]') is not None:
            root = copy.deepcopy(original_cat)
            root.set('clip-path','url(#right-ear-root-test)')
            node.insert(0,root)
            root = copy.deepcopy(original_cat)
            root.set('clip-path','url(#left-ear-root-test)')
            node.insert(0,root)
            break
    for node in frame.iter(f'{{{ns}}}g'):
        if node.get('class') == 'mouse-part' and node.find(f'.//{{{ns}}}g[@class="mouse-ear--right"]') is not None:
            for name in ['left','right']:
                root = copy.deepcopy(original_mouse)
                root.set('clip-path','url(#'+name+'-mouse-root-test)')
                node.insert(0,root)
            break
    for node in frame.iter(f'{{{ns}}}g'):
        classes = node.get('class','').split()
        pivot = re.findall(r'[\d.]+',node.get('style',''))
        for cls in classes:
            if cls in angles:
                x,y = pivot
                shift = 'translate(5 -7) ' if cls == 'cat-paw--peek' and phase else ''
                node.set('transform',f'{shift}rotate({angles[cls]*phase} {x} {y})')
    frame.set('viewBox','310 270 410 535')
    frame.set('width','820'); frame.set('height','1070')
    (base/f'artifacts/cattts-stress-{phase}.svg').write_text(E.tostring(frame,encoding='unicode'),encoding='utf-8')
print('Prepared rest + two joint extremes, wind at 2x.')
