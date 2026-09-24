import type { ObsessionVisualPhase } from "../obsessionVisualState";
import {
  drawCityCarpet,
  drawCityPulse,
  drawClouds,
  drawMoon,
  drawNearBlocks,
  drawSky,
  drawSkylineMasts,
  drawStars,
  drawViaduct,
} from "./pixelart/cityLayers";
import {
  ditherOver,
  hash,
  type PlaneIndex,
  plane,
  RAMP,
  ramp,
  SOLID,
  WARM,
  withPlane,
} from "./pixelart/pixelCore";
import { capAt, type RoofGeometry, roofGeometry } from "./pixelart/roofGeometry";
import {
  drawDeck,
  drawFlyingLeaves,
  drawLeafDrifts,
  drawMilkBowl,
  drawParapet,
  drawPuddle,
  drawRoofProps,
  drawShed,
  drawVentSteam,
} from "./pixelart/rooftopParts";
import { drawRoofCatSprite, ROOF_CAT_GROUND, ROOF_CAT_SIZE } from "./rooftopCatSprite";

// Четыре плана сцены: небо с луной, дальний город, ближние здания и крыша.
// Парапет, кот и лужа находятся в нижней части кадра, свободной от панелей.
// Дальние планы светлее ближних; основной источник света — луна.

export type RoofSceneOptions = {
  /** Инспектор: показать только один план глубины. */
  isolate?: PlaneIndex | null;
};

function sceneSeed(width: number, height: number) {
  return 0x517a11 ^ Math.imul(width, 2437) ^ Math.imul(height, 6151);
}

// Кот рендерится в свой буфер и вклеивается в мир: у кнопки-ядра и у крыши один и тот же
// кот, а не два похожих.
let catBuffer: HTMLCanvasElement | null = null;
let catContext: CanvasRenderingContext2D | null = null;

function catSprite(phase: ObsessionVisualPhase, frame: number) {
  if (typeof document === "undefined") return null;
  if (!catBuffer) {
    catBuffer = document.createElement("canvas");
    catBuffer.width = ROOF_CAT_SIZE;
    catBuffer.height = ROOF_CAT_SIZE;
    catContext = catBuffer.getContext("2d");
    if (catContext) catContext.imageSmoothingEnabled = false;
  }
  if (!catContext) return null;
  drawRoofCatSprite(catContext, phase, frame, "hero");
  return catBuffer;
}

export function drawStaticRoofScene(
  context: CanvasRenderingContext2D,
  width: number,
  height: number,
  options: RoofSceneOptions = {},
) {
  const seed = sceneSeed(width, height);
  const geometry = roofGeometry(width, height);
  const isolate = options.isolate ?? null;
  const visible = (index: PlaneIndex) => isolate === null || isolate === index;

  context.imageSmoothingEnabled = false;
  context.fillStyle = plane(0, RAMP.night[0]);
  context.fillRect(0, 0, width, height);

  if (visible(0)) {
    withPlane(0, () => {
      drawSky(context, geometry);
      drawStars(context, geometry, seed + 3);
      drawClouds(context, geometry, seed + 7);
      drawMoon(context, geometry);
    });
  }
  if (visible(1)) {
    withPlane(1, () => {
      drawCityCarpet(context, geometry, seed + 11);
      drawSkylineMasts(context, geometry, seed + 13);
    });
  }
  if (visible(2)) {
    withPlane(2, () => {
      drawViaduct(context, geometry, seed + 17);
      drawNearBlocks(context, geometry, seed + 19);
    });
  }
  if (visible(3)) {
    withPlane(3, () => {
      drawDeck(context, geometry, seed + 23);
      drawParapet(context, geometry, seed + 29);
      drawShed(context, geometry, seed + 31);
      drawRoofProps(context, geometry, seed + 37);
      drawLeafDrifts(context, geometry, seed + 43);
    });
  }
}

/** Порыв ветра: наклонная полоса дизеринга, идущая ВДОЛЬ парапета. */
function drawGust(context: CanvasRenderingContext2D, geometry: RoofGeometry, frame: number) {
  const { width, height } = geometry;
  const sweep = ((frame * 5) % (width + 160)) - 80;
  for (let x = 0; x < width; x += 1) {
    const top = capAt(geometry, x) + 6;
    if (Math.abs(x - sweep) > 14) continue;
    for (let y = top; y < height; y += 3) ditherOver(context, x, y, 1, 2, ramp(SOLID, 2), 0.4);
  }
}

export function drawDynamicRoofScene(
  context: CanvasRenderingContext2D,
  width: number,
  height: number,
  phase: ObsessionVisualPhase,
  frame: number,
  options: RoofSceneOptions = {},
) {
  const seed = sceneSeed(width, height);
  const geometry = roofGeometry(width, height);
  const isolate = options.isolate ?? null;

  if (isolate === null || isolate === 1) withPlane(1, () => drawCityPulse(context, geometry, frame, seed + 17));

  if (isolate !== null && isolate !== 3) return;

  withPlane(3, () => {
    drawPuddle(context, geometry, frame, seed + 47);
    drawVentSteam(context, geometry, frame);

    // Кот сидит на КРЫШКЕ парапета: лапы обязаны лечь ровно на её пиксель, иначе он
    // висит в воздухе, а тень внутри спрайта уезжает от лап.
    const cat = catSprite(phase, frame);
    if (cat) {
      context.drawImage(
        cat,
        Math.round(geometry.catX - ROOF_CAT_SIZE / 2),
        Math.round(geometry.catFeet - ROOF_CAT_GROUND),
        ROOF_CAT_SIZE,
        ROOF_CAT_SIZE,
      );
    }
    drawMilkBowl(context, geometry, frame);
    drawFlyingLeaves(context, geometry, frame, seed + 59);

    if (phase === "scanning") drawGust(context, geometry, frame);
    if (phase === "focused") {
      // Поток держится: тёплые блики по кромке парапета вокруг кота.
      for (let glint = 0; glint < 7; glint += 1) {
        const x = Math.round(geometry.catX - 34 + glint * 11 + Math.sin(frame * 0.15 + glint) * 2);
        context.fillStyle = ramp(WARM, glint % 3 === 0 ? 3 : 2);
        context.fillRect(x, capAt(geometry, x), 2, 1);
      }
    }
    if (phase === "fault") {
      // Помехи: жёсткие обрывки строк по настилу, как сбой на плёнке.
      const gust = Math.floor(frame / 4);
      for (let streak = 0; streak < 7; streak += 1) {
        const x = Math.floor(hash(seed + gust, streak, 91) * width);
        const top = capAt(geometry, x) + 5;
        const y = top + Math.floor(hash(seed + gust, streak, 97) * Math.max(1, height - top));
        context.fillStyle = streak % 2 === 0 ? ramp(SOLID, 0) : ramp(SOLID, 2);
        context.fillRect(x, y, 10 + streak * 5, streak % 3 === 0 ? 2 : 1);
      }
    }
  });
}
