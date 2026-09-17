import type { ObsessionVisualPhase } from "../../obsessionVisualState";
import {
  fillDisc,
  fillPolygon,
  hash,
  line,
  type PlaneIndex,
} from "../pixelart/pixelCore";

/**
 * Hand-authored reconstruction of `makeit.jpg` on a 450 × 336 pixel grid.
 * The reference is used only as a visual layout guide. No image pixels, browser
 * filters, gradients, fonts or source-image canvases participate in this scene.
 */
export const MAKE_IT_WIDTH = 450;
export const MAKE_IT_HEIGHT = 336;

export type MakeItSceneOptions = {
  isolate?: PlaneIndex | null;
};

const C = {
  sky0: "#f8e6bc",
  sky1: "#f3d8a5",
  sky2: "#e9c58e",
  haze0: "#cda873",
  haze1: "#ab8a5e",
  haze2: "#877a56",
  mountain: "#6f6b4c",
  ink: "#261b1c",
  ink2: "#352522",
  timber0: "#3a2724",
  timber1: "#50352a",
  timber2: "#6b4632",
  timber3: "#885a39",
  wood0: "#77503a",
  wood1: "#986843",
  wood2: "#b57f50",
  wood3: "#d09a62",
  plaster0: "#9a704d",
  plaster1: "#c09361",
  plaster2: "#deb77d",
  plaster3: "#f0d196",
  leaf0: "#2c3123",
  leaf1: "#40472c",
  leaf2: "#57623a",
  leaf3: "#717648",
  leaf4: "#8f9155",
  terracotta0: "#6b382b",
  terracotta1: "#965037",
  terracotta2: "#bd7048",
  terracotta3: "#d88e59",
  flower0: "#9d493e",
  flower1: "#d16c54",
  flower2: "#ee9b6f",
  cat0: "#221a20",
  cat1: "#2c2228",
  cat2: "#3e2d33",
  ear: "#a06a68",
  eye: "#fff1b7",
} as const;

type View = {
  readonly context: CanvasRenderingContext2D;
  readonly scale: number;
  readonly originX: number;
  x(value: number): number;
  y(value: number): number;
  size(value: number): number;
  rect(x: number, y: number, width: number, height: number, color: string): void;
  poly(points: readonly (readonly [number, number])[], color: string): void;
  line(x0: number, y0: number, x1: number, y1: number, color: string, thickness?: number): void;
  disc(x: number, y: number, rx: number, ry: number, color: string): void;
};

function sceneView(context: CanvasRenderingContext2D, width: number, height: number): View {
  const scale = height / MAKE_IT_HEIGHT;
  const originX = Math.round((width - MAKE_IT_WIDTH * scale) / 2);
  const x = (value: number) => Math.round(originX + value * scale);
  const y = (value: number) => Math.round(value * scale);
  const size = (value: number) => Math.max(1, Math.round(value * scale));
  return {
    context,
    scale,
    originX,
    x,
    y,
    size,
    rect(left, top, boxWidth, boxHeight, color) {
      context.fillStyle = color;
      const x0 = x(left), y0 = y(top), x1 = x(left + boxWidth), y1 = y(top + boxHeight);
      context.fillRect(x0, y0, Math.max(1, x1 - x0), Math.max(1, y1 - y0));
    },
    poly(points, color) {
      fillPolygon(context, points.map(([px, py]) => ({ x: x(px), y: y(py) })), color);
    },
    line(x0, y0, x1, y1, color, thickness = 1) {
      line(context, x(x0), y(y0), x(x1), y(y1), color, size(thickness));
    },
    disc(cx, cy, rx, ry, color) {
      fillDisc(context, x(cx), y(cy), size(rx), size(ry), color);
    },
  };
}

function drawSky(view: View, width: number, height: number) {
  const { context } = view;
  context.fillStyle = C.sky0;
  context.fillRect(0, 0, width, height);
  view.rect(244, 0, 206, 31, C.sky1);
  view.rect(260, 31, 190, 61, C.sky0);
  view.rect(276, 92, 174, 75, C.sky0);

  // Broad stepped cloud silhouettes from the reference's right-hand sky.
  view.poly([[270, 16], [278, 16], [278, 20], [285, 20], [285, 25], [293, 25], [293, 31], [270, 31]], C.sky2);
  view.poly([[330, 47], [346, 47], [346, 43], [358, 43], [358, 48], [369, 48], [369, 54], [379, 54], [379, 62], [330, 62]], C.sky1);
  view.poly([[407, 27], [419, 27], [419, 33], [428, 33], [428, 41], [439, 41], [439, 49], [450, 49], [450, 67], [407, 67]], C.sky1);
  view.poly([[365, 91], [382, 91], [382, 96], [395, 96], [395, 102], [409, 102], [409, 110], [365, 110]], C.sky2);

  for (const [px, py, length] of [[301, 29, 2], [321, 48, 4], [337, 80, 2], [390, 18, 2], [415, 73, 3], [435, 112, 3]] as const) {
    view.rect(px, py, length, 1, C.sky2);
  }
  // The checker-dither patch the reference keeps right of the eave shadow.
  for (let py = 86; py < 100; py += 1) {
    for (let px = 334 + (py % 2); px < 416; px += 4) view.rect(px, py, 1, 1, C.sky1);
  }
}

