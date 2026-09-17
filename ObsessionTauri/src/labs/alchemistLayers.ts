export type EyeFrame = 'open' | 'half' | 'closed';
export const eyeFrames: EyeFrame[] = ['open', 'half', 'closed'];
export type LayerImages = Record<'body' | 'flask' | EyeFrame, HTMLImageElement> & Partial<Record<'hat' | 'tail' | 'leftEar' | 'rightEar', HTMLImageElement>>;
type Rect = readonly [number, number, number, number];

// Source rectangles keep each cleaned eye independent: the generated frames have
// different spacing. Source PNGs stay unchanged; these are runtime placements.
export const eyePlacement: Record<EyeFrame, { source: readonly [Rect, Rect]; target: readonly [Rect, Rect] }> = {
  open: { source: [[315, 527, 249, 235], [687, 533, 251, 235]], target: [[393, 532, 200, 188], [687, 538, 200, 188]] },
  half: { source: [[329, 601, 257, 138], [671, 603, 259, 140]], target: [[393, 614, 200, 106], [687, 618, 200, 108]] },
  closed: { source: [[375, 615, 201, 70], [679, 615, 201, 70]], target: [[393, 652, 200, 68], [687, 658, 200, 68]] },
};

export function drawAlchemistFrame(canvas: HTMLCanvasElement, images: LayerImages, frame: EyeFrame, pixels: number, flaskY = 0, twitch?: AlchemistTwitch) {
  canvas.width = pixels;
  canvas.height = pixels;
  const context = canvas.getContext('2d');
  if (!context) return;
  context.imageSmoothingEnabled = false;
  const scale = pixels / 1254;
  const draw = (image: HTMLImageElement, source: Rect, target: Rect, offsetY = 0) => {
    // Snap both edges to physical canvas pixels, not fractional CSS pixels.
    const [x, y, w, h] = target;
    const left = Math.round(x * scale), top = Math.round(y * scale);
    // Offset only the position, never the rounded dimensions. This avoids a
    // one-pixel scale wobble as the flask moves between physical pixel rows.
    context.drawImage(image, ...source, left, top + Math.round(offsetY * scale), Math.round((x + w) * scale) - left, Math.round((y + h) * scale) - top);
  };
  if (images.hat && images.tail && images.leftEar && images.rightEar) {
    drawSeparatedCat(context, images as SeparatedImages, draw, scale, twitch);
  } else {
  const bodyWidth = 1254 * images.body.naturalWidth / images.body.naturalHeight;
  draw(images.body, [0, 0, images.body.naturalWidth, images.body.naturalHeight], [(1254 - bodyWidth) / 2, 0, bodyWidth, 1254]);
  if (twitch && Object.values(twitch).some(value => value !== 0)) {
    // Sample from an already pixel-snapped body, never resample the source at a
    // different scale during motion. Scratch canvas is transient, not a PNG edit.
    const body = document.createElement('canvas');
    body.width = pixels; body.height = pixels;
    const snapshot = body.getContext('2d');
    if (snapshot) {
      snapshot.drawImage(canvas, 0, 0);
      for (const part of ['tail', 'leftEar', 'rightEar'] as const) {
        if (!twitch[part]) continue;
        const [sx, sy, sw, sh] = twitchRegions[part];
        const x = Math.round(sx * scale), y = Math.round(sy * scale);
        const w = Math.round((sx + sw) * scale) - x, h = Math.round((sy + sh) * scale) - y;
        context.clearRect(x, y, w, h);
        for (let row = 0; row < h; row++) {
          const offset = twitchRowShift(twitch[part], row, h, scale);
          context.drawImage(body, x, y + row, w, 1, x + offset, y + row, w, 1);
        }
      }
    }
  }
  }
  draw(images.flask, [383, 445, 487, 429], [419, 742, 418, 369], flaskY);
  const placement = eyePlacement[frame];
  placement.source.forEach((rect, index) => draw(images[frame], rect, placement.target[index]));
}
import { twitchRegions, twitchRowShift, type AlchemistTwitch } from './alchemistTwitch';

type SeparatedImages = LayerImages & Record<'hat' | 'tail' | 'leftEar' | 'rightEar', HTMLImageElement>;
type LayerDraw = (image: HTMLImageElement, source: Rect, target: Rect, offsetY?: number) => void;

// Registered source windows, not equal thirds: the background-removal export
// changed the canvas aspect ratio. Keep one scale per part across all poses.
export const separatedParts = {
  tail: { sources: [[80, 190, 340, 610], [640, 190, 420, 610], [1240, 190, 320, 610]], targets: [[247, 779, 204, 366], [190, 779, 252, 366], [249, 779, 192, 366]] },
  leftEar: { sources: [[100, 295, 390, 390], [640, 295, 390, 390], [1205, 295, 390, 390]], targets: [[330, 258, 190, 190], [330, 258, 190, 190], [330, 258, 190, 190]] },
  rightEar: { sources: [[150, 280, 290, 345], [715, 280, 290, 345]], targets: [[812, 277, 160, 190], [812, 277, 160, 190]] },
} as const;

export function separatedPose(part: 'tail' | 'leftEar' | 'rightEar', amplitude: number): number {
  if (amplitude === 0) return 0;
  // The third right-ear drawing changes anatomy; exclude it from playback.
  return part === 'rightEar' || amplitude < 0 ? 1 : 2;
}

function drawSeparatedCat(context: CanvasRenderingContext2D, images: SeparatedImages, draw: LayerDraw, scale: number, twitch?: AlchemistTwitch) {
  const part = (name: 'tail' | 'leftEar' | 'rightEar') => {
    const pose = separatedPose(name, twitch?.[name] ?? 0);
    const definition = separatedParts[name];
    draw(images[name], definition.sources[pose], definition.targets[pose]);
  };
  part('tail');
  draw(images.body, [0, 0, 1254, 1254], [125, 236, 1003, 1003]);
  draw(images.hat, [0, 0, 1254, 1254], [0, 0, 1254, 1254]);
  part('leftEar');
  part('rightEar');
  // Reuse the SAME hat pixels for front occlusion; the independently generated
  // brim would produce doubled contours. No source PNG is altered.
  context.save();
  context.beginPath();
  const edge = [[0, 500], [330, 450], [470, 414], [610, 432], [790, 502], [980, 605], [1254, 730], [1254, 1254], [0, 1254]];
  edge.forEach(([x, y], index) => {
    if (index === 0) context.moveTo(Math.round(x * scale), Math.round(y * scale));
    else context.lineTo(Math.round(x * scale), Math.round(y * scale));
  });
  context.closePath();
  context.clip();
  draw(images.hat, [0, 0, 1254, 1254], [0, 0, 1254, 1254]);
  context.restore();
}
