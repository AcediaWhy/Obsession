from pathlib import Path
import xml.etree.ElementTree as E
import re, copy

ns = 'http://www.w3.org/2000/svg'
E.register_namespace('', ns)
source = E.parse(Path.home() / 'Downloads/cattts.svg').getroot()
paths = source.findall(f'{{{ns}}}path')
out = E.Element(f'{{{ns}}}svg', {'viewBox':'300 270 420 540','width':'840','height':'1080'})
E.SubElement(out, f'{{{ns}}}rect', {'x':'300','y':'270','width':'420','height':'540','fill':'#d6b093'})
for i in [4,5,6,7,8,9,10,1121,1150,1151,1152,1153,1334]:
    out.append(copy.deepcopy(paths[i]))
for y in range(300,801,50):
    E.SubElement(out,f'{{{ns}}}path',{'d':f'M300 {y}H720','stroke':'#9ca','stroke-width':'.3'})
    t=E.SubElement(out,f'{{{ns}}}text',{'x':'301','y':str(y),'font-size':'8'}); t.text=str(y)
for x in range(350,701,50):
    E.SubElement(out,f'{{{ns}}}path',{'d':f'M{x} 270V810','stroke':'#9ca','stroke-width':'.3'})
    t=E.SubElement(out,f'{{{ns}}}text',{'x':str(x),'y':'280','font-size':'8'}); t.text=str(x)
for idx in [1121,4]:
    pts=[]
    for cmd,arg in re.findall(r'([MLCZ])([^MLCZ]*)',paths[idx].get('d')):
        a=[float(v) for v in re.findall(r'-?\d*\.?\d+(?:e-?\d+)?',arg)]
        n=6 if cmd=='C' else 2
        pts.extend(tuple(a[k+n-2:k+n]) for k in range(0,len(a),n))
    for k,(x,y) in enumerate(pts):
        if k% (8 if idx==1121 else 3): continue
        E.SubElement(out,f'{{{ns}}}circle',{'cx':str(x/2),'cy':str(y/2),'r':'1.1','fill':'cyan'})
        t=E.SubElement(out,f'{{{ns}}}text',{'x':str(x/2+2),'y':str(y/2),'font-size':'5','fill':'#00ffff'}); t.text=str(k)
    print(idx, 'vertices',len(pts))
Path(__file__).with_name('cattts-parts-map.svg').write_text(E.tostring(out,encoding='unicode'),encoding='utf-8')