function drawDistantStreet(view: View) {
  // Mountain silhouettes: only behind the right house, as in the reference.
  view.poly([[368, 186], [392, 168], [410, 178], [432, 148], [450, 158], [450, 230], [368, 230]], C.mountain);
  view.poly([[380, 192], [400, 178], [418, 188], [436, 166], [450, 174], [450, 240], [380, 240]], C.haze2);

  // Chimney pipe over the roofs.
  view.rect(406, 126, 4, 18, C.haze2);
  view.rect(405, 124, 6, 3, C.haze1);

  // Distant block shapes stay low contrast so the foreground roof remains clear.
  const blocks = [
    [259, 191, 18, 78], [279, 199, 20, 69], [302, 203, 22, 66], [328, 198, 18, 73],
    [351, 207, 24, 62], [381, 196, 19, 75], [407, 204, 25, 67], [438, 194, 18, 77],
  ] as const;
  for (let index = 0; index < blocks.length; index += 1) {
    const [x, y, w, h] = blocks[index];
    view.rect(x, y, w, h, index % 2 ? C.haze1 : C.haze0);
    view.poly([[x, y], [x + w / 2, y - 5], [x + w, y]], index % 2 ? C.haze2 : C.haze1);
    for (let row = y + 10; row < y + h - 6; row += 11) {
      for (let col = x + 5; col < x + w - 4; col += 9) view.rect(col, row, 2, 2, C.haze2);
    }
  }
}

type Point = readonly [number, number];

/** Four measured corners keep tile seams on the same perspective plane. */
function roofPlane(view: View, a: Point, b: Point, c: Point, d: Point, rows: number, columns: number, seed: number) {
  const at = (u: number, v: number): Point => [
    (a[0] * (1-u) + b[0] * u) * (1-v) + (d[0] * (1-u) + c[0] * u) * v,
    (a[1] * (1-u) + b[1] * u) * (1-v) + (d[1] * (1-u) + c[1] * u) * v,
  ];
  view.poly([a,b,c,d], C.timber1);
  for (let row=0; row<rows; row++) {
    for (let col=-1; col<columns; col++) {
      const u0=Math.max(0,(col+(row%2)*0.5)/columns);
      const u1=Math.min(1,(col+1+(row%2)*0.5)/columns);
      if(u1<=u0) continue;
      const p=at(u0,row/rows), q=at(u1,row/rows);
      const r=at(u1,(row+0.88)/rows), s=at(u0,(row+0.88)/rows);
      const shade=hash(seed,row,col);
      view.poly([p,q,r,s],shade<0.25?C.wood0:shade<0.7?C.wood1:C.wood2);
      view.line(s[0]+1,s[1]-1,r[0]-1,r[1]-1,C.wood2);
      if(shade>0.62) view.line(p[0]+1,p[1]+1,s[0]+1,s[1]-2,C.wood3);
    }
  }
  view.line(d[0],d[1],c[0],c[1],C.timber1,2);
}

