"""Deterministic, source-registered cutouts. Run with Pillow and numpy."""
from pathlib import Path
import json
import numpy as np
from PIL import Image, ImageDraw, ImageFilter

ROOT = Path(__file__).resolve().parent
SOURCE = Path('C:/Users/Usogui/Downloads/ChatGPT Image Sep 14, 2026, 08_59_00 AM.png')
im = Image.open(SOURCE).convert('RGB')
W, H = im.size
assert (W, H) == (1254, 1254), 'Masks are registered to this exact source.'
rgb = np.array(im)

def polygon(points):
    m = Image.new('L', im.size)
    ImageDraw.Draw(m).polygon(points, fill=255)
    return np.array(m)

BODY = [(514,568),(516,509),(526,481),(544,480),(569,506),(601,542),(644,542),(665,495),(685,473),(698,476),(720,511),(737,562),(750,608),(772,647),(791,675),(780,704),(757,728),(777,760),(793,815),(788,869),(777,904),(760,928),(557,934),(530,914),(509,883),(500,858),(516,800),(520,746),(495,712),(486,682),(495,646)]
TAIL = [(770,886),(787,866),(800,836),(803,808),(795,775),(802,744),(815,738),(834,746),(851,770),(860,793),(858,829),(847,863),(827,886),(803,902),(770,919),(750,907)]
PAW = [(517,733),(502,736),(490,742),(486,759),(492,774),(508,786),(523,785),(532,770),(532,746)]
EYE_L = [(536,643),(554,642),(568,650),(578,668),(581,687),(574,698),(560,708),(542,704),(529,694),(523,679),(525,662)]
EYE_R = [(659,642),(677,641),(693,650),(704,670),(705,689),(695,700),(675,708),(656,704),(642,694),(637,679),(641,659)]
CANOPY = [(252,547),(264,507),(305,458),(357,408),(414,359),(476,331),(546,304),(616,285),(620,255),(628,248),(637,252),(638,263),(633,286),(699,306),(766,336),(825,373),(879,424),(922,480),(954,534),(980,574),(972,579),(956,570),(905,589),(832,610),(817,609),(819,625),(812,627),(806,610),(752,609),(698,600),(631,598),(567,595),(497,595),(454,594),(448,610),(442,613),(439,604),(445,590),(393,584),(331,569),(271,548)]
HANDLE = [(516,741),(530,735),(526,775),(517,808),(511,819),(499,829),(482,826),(469,818),(460,802),(460,785),(465,771),(474,770),(482,779),(478,797),(484,806),(496,809),(501,795),(509,768)]
GROUND = [(117,896),(139,880),(160,893),(178,856),(192,862),(211,893),(294,894),(348,886),(401,887),(414,850),(423,859),(428,889),(476,887),(783,888),(844,890),(858,866),(871,880),(912,891),(936,866),(948,876),(964,899),(982,876),(997,866),(1011,891),(1051,890),(1088,908),(1054,940),(1041,1008),(1006,1032),(908,1038),(881,1024),(784,1040),(780,1068),(750,1074),(718,1055),(650,1040),(521,1047),(492,1071),(463,1048),(323,1031),(230,1015),(226,955),(176,927)]

def cut(mask, preserve_holes=False):
    # Remove the nearly white paper only inside the selected region.
    minimum = rgb.min(axis=2).astype(float)
    chroma = rgb.max(axis=2).astype(float) - minimum
    # Grey anti-aliasing belongs to black ink over paper, not opaque grey rims.
    a = np.where(chroma < 32, np.clip((249-minimum)/249,0,1), np.clip((249-minimum)/38,0,1)) * (mask / 255)
    if preserve_holes:
        solid = Image.fromarray(np.uint8((rgb.min(axis=2) < 150) & (mask > 0)) * 255)
        ImageDraw.floodfill(solid, (0, 0), 128)
        filled = Image.fromarray(np.uint8((np.array(solid) != 128) & (mask > 0)) * 255)
        interior = np.array(filled.filter(ImageFilter.MinFilter(9))) > 0
        a[interior] = 1
    out = np.dstack([rgb, np.uint8(a * 255)])
    # Undo the white paper matte at partially covered edge pixels.
    edge = (a > 0) & (a < 1)
    for c in range(3):
        out[..., c][edge] = np.uint8(np.clip((rgb[..., c][edge] - 255 * (1-a[edge])) / a[edge], 0, 255))
    out[a == 0, :3] = 0
    return Image.fromarray(out, 'RGBA')

layers = {}
body_mask = polygon(BODY)
# Whiskers are separate thin regions, retained from the source rather than redrawn.
for points in [[(435,655),(493,666),(501,678),(485,688),(455,700),(459,691),(486,679),(436,666)],[(466,712),(499,699),(507,707),(470,724)],[(750,657),(817,650),(820,665),(773,677),(814,682),(814,694),(774,687),(794,715),(790,723),(754,698)]]:
    body_mask = np.maximum(body_mask, polygon(points))
