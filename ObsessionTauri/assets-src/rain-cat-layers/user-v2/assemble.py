"""Static registration proof for user-provided RGBA layers; originals untouched."""
from pathlib import Path
import json
import numpy as np
from PIL import Image, ImageDraw

ROOT = Path(__file__).resolve().parent
SIZE = (1254,1254)
FILES = {'body':'фывф.png','tail':'part 1.png','eyes':'part2.png','umbrella':'part3.png','paw':'part4.png','puddle':'part5.png'}
layers = {name:Image.open(ROOT/'originals'/file).convert('RGBA') for name,file in FILES.items()}
placed = {}
transforms = {}
for name,layer in layers.items():
    # Uniform scaling only: umbrella canvas differs; eyes were exported enlarged.
    scale, x, y = {'umbrella':(0.6,113,220),'eyes':(0.72,179,196)}.get(name,(1,0,0))
    canvas=Image.new('RGBA',SIZE)
    if scale!=1:
        layer=layer.resize((round(layer.width*scale),round(layer.height*scale)),Image.Resampling.LANCZOS)
    canvas.alpha_composite(layer,(x,y))
    placed[name]=canvas
    transforms[name]={'scale':scale,'offset':[x,y]}

order=['puddle','umbrella','tail','body','paw','eyes']
assembled=Image.new('RGBA',SIZE)
for name in order: assembled.alpha_composite(placed[name])
assembled.save(ROOT/'assembly-draft.png')
review=Image.new('RGB',(2508,1254),'white')
review.paste((43,56,70),(1254,0,2508,1254))
review.paste(assembled,(0,0),assembled)
review.paste(assembled,(1254,0),assembled)
review.resize((1400,700),Image.Resampling.LANCZOS).save(ROOT/'review-draft.jpg',quality=94)
(ROOT/'registration-draft.json').write_text(json.dumps({'canvas':SIZE,'order':order,'transforms':transforms,'status':'static registration draft, not animation-ready'},indent=2),encoding='utf8')
print('Static proof saved; original files unchanged.')