function drawRightHouse(view: View) {
  // The farther building shares the warm haze instead of foreground-black edges.
  const original = view;
  const colors: Record<string,string> = {
    [C.timber0]: "#624a36", [C.timber1]: "#72533c",
    [C.timber2]: "#7e5c41", [C.wood0]: "#896541",
    [C.wood1]: "#97734a", [C.wood2]: "#ad8857",
    [C.wood3]: "#bf9b69", [C.plaster1]: "#ae8958",
    [C.plaster2]: "#c19b64", [C.plaster3]: "#d7b57b",
  };
  view = {
    ...original,
    rect: (x,y,w,h,color) => original.rect(x,y,w,h,colors[color] ?? color),
    poly: (points,color) => original.poly(points,colors[color] ?? color),
    line: (x,y,bx,by,color,thickness) => original.line(x,y,bx,by,colors[color] ?? color,thickness),
    disc: (x,y,rx,ry,color) => original.disc(x,y,rx,ry,colors[color] ?? color),
  };
  // Street behind the receding side of the shop.
  view.poly([[384,336],[450,261],[450,336]],C.plaster2);
  for(let y=278;y<336;y+=6) {
    const x=450-(y-271)*0.83;
    view.line(x,y,x+8,y-2,C.haze0,2);
    view.line(x+15,y+1,x+23,y-1,C.wood2);
  }
  // Lower front is in shade; its right face catches the sky light.
  view.poly([[302,265],[390,254],[390,336],[301,336]],C.wood0);
  view.poly([[390,254],[435,245],[434,313],[390,336]],C.plaster2);
  view.poly([[393,266],[431,255],[430,266],[393,280]],C.plaster3);
  for(const x of [302,315,347,376,388]) view.rect(x,275,3,61,C.timber1);
  view.rect(319,288,23,48,C.timber1);
  view.rect(322,292,16,44,C.wood0);
  for(let x=323;x<338;x+=4) view.rect(x,294,1,41,C.timber2);
  view.rect(350,289,27,29,C.timber1);
  for(let x=354;x<376;x+=5) view.rect(x,292,2,23,C.wood2);
  for(const x of [395,413,430]) view.poly([[x,261-(x-390)*.25],[x+2,261-(x-390)*.25],[x+2,324-(x-390)*.4],[x,324-(x-390)*.4]],C.timber1);
  view.poly([[394,285],[426,276],[426,294],[394,307]],C.timber0);
  for(let x=398;x<426;x+=5) view.line(x,287-(x-394)*.3,x,304-(x-394)*.4,C.wood1);
  // Storefront awning casts a deep, narrow shadow.
  view.poly([[303,278],[381,269],[387,281],[309,291]],C.timber1);
  roofPlane(view,[306,277],[378,269],[383,281],[309,287],3,12,72);
  view.line(309,290,383,283,C.timber0,3);

  // Upper storey: front and receding side meet at x=391.
  view.poly([[315,184],[391,187],[391,241],[315,251]],C.plaster1);
  view.poly([[391,187],[432,180],[432,229],[391,241]],C.plaster2);
  view.poly([[315,188],[391,192],[391,202],[315,196]],C.wood0);
  view.poly([[392,192],[432,185],[432,194],[392,202]],C.wood1);
  for(const x of [317,386]) view.rect(x,190,4,53,C.wood0);
  view.poly([[393,191],[396,190],[396,238],[393,239]],C.wood0);
  view.poly([[427,185],[431,184],[431,229],[427,231]],C.wood0);
  view.rect(327,207,51,30,C.wood0);
  view.rect(331,211,43,22,C.timber1);
  for(let x=333;x<374;x+=6) view.rect(x,212,2,20,C.wood1);
  for(let y=216;y<232;y+=6) view.rect(332,y,42,2,C.wood2);
  view.line(328,238,379,237,C.timber2,2);
  view.poly([[401,201],[425,197],[425,219],[401,226]],C.wood0);
  view.poly([[404,205],[422,202],[422,217],[404,222]],C.timber1);
  for(let x=405;x<423;x+=4) view.line(x,205-(x-405)*.15,x,221-(x-405)*.25,C.wood2);
  view.line(400,229,426,222,C.timber2,2);
  roofPlane(view,[328,201],[378,201],[382,208],[325,207],2,10,61);

  // Gable plaster is not a tiled triangle. Tiles belong on the right roof plane.
  view.poly([[306,190],[353,143],[394,190]],C.timber1);
  view.poly([[316,186],[353,153],[385,186]],C.plaster1);
  view.line(353,155,353,184,C.wood0,3);
  view.line(338,170,351,184,C.wood0,2);
  view.line(368,168,356,184,C.wood0,2);
  view.line(320,187,386,187,C.wood0,3);
  roofPlane(view,[353,143],[393,151],[435,186],[393,192],10,9,41);
  view.line(303,192,353,143,C.timber1,4);
  view.line(305,190,353,145,C.wood0,2);
  view.line(351,142,395,150,C.wood2,2);
  view.line(394,193,436,186,C.wood0,3);
  // Lower roof also has two distinct planes and a visible fascia.
  roofPlane(view,[318,237],[392,231],[389,266],[296,276],7,10,52);
  roofPlane(view,[392,231],[430,219],[434,254],[389,266],7,6,53);
  view.line(296,278,390,268,C.timber1,3);
  view.line(390,268,435,256,C.wood0,3);
  view.line(298,276,389,266,C.wood2);
  for(const [x,y] of [[401,321],[414,314],[424,307]] as const) {
    view.rect(x,y,7,8,C.terracotta1);
    view.rect(x-1,y,9,2,C.wood0);
    view.disc(x+3,y-3,5,4,C.leaf2);
    view.rect(x+3,y-6,2,2,C.flower1);
  }
}

function drawWoodGrain(view: View, x: number, y: number, width: number, height: number, seed: number, light = false, density = 0.42) {
  // Unequal broken grain clusters, rather than evenly spaced vertical scratches.
  const count=Math.floor(width*height*density/54);
  for(let i=0;i<count;i++) {
    const px=x+1+Math.floor(hash(seed,i,1)*Math.max(1,width-4));
    const py=y+1+Math.floor(hash(seed,i,2)*Math.max(1,height-8));
    const length=2+Math.floor(hash(seed,i,3)*12);
    const bottom=Math.min(y+height-1,py+length);
    const shade=seed===47 ? (i%3===0 ? "#49342c" : "#3d2b26") : i%4===0?C.timber1:i%3===0?C.wood0:C.timber2;
    view.line(px,py,px,bottom,light?C.wood3:shade);
    if(i%3===0) {
      view.rect(px+1,py+2,Math.min(2,width-2),Math.max(1,(bottom-py)*.45),light?C.plaster1:shade);
      view.line(px+2,py+1,px+2,Math.min(bottom,py+5),seed===47 ? C.timber1 : C.wood0);
    }
    if(i%13===0) {
      view.poly([[px,py],[px+2,py+2],[px+1,py+5],[px-1,py+3]],C.timber1);
      view.rect(px,py+2,1,2,C.timber0);
    }
  }
}

