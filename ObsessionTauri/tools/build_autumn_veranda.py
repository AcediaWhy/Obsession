"""Build the lab's original vector scenery; no raster processing or remote assets."""
from pathlib import Path
import random
import math
import re

random.seed(23)
root = Path(__file__).resolve().parents[1]
s = ['''<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1600 1000" preserveAspectRatio="xMidYMid slice" class="av-veranda" aria-hidden="true">
<defs>
 <linearGradient id="av-sky" x2="0" y2="1"><stop stop-color="#252736"/><stop offset=".55" stop-color="#795158"/><stop offset="1" stop-color="#dc9971"/></linearGradient>
 <linearGradient id="av-ground" x2="0" y2="1"><stop stop-color="#49363b"/><stop offset="1" stop-color="#171923"/></linearGradient>
 <linearGradient id="av-wood" x2=".15" y2="1"><stop stop-color="#9b6550"/><stop offset=".28" stop-color="#624236"/><stop offset="1" stop-color="#2f252b"/></linearGradient>
 <linearGradient id="av-metal" x2="1" y2=".1"><stop stop-color="#171c25"/><stop offset=".42" stop-color="#775c48"/><stop offset=".55" stop-color="#292c31"/><stop offset="1" stop-color="#151921"/></linearGradient>
 <radialGradient id="av-pumpkin" cx=".32" cy=".22" r=".83"><stop stop-color="#eca26b"/><stop offset=".4" stop-color="#be6744"/><stop offset=".8" stop-color="#784037"/><stop offset="1" stop-color="#422c32"/></radialGradient>
 <radialGradient id="av-rib" cx=".3" cy=".2" r=".85"><stop stop-color="#f1b17b"/><stop offset=".42" stop-color="#c4774c"/><stop offset="1" stop-color="#653738"/></radialGradient>
 <radialGradient id="av-glow"><stop stop-color="#ffd48e" stop-opacity=".46"/><stop offset=".35" stop-color="#f8ac62" stop-opacity=".17"/><stop offset="1" stop-color="#e18749" stop-opacity="0"/></radialGradient>
 <linearGradient id="av-glass" x2="1" y2="1"><stop stop-color="#fbd398" stop-opacity=".7"/><stop offset=".48" stop-color="#ce8547" stop-opacity=".2"/><stop offset="1" stop-color="#ffcf80" stop-opacity=".58"/></linearGradient>
 <radialGradient id="av-leaf" cx=".3" cy=".2"><stop stop-color="#d39460"/><stop offset="1" stop-color="#793c3b"/></radialGradient>
 <pattern id="av-grain" width="240" height="140" patternUnits="userSpaceOnUse">
 <path d="M0 9Q72 2 129 11T240 9M0 17Q71 10 138 19T240 18M0 52Q74 57 138 50T240 55M0 112Q85 120 150 108T240 114M0 126Q92 121 155 129T240 123" stroke="#e9b581" stroke-opacity=".13" fill="none"/>
 <path d="M0 35Q45 29 77 38T144 37T240 34M0 91Q39 85 94 95T240 89M0 133Q120 141 240 133" stroke="#140f17" stroke-opacity=".4" fill="none"/>
 <path d="M63 72Q86 55 117 72Q89 87 63 72ZM77 72Q89 65 104 72Q92 78 77 72Z" stroke="#251921" stroke-opacity=".35" fill="none"/>
 </pattern>
 <filter id="av-soft"><feGaussianBlur stdDeviation="5"/></filter>
 <filter id="av-shadow" x="-40%" y="-40%" width="180%" height="200%"><feDropShadow dy="12" stdDeviation="10" flood-color="#090c15" flood-opacity=".55"/></filter>
 <g id="av-oak"><path d="M0 0C-13-4-24-17-13-19C-27-28-22-37-10-33C-17-48-6-55 0-64C8-52 18-47 12-35C26-40 29-29 15-21C28-18 17-5 0 0Z" fill="url(#av-leaf)" stroke="#663536" stroke-width="1.2"/><path d="M0 6V-54M0-13L-13-23M0-24L12-34M0-36L-9-45" fill="none" stroke="#f3b576" stroke-opacity=".35" stroke-width="1"/></g>
 <g id="av-maple"><path d="M0 0L-19-9L-15-18L-31-31L-17-30L-18-43L-6-35L0-59L7-35L20-43L18-28L32-32L18-17L20-8Z" fill="url(#av-leaf)" stroke="#703b38"/><path d="M0 7V-46M0-8L-19-28M0-8L19-28" stroke="#efb277" stroke-opacity=".4" fill="none"/></g>
</defs>
<rect width="1600" height="1000" fill="url(#av-sky)"/>
<circle cx="1130" cy="196" r="165" fill="url(#av-glow)"/>
<circle cx="1130" cy="196" r="37" fill="#f7d6b4" opacity=".7"/>
<g class="av-clouds" fill="#cab0a0" opacity=".13" filter="url(#av-soft)"><path d="M550 172Q750 136 950 175T1420 173L1420 189H550Z"/><path d="M150 296Q450 257 730 290T1360 274L1360 303H150Z"/></g>
''']

