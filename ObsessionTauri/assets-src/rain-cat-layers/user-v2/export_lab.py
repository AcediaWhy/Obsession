"""Export the approved registration without modifying supplied artwork."""
from pathlib import Path
import json
from PIL import Image
root=Path(__file__).resolve().parent
out=root.parents[2]/'public'/'lab-assets'/'rain-cat-user-v2'
out.mkdir(parents=True,exist_ok=True)
registration=json.loads((root/'registration-draft.json').read_text())
files={'body':'фывф.png','tail':'part 1.png','eyes':'part2.png','umbrella':'part3.png','paw':'part4.png','puddle':'part5.png'}
assembled=Image.new('RGBA',(1254,1254))
for name in registration['order']:
    layer=Image.open(root/'originals'/files[name]).convert('RGBA')
    t=registration['transforms'][name]
    if t['scale']!=1:
        layer=layer.resize((round(layer.width*t['scale']),round(layer.height*t['scale'])),Image.Resampling.LANCZOS)
    placed=Image.new('RGBA',(1254,1254))
    placed.alpha_composite(layer,tuple(t['offset']))
    assembled.alpha_composite(placed)
    placed.resize((512,512),Image.Resampling.LANCZOS).save(out/f'{name}.webp',lossless=True,method=6)
assembled.resize((512,512),Image.Resampling.LANCZOS).save(out/'still.webp',lossless=True,method=6)
print(out)
print('Six layers bytes:',sum((out/f'{name}.webp').stat().st_size for name in files))