function drawMainArchitecture(view: View, height: number) {
  const leftEdge=Math.max(0,view.x(0));
  if(leftEdge>0) {
    view.context.fillStyle=C.timber1;
    view.context.fillRect(0,0,leftEdge,height);
  }
  view.rect(-90,0,206,336,C.timber1);
  for(let x=-88;x<112;x+=12) {
    view.rect(x,0,10,336,x%3?C.wood0:C.timber2);
    view.line(x+1,0,x+1,336,C.timber0);
    drawWoodGrain(view,x,0,10,336,x+501,false,.8);
  }
  // Left window sits behind a shallow lean-to and a separate shelf.
  view.rect(0,46,57,137,C.timber0);
  view.rect(4,51,45,130,C.ink2);
  for(const x of [8,29,48]) {
    view.rect(x,51,4,130,C.wood0);
    view.rect(x+1,51,1,130,C.wood2);
  }
  view.poly([[-8,30],[57,23],[60,30],[47,48],[-8,48]],C.timber0);
  roofPlane(view,[-8,30],[57,24],[47,44],[-8,45],4,9,18);
  view.line(-8,47,48,47,C.timber0,3);
  view.rect(0,181,70,16,C.timber0);
  view.rect(0,182,69,8,C.wood1);
  view.line(0,182,68,182,C.wood3);
  for(let x=2;x<68;x+=9) view.rect(x,185+(x%3),5,2,C.timber2);
  view.poly([[47,197],[65,197],[64,216]],C.timber0);
  view.line(50,198,62,210,C.wood0,2);

  // Door recess, the two sunlit jambs, and the projecting right corner.
  view.rect(89,0,27,234,C.plaster2);
  view.rect(94,0,17,232,C.plaster3);
  view.rect(115,0,87,226,C.timber0);
  view.rect(122,17,75,208,C.ink2);
  view.rect(128,23,65,190,C.timber0);
  view.rect(178,24,14,188,C.timber1);
  view.rect(190,22,3,188,C.timber0);
  view.rect(115,5,7,222,C.wood0);
  view.rect(116,9,2,216,C.wood2);
  view.rect(197,17,13,205,C.wood0);
  view.rect(200,19,3,200,C.wood2);
  view.rect(208,0,53,216,C.plaster2);
  view.rect(214,18,43,195,C.plaster3);
  view.rect(211,19,4,196,C.plaster1);
  view.rect(258,0,8,214,C.wood0);
  view.rect(260,0,2,214,C.wood2);
  view.rect(265,0,3,210,C.timber0);
  // Top beams follow the reference eave, which ends to the right of the door.
  view.poly([[158,0],[281,0],[297,12],[298,18],[286,20],[198,26]],C.timber0);
  view.poly([[174,0],[279,0],[294,12],[285,15],[199,20]],C.wood0);
  view.line(175,1,199,15,C.wood2,3);
  view.line(199,15,285,18,C.wood1,2);
  view.line(199,22,288,21,C.ink,2);
  drawWoodGrain(view,123,25,74,184,47,false,.75);
  drawWoodGrain(view,196,23,13,192,51,false,.85);
  // Broken plaster marks use nearby ochres, rather than bright scratches.
  for(let i=0;i<90;i++) {
    const x=hash(57,i,0)>.5?94+hash(58,i,0)*17:215+hash(58,i,0)*40;
    const y=24+hash(59,i,0)*182;
    view.rect(x,y,1+Math.floor(hash(60,i,0)*2),2+hash(61,i,0)*4,i%3?C.plaster2:C.plaster1);
  }

  // Wall and corner below the ledge are drawn before the projecting shelf.
  view.poly([[98,235],[206,243],[286,224],[286,317],[176,336],[98,336]],C.timber1);
  view.poly([[177,246],[279,227],[279,313],[179,335]],C.wood1);
  for(let x=181;x<279;x+=10) {
    const top=245-(x-180)*.19, bottom=335-(x-180)*.21;
    view.poly([[x,top],[x+8,top-1.5],[x+8,bottom-1.5],[x,bottom]],x%3?C.wood1:C.wood2);
    view.line(x,top,x,bottom,C.timber2);
    drawWoodGrain(view,x,top+3,8,bottom-top-5,97+x,false,.9);
  }
  view.poly([[101,244],[174,251],[174,336],[100,336]],C.wood0);
  for(let x=104;x<175;x+=11) {
    view.rect(x,248,2,88,C.timber1);
    drawWoodGrain(view,x,250,9,83,x+71,false,.75);
  }
  view.poly([[104,260],[109,263],[163,252],[163,260],[105,316],[103,311]],C.timber0);
  view.line(108,306,162,257,C.wood1,3);
  view.poly([[242,243],[270,232],[243,269]],C.timber0);
  view.poly([[247,246],[263,239],[247,260]],C.timber2);
  view.poly([[281,227],[286,226],[286,317],[281,319]],C.timber0);
  view.line(282,233,282,314,C.wood2);
  view.poly([[176,333],[285,307],[300,307],[299,319],[226,336],[176,336]],C.timber0);
  view.poly([[179,331],[285,305],[297,307],[288,312],[200,336],[178,336]],C.wood1);
  view.line(183,332,291,308,C.wood2,2);

  // Measured ledge: receding rear edge, broad top, and a thick front fascia.
  view.poly([[96,228],[190,212],[297,209],[300,213],[299,225],[207,249],[96,241]],C.timber0);
  view.poly([[98,228],[190,214],[296,211],[297,218],[205,242],[98,235]],C.wood2);
  view.poly([[98,235],[205,242],[298,218],[298,224],[205,247],[98,240]],C.wood0);
  view.line(98,234,204,241,C.wood3);
  view.line(205,241,296,217,C.wood3,2);
  view.line(206,247,297,225,C.timber1,2);
  // Grain follows the plank perspective, with small grouped nicks.
  for(let i=0;i<165;i++) {
    const u=hash(201,i,0), v=hash(202,i,0);
    const x=100+u*194;
    const rear=x<190?228-(x-100)*.155:214-(x-190)*.027;
    const front=x<205?234+(x-100)*.064:241-(x-205)*.26;
    const y=rear+(front-rear)*v;
    view.line(x,y,Math.min(296,x+2+hash(203,i,0)*5),y-1,i%4===0?C.plaster2:i%3===0?C.timber2:C.wood1);
  }
}

