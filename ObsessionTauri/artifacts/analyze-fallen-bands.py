"""Compare screenshot bands to source pixels without modifying either image."""
from pathlib import Path
import numpy as np
from PIL import Image
root=Path(__file__).resolve().parents[1]
src=np.asarray(Image.open(root/'src/assets/fallendown/ruins-garden-v3.png').convert('RGB').resize((1437,958),Image.Resampling.BILINEAR),dtype=float)
for number,x,y in [(1,1123,585),(2,960,570)]:
    t=np.asarray(Image.open(root/f'artifacts/fallen-report-{number}.png').convert('RGB'),dtype=float)
    h,w=t.shape[:2]
    print('REPORT',number,flush=True)
    for row in range(0,h-3,3):
        part=t[row:row+3,3:w-3]
        best=None
        for dy in range(-4,5):
            for dx in range(-4,5):
                reference=src[y+row+dy:y+row+dy+3,x+3+dx:x+w-3+dx]
                delta=part-reference
                bias=delta.mean(axis=(0,1))
                error=((delta-bias)**2).mean()
                if best is None or error<best[0]:best=(error,dx,dy,bias.round(1))
        print('row',row,'global',y+row,'error',round(best[0],1),'offset',best[1:3],'RGB',best[3],flush=True)