# Distant woodland: varied trunks, branching and clustered crowns.
for layer in range(3):
    s.append(f'<g opacity="{.35 + layer*.17}">')
    for i in range(26):
        x=i*70+random.randint(-35,35); y=410+layer*65+random.randint(-40,20)
        h=random.randint(95,190); col=random.choice(['#644247','#80504a','#93614d'] if layer==0 else ['#392e38','#45313a','#51373b'])
        s.append(f'<path d="M{x} 686V{y-h}M{x} {y-30}l-24-55M{x} {y-5}l32-71" stroke="#332b34" stroke-width="{3+layer}"/>')
        for j in range(5):
            cx=x+random.randint(-33,33); cy=y-h+random.randint(0,75)
            rx=random.randint(26,47); ry=random.randint(25,47)
            crown=[]
            for a in range(32):
                theta=a*math.tau/32
                r=1+.13*math.sin(a*2.5)+.07*math.cos(a*1.6+j)
                crown.append(f'{cx+math.cos(theta)*rx*r:.1f},{cy+math.sin(theta)*ry*r:.1f}')
            s.append(f'<path d="M{"L".join(crown)}Z" fill="{col}"/>')
            for a in range(4):
                tx=cx+math.sin(a*2.4+j)*rx*.6; ty=cy+math.cos(a*2.4+j)*ry*.6
                s.append(f'<path d="M{tx:.1f} {ty:.1f}q5-6 10-2m-3 7q5-4 10-1" stroke="#d6a080" stroke-opacity=".12" stroke-width="2" fill="none"/>')
    s.append('</g>')
s.append('<path d="M0 600Q380 530 750 603T1600 575V1000H0Z" fill="url(#av-ground)"/>')
# Porch deck with perspective joints, wood grain and fasteners.
s.append('<path d="M0 721L1600 668V1000H0Z" fill="url(#av-wood)"/>')
for y in [722,758,805,866,942]:
    s.append(f'<path d="M0 {y}L1600 {y-54}" stroke="#17171f" stroke-width="5"/><path d="M0 {y+5}L1600 {y-49}" stroke="#d09a6d" stroke-opacity=".25" stroke-width="2"/>')
s.append('<path d="M0 721L1600 668V1000H0Z" fill="url(#av-grain)"/>')
for x,y in [(270,752),(640,793),(1040,851),(415,927),(1260,972)]:
    s.append(f'<path d="M{x} {y-27}l4 50" stroke="#211b22" stroke-width="3"/><circle cx="{x-10}" cy="{y-17}" r="2.5" fill="#161820"/><circle cx="{x+15}" cy="{y+13}" r="2.5" fill="#161820"/>')
# Rail, balusters, bolts, light edges.
for x in range(50,1620,115):
    s.append(f'<path d="M{x} 557V728h18V557Z" fill="#332930"/><path d="M{x+18} 557V728" stroke="#9c6b51" stroke-opacity=".5" stroke-width="3"/>')
s.append('<path d="M0 543L1600 503V531L0 571Z" fill="#614438"/><path d="M0 543L1600 503V511L0 551Z" fill="#b17e58"/><path d="M0 558L1600 519" stroke="#251e27" stroke-width="5"/><path d="M0 700L1600 656V670L0 714Z" fill="#4d3731"/>')
for x in [148,1402]:
    s.append(f'<path d="M{x} 491h42v260h-42Z" fill="url(#av-wood)"/><path d="M{x} 491h42v260h-42Z" fill="url(#av-grain)"/><path d="M{x-8} 479h58v17h-58Z" fill="#76523e"/><path d="M{x+5} 502V735" stroke="#c28a60" stroke-opacity=".3" stroke-width="3"/>')
# Ground leaves cast short shadows and carry actual veins.
for i in range(55):
    x=random.randint(110,1500); y=random.randint(758,995); angle=random.randint(0,360); scale=random.uniform(.17,.44)
    s.append(f'<g transform="translate({x} {y}) rotate({angle}) scale({scale:.2f})"><use href="#av-maple" transform="translate(5 7)" opacity=".22"/><use href="#av-maple"/></g>')

# Group of pumpkins, each with a coherent curved rib system and skin marks.
for x,y,k in [(1260,813,.74),(335,875,.74),(1430,882,1.12),(1160,940,.6)]:
    s.append(f'<g transform="translate({x} {y}) scale({k})"><ellipse cy="81" rx="119" ry="23" fill="#10131d" opacity=".75" filter="url(#av-soft)"/><g filter="url(#av-shadow)">')
    s.append('<path d="M-11-68Q-26-115-7-125L10-120Q-5-101 12-68Z" fill="#55513a" stroke="#2c312b" stroke-width="3"/><path d="M-6-80Q-14-105 0-118" stroke="#aaa173" stroke-width="3" fill="none"/>')
    s.append('<path d="M0-68C-42-96-115-63-119 0C-127 63-58 94 0 87C62 96 123 61 117 0C112-62 50-99 0-68Z" fill="url(#av-pumpkin)"/>')
    for offset in [-66,-34,0,34,66]:
        s.append(f'<ellipse cx="{offset}" cy="7" rx="{36 if offset else 33}" ry="79" fill="url(#av-rib)" opacity="{.6 if offset else .95}"/>')
    for offset in [-75,-42,0,42,75]:
        s.append(f'<path d="M{offset*.22} -69Q{offset*1.65} -30 {offset} 50Q{offset*.8} 77 {offset*.34} 86" fill="none" stroke="#542f30" stroke-opacity=".38" stroke-width="2.5"/>')
    for i in range(85):
        px=random.uniform(-97,97); py=random.uniform(-54,62)
        if (px/104)**2+(py/75)**2 < 1:
            s.append(f'<path d="M{px:.1f} {py:.1f}l.7 {random.uniform(1,3):.1f}" stroke="{random.choice(["#ffcd8c","#623833"])}" opacity=".22" stroke-width="1.2"/>')
    s.append('</g></g>')