function drawPot(view: View, x: number, y: number, width: number, height: number, pale = false) {
  const inset=Math.max(2,Math.round(width*.15));
  const dark=pale?C.wood0:C.terracotta0, body=pale?C.plaster1:C.terracotta2;
  view.poly([[x+1,y+2],[x+width-1,y+2],[x+width-inset,y+height],[x+inset,y+height]],dark);
  view.poly([[x+3,y+4],[x+width-3,y+4],[x+width-inset-1,y+height-2],[x+inset+1,y+height-2]],body);
  view.poly([[x+3,y+4],[x+width*.3,y+4],[x+width*.35,y+height-2],[x+inset+1,y+height-2]],pale?C.wood1:C.terracotta1);
  view.poly([[x+width*.7,y+5],[x+width-3,y+4],[x+width-inset-1,y+height-3],[x+width*.72,y+height-3]],pale?C.plaster2:C.terracotta3);
  view.rect(x,y,width,4,dark);
  view.rect(x+1,y+1,width-2,2,body);
  view.line(x+3,y-1,x+width-4,y-1,C.timber0,2);
  view.line(x+inset,y+height,x+width-inset,y+height,dark);
}

function drawIvyLeaf(view: View, x: number, y: number, size: number, flip: number, variant: number) {
  const shapes: readonly (readonly Point[])[] = [
    [[-5,-2],[-2,-3],[0,-6],[3,-4],[3,-1],[5,0],[3,4],[0,6],[-3,3]],
    [[-5,-3],[-1,-4],[2,-5],[5,-2],[4,2],[0,5],[-3,3]],
    [[-4,-4],[0,-3],[2,-6],[4,-3],[5,1],[2,5],[-1,4],[-4,1]],
  ];
  const map=(p:readonly Point[])=>p.map(([a,b])=>[x+a*size*flip,y+b*size] as const);
  const shape=shapes[Math.abs(variant)%3];
  view.poly(map(shape),C.ink);
  const fill=variant%5===0?C.leaf3:variant%3===0?C.leaf1:C.leaf2;
  view.poly(shape.map(([a,b])=>[x+a*size*.79*flip,y+b*size*.79] as const),fill);
  if(variant%3!==0) view.poly(map([[-3,-2],[-1,-3],[0,-4],[1,-2],[0,1],[-2,2]]),variant%5===0?C.leaf4:C.leaf3);
  if(size>1) view.line(x,y+3*size,x+size*flip,y-size,C.leaf1);
}

function drawVine(view: View, points: readonly (readonly [number, number])[], seed: number, leafSize = 0.8, wind = 0) {
  for (let index = 1; index < points.length; index += 1) {
    const [previousX, ay] = points[index - 1];
    const ax = previousX + (index > 3 ? wind : 0);
    const [bx, by] = points[index];
    const shiftedX = bx + (index > 2 ? wind : 0);
    view.line(ax, ay, shiftedX, by, C.leaf0);
    const distance = Math.max(1, Math.hypot(shiftedX - ax, by - ay));
    const leaves = Math.max(1, Math.floor(distance / 9));
    for (let leaf = 1; leaf <= leaves; leaf += 1) {
      const t = leaf / leaves;
      const x = ax + (shiftedX - ax) * t;
      const y = ay + (by - ay) * t;
      const flip = (seed + index + leaf) % 2 ? 1 : -1;
      view.line(x, y, x + flip * 4, y - 2, C.leaf1);
      drawIvyLeaf(view, x + flip * 5, y - 3, leafSize * (leaf % 3 === 0 ? 0.8 : 1), flip, seed + index + leaf);
    }
  }
}

/**
 * Плотная шапка плюща: кластеры листьев по хэшу. Именно плотные шапки, а не
 * одиночные плети, делают стену референса «заросшей».
 */
function drawIvyMass(view: View, x0: number, y0: number, x1: number, y1: number, seed: number, density: number) {
  for (let y = y0; y < y1; y += 7) {
    for (let x = x0; x < x1; x += 8) {
      if (hash(seed, x, y) > density) continue;
      const px = x + Math.floor(hash(seed + 1, x, y) * 5);
      const py = y + Math.floor(hash(seed + 2, x, y) * 5);
      drawIvyLeaf(view, px, py, 0.92 + hash(seed + 3, x, y) * 0.3, hash(seed + 4, x, y) > 0.5 ? 1 : -1, seed + x + y);
    }
  }
}