body_mask[rgb[...,0].astype(int)-rgb[...,2].astype(int)>32] = 0
body = cut(body_mask, True)
fur = tuple(int(v) for v in np.median(rgb[725:735,560:620].reshape(-1,3),axis=0)) + (255,)
d = ImageDraw.Draw(body)
for points in [EYE_L, EYE_R]:
    eye_cover = Image.fromarray(polygon(points)).filter(ImageFilter.MaxFilter(7))
    body.paste(fur, (0,0,W,H), eye_cover)
layers['cat-body'] = body
eyes_mask = np.maximum(polygon(EYE_L), polygon(EYE_R))
eyes = im.convert('RGBA')
eyes.putalpha(Image.fromarray(eyes_mask))
layers['eyes'] = eyes
tail = cut(polygon(TAIL), True)
under = Image.new('RGBA', im.size)
ImageDraw.Draw(under).ellipse((746,871,797,915),fill=fur)
under.alpha_composite(tail)
layers['tail'] = under
paw = cut(polygon(PAW), True)
layers['holding-paw'] = paw

canopy_mask = polygon(CANOPY)
umbrella = cut(canopy_mask)
# Restore only canopy occluded by the cat, using existing underside texture.
occluded = (polygon(BODY) > 0) & (canopy_mask > 0)
texture = rgb[512:548,320:470]
u = np.array(umbrella)
yy,xx = np.where(occluded)
u[yy,xx,:3] = texture[(yy-512)%texture.shape[0],(xx-320)%texture.shape[1]]
u[yy,xx,3] = 255
umbrella = Image.fromarray(u,'RGBA')
# Complete hidden shaft; it stays behind the body and under the holding paw.
shaft = Image.new('RGBA', im.size)
sd = ImageDraw.Draw(shaft)
sd.line([(616,446),(507,792)], fill=(17,17,18,255), width=11)
shaft.alpha_composite(umbrella)
shaft.alpha_composite(cut(polygon(HANDLE), True))
layers['umbrella'] = shaft

ground_mask = polygon(GROUND)
ground_mask[polygon(BODY)>0] = 0
ground_mask[polygon(TAIL)>0] = 0
ground = cut(ground_mask)
under = Image.new('RGBA', im.size)
gd = ImageDraw.Draw(under)
gd.ellipse((505,895,802,954), fill=(116,112,101,67))
for x,y,length in [(527,926,77),(619,931,103),(554,941,57),(687,946,75)]:
    gd.line((x,y,x+length,y),fill=(37,35,32,120),width=2)
under.alpha_composite(ground)
layers['puddle'] = under

OUT = ROOT / 'layers-v1'
OUT.mkdir(exist_ok=True)
SMALL = OUT / '256'
SMALL.mkdir(exist_ok=True)
for name, layer in layers.items():
    layer.save(OUT / f'{name}.png', optimize=True)
    layer.save(OUT / f'{name}.webp', lossless=True, method=6)
    layer.resize((256,256),Image.Resampling.LANCZOS).save(SMALL / f'{name}.webp', lossless=True, method=6)

order = ['puddle','umbrella','tail','cat-body','holding-paw','eyes']
composite = Image.new('RGBA', im.size)
for name in order: composite.alpha_composite(layers[name])
composite.save(OUT / 'assembled.png', optimize=True)
# Review on both light and dark surfaces to expose paper fringes.
review = Image.new('RGB',(1254*2,1254),(255,255,255))
review.paste((36,47,61),(1254,0,2508,1254))
review.paste(composite,(0,0),composite)
review.paste(composite,(1254,0),composite)
review.resize((1400,700),Image.Resampling.LANCZOS).save(ROOT/'review-v1.jpg',quality=93)
sheet=Image.new('RGB',(1200,800),(43,50,59))
for i,name in enumerate(order):
    small=layers[name].resize((400,400),Image.Resampling.LANCZOS)
    sheet.paste(small,((i%3)*400,(i//3)*400),small)
    ImageDraw.Draw(sheet).text(((i%3)*400+12,(i//3)*400+12),name,fill='white')
sheet.save(ROOT/'layers-contact-sheet.jpg',quality=93)
manifest={'canvas':[W,H],'source':str(SOURCE),'order':order,'pivots':{'cat-body':[634,918],'eyes':[617,675],'tail':[776,892],'umbrella':[512,764],'holding-paw':[520,763]},'layers':{n:{'file':f'{n}.webp','bbox':layers[n].getbbox(),'bytes':(OUT/f'{n}.webp').stat().st_size} for n in order}}
(OUT/'manifest.json').write_text(json.dumps(manifest,indent=2),encoding='utf-8')
print(json.dumps({'files':len(layers),'webp_bytes':sum(v['bytes'] for v in manifest['layers'].values()),'output':str(OUT)}))