# Lantern in visible lower third; warm spill shares its physical position.
s.append('''<g transform="translate(966 783)">
 <ellipse class="av-light" cy="71" rx="263" ry="89" fill="url(#av-glow)"/>
 <ellipse cy="70" rx="53" ry="12" fill="#0a111a" opacity=".8"/>
 <circle class="av-light" cy="-8" r="145" fill="url(#av-glow)"/>
 <g class="av-lantern" filter="url(#av-shadow)">
 <path d="M-21-77C-42-142 43-142 22-77" fill="none" stroke="#1e232b" stroke-width="7"/>
 <path d="M-23-77C-39-137 40-137 24-77" fill="none" stroke="#a47b53" stroke-width="1.5"/>
 <path d="M-35-68L-49 61H49L35-68Z" fill="url(#av-glass)" stroke="#352b28" stroke-width="4"/>
 <path d="M-30-54L-39 40L-22 34L-15-54Z" fill="#ffe0ad" opacity=".13"/>
 <rect x="-15" y="9" width="30" height="42" rx="4" fill="#d6b388"/><path d="M0 10V-1" stroke="#482a27" stroke-width="2"/>
 <path class="av-flame" d="M0 5C-27-6-8-25 1-42C3-22 20-7 0 5Z" fill="#ffc472"/>
 <path class="av-flame" d="M0 4C-10-3-4-15 0-21C6-10 10-2 0 4Z" fill="#fff4c2"/>
 <path d="M-44-72H44L34-93H-34Z M-52 57H52L47 73H-47Z" fill="url(#av-metal)" stroke="#99704a" stroke-width="2"/>
 <path d="M-32-70L-43 57M32-70L43 57M0-70V-50" stroke="url(#av-metal)" stroke-width="7"/>
 <path d="M-41-66H41M-46 58H46" stroke="#d8a06a" stroke-opacity=".7" stroke-width="2"/>
 </g></g>''')
# Foreground tree framing with bark and articulated branches, not floating blobs.
for flip in [False,True]:
    s.append(f'<g transform="{ "translate(1600 0) scale(-1 1)" if flip else "translate(0 0)"}">')
    s.append('<path d="M0 0H105Q65 273 96 497L135 1000H0Z" fill="#262027"/><path d="M29 0Q8 242 53 537L78 1000M64 0Q43 270 76 516" fill="none" stroke="#694737" stroke-width="5" opacity=".55"/><path d="M59 85Q258 67 493-31M76 176Q197 193 339 99M53 71Q167 38 266 0" stroke="#34262a" stroke-width="18" stroke-linecap="round" fill="none"/>')
    for i in range(35):
        x=random.randint(110,475); y=random.randint(-10,190); scale=random.uniform(.62,1.13); ang=random.randint(-100,110)
        s.append(f'<path d="M{int(x*.56)} {int(y*.35)}Q{x-40} {y+8} {x} {y}" stroke="#533c31" stroke-width="1.8" fill="none"/>')
        s.append(f'<g transform="translate({x} {y}) rotate({ang}) scale({scale:.2f})"><g class="av-bough" style="animation-delay:-{i%8}s;animation-duration:{5+i%5}s"><use href="#av-oak"/></g></g>')
    s.append('</g>')
s.append('</svg>')
svg='\n'.join(s)
(root/'public/lab-assets/autumn-velvet/scene.svg').write_text(svg,encoding='utf-8')
html_path=root/'autumn-velvet-lab.html'
html=html_path.read_text(encoding='utf-8')
html=re.sub(r'<div class="autumn-velvet-scene__art">.*?</div>',lambda m:'<div class="autumn-velvet-scene__art">'+svg+'</div>',html,flags=re.S)
if 'av-preview-tools' not in html:
    html=html.replace('<div id="root"></div>','''<div class="av-preview-tools">
      <span>Autumn Velvet · веранда</span>
      <label><input type="checkbox" id="av-scene-only"/> Только фон</label>
      <label><input type="checkbox" id="av-pause"/> Пауза</label>
    </div>
    <div id="root"></div>''')
html_path.write_text(html,encoding='utf-8')
print(f'Veranda SVG: {len(svg.encode()):,} bytes')