function drawFern(view: View, rootX: number, rootY: number, wind: number) {
  const tips = [[-38,-25],[-31,-38],[-20,-47],[-8,-52],[9,-48],[25,-53],[34,-32],[28,-20],[-32,-15],[-17,-32],[15,-30]] as const;
  for(let frond=0;frond<tips.length;frond++) {
    const [dx,dy]=tips[frond], tx=rootX+dx+wind*(frond%3===0?1:0), ty=rootY+dy;
    const mx=rootX+dx*.45, my=rootY+dy*.82;
    let previous:Point=[rootX,rootY];
    for(let step=1;step<=15;step++) {
      const t=step/15;
      const x=(1-t)**2*rootX+2*(1-t)*t*mx+t*t*tx;
      const y=(1-t)**2*rootY+2*(1-t)*t*my+t*t*ty;
      view.line(previous[0],previous[1],x,y,C.leaf1,2);
      view.line(previous[0],previous[1],x,y,C.leaf3);
      if(step>2 && step<15) {
        const vx=2*(1-t)*(mx-rootX)+2*t*(tx-mx), vy=2*(1-t)*(my-rootY)+2*t*(ty-my);
        const n=Math.hypot(vx,vy), ux=vx/n, uy=vy/n;
        const length=5*(1-t)+1;
        for(const side of [-1,1]) {
          const ex=x-uy*length*side+ux*3, ey=y+ux*length*side+uy*3;
          view.poly([[x,y],[ex-ux*2,ey-uy*2],[ex,ey],[x+ux*2,y+uy*2]],C.leaf0);
          view.line(x,y,ex,ey,frond%3===0?C.leaf3:C.leaf2,2);
          view.rect(ex,ey,1,1,frond%3===0?C.leaf4:C.leaf3);
        }
      }
      previous=[x,y];
    }
  }
}

function drawLeafyPot(view: View, x: number, y: number, width: number, height: number, seed: number) {
  drawPot(view, x, y, width, height);
  const cx = x + width / 2;
  const leafScale = 0.64 + Math.min(0.16, width / 150);
  for (let branch = 0; branch < 7; branch += 1) {
    const tipX = cx + (branch - 3) * width * 0.16;
    const tipY = y - 11 - (branch % 3) * 9;
    view.line(cx, y + 2, tipX, tipY, C.leaf0);
    drawIvyLeaf(view, tipX, tipY, leafScale + (branch % 2) * 0.08, branch % 2 ? 1 : -1, seed + branch);
    if (branch > 0 && branch < 6) drawIvyLeaf(view, (cx + tipX) / 2 + (branch % 2 ? 4 : -4), (y + tipY) / 2, leafScale * 0.82, branch % 2 ? -1 : 1, seed + branch + 10);
  }
  drawIvyLeaf(view, cx - width * 0.24, y - 3, leafScale, 1, seed + 21);
  drawIvyLeaf(view, cx + width * 0.22, y - 4, leafScale, -1, seed + 22);
}

function drawFlowers(view: View, x: number, y: number, scale: number, wind: number) {
  const stems = [[-9, -16], [0, -24], [10, -18], [5, -11]] as const;
  for (let index = 0; index < stems.length; index += 1) {
    const [dx, dy] = stems[index];
    const bx = x + dx * scale + (index % 2 ? wind : 0);
    const by = y + dy * scale;
    view.line(x, y, bx, by, C.leaf1);
    drawIvyLeaf(view, x + dx * 0.45 * scale + (index % 2 ? 4 : -4), y + dy * 0.45 * scale, 0.5 * scale, index % 2 ? 1 : -1, index + 41);
    view.disc(bx, by, 4 * scale, 3 * scale, C.flower0);
    view.rect(bx - 3 * scale, by - 2 * scale, 3 * scale, 2 * scale, C.flower2);
    view.rect(bx + scale, by - 3 * scale, 3 * scale, 2 * scale, C.flower1);
    view.rect(bx, by + scale, 3 * scale, 2 * scale, C.flower2);
  }
  for (const [dx, dy] of [[-8, -3], [-3, -7], [4, -7], [9, -3], [0, -2]] as const) drawIvyLeaf(view, x + dx * scale, y + dy * scale, 0.55 * scale, dx > 0 ? -1 : 1, dx + 53);
}

