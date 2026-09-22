"""Read-only pixel comparison of reported screenshot crops to the source art."""
from pathlib import Path
import numpy as np
from PIL import Image

root = Path(__file__).resolve().parents[1]
source = Image.open(root / 'src/assets/fallendown/ruins-garden-v3.png').convert('RGB')
reports = [
    Path('C:/Users/Usogui/AppData/Local/Temp/codex-clipboard-70b01669-caee-40d5-92ac-82c164722d9b.png'),
    Path('C:/Users/Usogui/AppData/Local/Temp/codex-clipboard-6c30e8f7-fc78-4c8f-a7cd-ea822459e0a5.png'),
]
for report in reports:
    template = np.asarray(Image.open(report).convert('RGB'), dtype=np.float32)
    th, tw = template.shape[:2]
    best = None
    for scale in [.75, .81, .875, .935546875, 1, 1.1, 1.25]:
        for name, method in [('nearest', Image.Resampling.NEAREST), ('bilinear', Image.Resampling.BILINEAR)]:
            src = np.asarray(source.resize((round(1536*scale), round(1024*scale)), method), dtype=np.float32)
            h, w = src.shape[:2]
            x0, y0 = int(w*.45), int(h*.53)
            cw, ch = w-tw-x0+1, int(h*.82)-th-y0+1
            score = np.zeros((ch,cw), dtype=np.float32)
            for y in range(0,th,4):
                for x in range(0,tw,4):
                    delta = src[y0+y:y0+y+ch,x0+x:x0+x+cw] - template[y,x]
                    score += (delta*delta).sum(axis=2)
            yy,xx = np.unravel_index(score.argmin(), score.shape)
            error = float(score[yy,xx]/(len(range(0,th,4))*len(range(0,tw,4))*3))
            if best is None or error < best[0]:
                best = (error,scale,name,x0+int(xx),y0+int(yy))
    error,scale,name,x,y=best
    print(report.name, 'mse=',round(error,2),'scale=',scale,'filter=',name,'rendered_xy=',(x,y),'source_xy=',(round(x/scale,2),round(y/scale,2)), 'size=',(tw,th),flush=True)