function drawPlants(view: View, wind: number) {
  // Canopies are grouped around the measured vines, with deliberate open wall.
  const canopy=(cx:number,cy:number,rx:number,ry:number,seed:number) => {
    for(let y=-ry;y<ry;y+=7) for(let x=-rx;x<rx;x+=7) {
      const edge = .67 + hash(seed,Math.floor(x/9),Math.floor(y/12))*.65;
      if((x/rx)**2+(y/ry)**2>edge) continue;
      const px=cx+x+hash(seed+1,x,y)*4, py=cy+y+hash(seed+2,x,y)*4;
      drawIvyLeaf(view,px+2,py+3,1.3,1,3);
      drawIvyLeaf(view,px,py,.9+hash(seed+3,x,y)*.6,x%2?1:-1,Math.floor(hash(seed+4,x,y)*20));
    }
  };
  for(const [x,y,rx,ry,seed] of [
    [64,18,14,25,20],[91,29,15,37,24],[83,76,11,24,27],
    [139,15,15,28,30],[153,48,17,22,34],[135,82,10,21,38],
    [229,16,18,24,42],[247,37,22,24,45],[237,65,13,19,47],
    [255,91,10,17,51],[30,234,26,33,56],[65,272,30,32,60],
    [20,313,25,27,63],[89,321,21,20,65],
  ] as const) canopy(x,y,rx,ry,seed);
  drawIvyMass(view,0,205,16,269,241,.8);
  drawVine(view,[[91,0],[97,30],[92,61],[99,87],[95,110],[101,127]],37,1,-wind);
  drawVine(view,[[134,22],[128,48],[135,71],[140,100],[153,124]],41,1,wind);
  drawVine(view,[[239,50],[226,73],[231,99]],49,1,wind);
  drawVine(view,[[258,67],[262,94],[258,119],[269,143],[265,158]],53,.95,-wind);
  drawVine(view,[[63,48],[65,76],[59,96]],57,.95,wind);
  drawVine(view,[[8,194],[23,213],[48,226],[66,248],[93,267]],59,1.2,wind);
  drawVine(view,[[67,199],[74,222],[83,243],[99,258]],63,1.1,-wind);

  // One rounded hanging pot, not a second invented planter.
  view.line(155,27,155,71,C.ink);
  view.line(146,68,149,75,C.timber0);
  view.poly([[148,72],[164,72],[168,80],[165,93],[157,97],[147,92],[145,82]],C.terracotta0);
  view.poly([[150,74],[162,74],[165,80],[162,92],[155,94],[148,89],[148,80]],C.terracotta1);
  view.poly([[160,76],[164,81],[161,91],[156,93],[156,88]],C.terracotta2);
  drawIvyLeaf(view,163,79,.8,-1,84);
  view.poly([[166,38],[171,38],[171,36],[174,38],[172,42],[171,47],[169,46],[169,42],[166,41]],C.wood2);

  drawLeafyPot(view,9,163,17,19,61);
  drawLeafyPot(view,34,162,24,20,67);
  // Rear fern first. Foreground pot overlaps its left foot, as in the reference.
  view.disc(163,214,18,2,C.timber1);
  drawPot(view,148,185,33,29);
  drawFern(view,164,185,wind);
  view.disc(124,227,18,2,C.timber1);
  drawLeafyPot(view,108,202,31,25,79);
  // Trailing ivy is beside the foreground pot, not across its whole body.
  drawVine(view,[[137,193],[145,204],[142,219],[153,235],[149,253],[158,271],[151,287],[164,307],[158,327]],81,1.05,wind);
  drawVine(view,[[154,223],[168,230],[186,233],[208,231]],83,.9,-wind);
  drawVine(view,[[158,241],[165,260],[162,279],[169,299],[172,324],[166,337]],87,.8,-wind);
  for(const [x,y] of [[143,201],[153,218],[149,239],[156,260],[161,283]] as const) {
    drawIvyLeaf(view,x,y,1.3,1,91+y);
    drawIvyLeaf(view,x+8,y+7,1,-1,92+y);
  }
  drawFlowers(view,415,320,.28,0);
}

function drawShopSign(view: View, sway: number) {
  const ox = 280 + sway;
  const oy = 58;
  view.line(266, 45, 331, 45, C.ink, 2);
  view.line(269, 43, 330, 43, C.timber2);
  view.line(330, 43, 335, 40, C.ink, 2);
  view.line(287, 44, 287 + sway, 58, C.ink, 2);
  view.line(319, 44, 319 + sway, 58, C.ink, 2);
  // Rounded dark frame with a mid-brown board face.
  view.poly([[ox + 3, oy], [ox + 51, oy], [ox + 54, oy + 3], [ox + 54, oy + 106], [ox + 51, oy + 109], [ox + 3, oy + 109], [ox, oy + 106], [ox, oy + 3]], C.ink2);
  view.poly([[ox + 4, oy + 4], [ox + 50, oy + 4], [ox + 50, oy + 105], [ox + 4, oy + 105]], C.wood0);
  view.rect(ox + 6, oy + 6, 42, 97, C.wood1);
  view.line(ox + 6, oy + 6, ox + 47, oy + 6, C.wood3);
  view.line(ox + 6, oy + 6, ox + 6, oy + 100, C.wood3);
  view.line(ox + 48, oy + 9, ox + 48, oy + 102, C.timber1);
  for (const [x, y, w, h] of [[10, 12, 2, 5], [28, 8, 1, 4], [44, 15, 2, 7], [8, 34, 2, 6], [46, 49, 2, 4], [9, 70, 1, 5], [42, 85, 2, 8], [17, 99, 5, 2], [36, 96, 3, 2]] as const) view.rect(ox + x, oy + y, w, h, C.timber2);

  const glyph = (x: number, y: number, strokes: readonly (readonly (readonly [number, number])[])[]) => {
    for (const stroke of strokes) {
      for (let index = 1; index < stroke.length; index += 1) {
        const [x0, y0] = stroke[index - 1], [x1, y1] = stroke[index];
        view.line(ox + x + x0, oy + y + y0, ox + x + x1, oy + y + y1, C.ink2, 2);
      }
    }
  };
  glyph(10, 15, [[[4, 0], [4, 19]], [[0, 7], [6, 5], [1, 14]], [[4, 10], [9, 6], [14, 6], [17, 9], [17, 14], [13, 18]]]);
  glyph(10, 39, [[[1, 0], [1, 8], [4, 14], [7, 9]], [[13, 1], [16, 5], [17, 10]]]);
  glyph(10, 61, [[[8, 0], [8, 2]], [[0, 3], [18, 3]], [[5, 5], [6, 8]], [[13, 5], [12, 8]], [[2, 9], [16, 9], [16, 23], [13, 23]], [[2, 9], [2, 23]], [[7, 10], [5, 13]], [[11, 10], [13, 13]], [[6, 16], [12, 16], [12, 21], [6, 21], [6, 16]]]);
  glyph(10, 88, [[[8, 0], [8, 2]], [[2, 3], [18, 3]], [[2, 3], [2, 12], [0, 18]], [[9, 6], [9, 11]], [[9, 8], [16, 8]], [[6, 12], [16, 12], [16, 19], [6, 19], [6, 12]]]);

  const small = [
    [[1, 0], [1, 6], [0, 7], [0, 4], [5, 4], [6, 6], [4, 8]],
    [[0, 1], [4, 0], [2, 4], [5, 4], [5, 7], [1, 7]],
    [[1, 1], [1, 6], [3, 7], [4, 5]],
    [[0, 4], [2, 1], [5, 1], [6, 4], [4, 7], [1, 6]],
    [[0, 1], [6, 1], [3, 1], [3, 8], [1, 6]],
    [[0, 2], [6, 2], [3, 0], [3, 7], [6, 7]],
  ] as const;
  small.forEach((stroke, index) => {
    for (let point = 1; point < stroke.length; point += 1) {
      view.line(ox + 40 + stroke[point - 1][0], oy + 14 + index * 13 + stroke[point - 1][1], ox + 40 + stroke[point][0], oy + 14 + index * 13 + stroke[point][1], C.ink2);
    }
  });
  view.rect(ox + 42, oy + 96, 6, 5, C.terracotta1);
}

function drawCat(view: View, phase: ObsessionVisualPhase, frame: number) {
  const tail=Math.round(Math.sin(frame/27));
  const breath=Math.sin(frame/22)>.82?1:0;
  const blink=frame%137===109 || frame%137===110;
  view.disc(222,219,22,2,C.timber1);
  view.poly([[211,216],[201,211],[195,203],[192,193],[192,180],[195+tail,171],[196+tail,164],[194+tail,160],[197+tail,159],[202+tail,163],[203+tail,169],[199,180],[198,190],[200,200],[206,206],[214,210]],C.cat0);
  view.line(197+tail,163,199+tail,168,C.cat2,2);
  view.line(195,181,195,192,C.cat1,2);
  // Small head, narrow chest, broad seated haunch, two separate front paws.
  view.poly([[230,169],[240,170],[241+breath,179],[239+breath,189],[238,200],[238,216],[240,218],[239,220],[233,220],[230,216],[230,219],[218,219],[209,215],[204,209],[202,201],[205,192],[211,184],[221,176]],C.cat0);
  view.poly([[224,178],[237,176],[237+breath,187],[232,203],[229,215],[220,216],[212,212],[208,205],[209,196],[215,186]],C.cat1);
  view.line(229,201,229,216,C.cat2);
  view.line(235,204,234,218,C.ink);
  view.rect(219,206,1,3,C.cat2);
  view.rect(222,211,1,3,C.cat2);
  view.poly([[220,150],[221,137],[223,136],[228,145],[242,145],[247,137],[249,138],[249,153],[251,159],[250,168],[247,172],[241,175],[230,174],[222,170],[219,164]],C.cat0);
  view.poly([[222,140],[225,144],[226,148],[222,149]],C.ear);
  view.poly([[245,147],[248,140],[247,149]],C.cat2);
  view.line(219,162,216,162,C.cat0);
  view.line(220,165,217,165,C.cat0);
  view.rect(252,164,1,1,C.cat0);
  if(blink) {
    view.line(226,160,233,161,C.cat2);
    view.line(243,161,247,160,C.cat2);
  } else {
    view.poly([[228,156],[232,155],[234,158],[234,163],[232,165],[228,164],[226,161],[226,158]],C.eye);
    view.poly([[244,156],[247,156],[248,159],[247,164],[244,164],[242,162],[242,158]],C.eye);
    view.poly([[231,157],[233,158],[233,162],[231,163],[230,161],[230,158]],C.cat0);
    view.rect(245,157,2,5,C.cat0);
    view.rect(231,157,1,1,C.eye);
    view.rect(245,157,1,1,C.eye);
    if(phase==="focused") view.rect(231,162,1,1,C.wood3);
  }
}

export function drawStaticMakeItScene(
  context: CanvasRenderingContext2D,
  width: number,
  height: number,
  options: MakeItSceneOptions = {},
) {
  const view = sceneView(context, width, height);
  const isolate = options.isolate ?? null;
  const visible = (plane: PlaneIndex) => isolate === null || isolate === plane;
  context.imageSmoothingEnabled = false;
  context.clearRect(0, 0, width, height);
  context.fillStyle = C.sky0;
  context.fillRect(0, 0, width, height);

  if (visible(0)) drawSky(view, width, height);
  if (visible(1)) drawDistantStreet(view);
  if (visible(2)) {
    drawRightHouse(view);
    drawMainArchitecture(view, height);
  }
}

export function drawDynamicMakeItScene(
  context: CanvasRenderingContext2D,
  width: number,
  height: number,
  phase: ObsessionVisualPhase,
  frame: number,
  options: MakeItSceneOptions = {},
) {
  const view = sceneView(context, width, height);
  const isolate = options.isolate ?? null;
  const wind = Math.round(Math.sin(frame / 29));
  if (isolate === null || isolate === 2) drawShopSign(view, Math.round(Math.sin(frame / 37)));
  if (isolate === null || isolate === 3) {
    drawPlants(view, wind);
    drawCat(view, phase, frame);
  }
}
