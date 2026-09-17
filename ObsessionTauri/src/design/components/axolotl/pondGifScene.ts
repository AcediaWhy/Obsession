/** Lossless decoded GIF frames. Source timing: 16 frames × 150 ms. */
export const SOURCE_ART_WIDTH = 1024;
export const SOURCE_ART_HEIGHT = 1024;
export const POND_FRAME_MS = 150;
export type PondScene = { frames: HTMLImageElement[] };
let pending: Promise<PondScene> | undefined;
export function loadSourceArt(): Promise<PondScene> {
  if (!pending) {
    pending = Promise.all(Array.from({length:16}, (_, index) => new Promise<HTMLImageElement>((resolve,reject) => {
      const image = new Image();
      image.onload = () => resolve(image);
      image.onerror = () => reject(new Error(`Не удалось загрузить кадр пруда ${index}`));
      image.src = `${import.meta.env.BASE_URL}lab-assets/pond/frame-${String(index).padStart(2,"0")}.png`;
    }))).then(frames => ({frames})).catch(error => { pending=undefined; throw error; });
  }
  return pending;
}
export function drawSourceArt(context:CanvasRenderingContext2D, scene:PondScene, width:number, height:number, seconds:number, still=false) {
  const elapsed = Number.isFinite(seconds) ? Math.max(0,seconds) : 0;
  const index = still ? 0 : Math.floor((elapsed*1000 + .001)/POND_FRAME_MS)%scene.frames.length;
  const sourceRatio = SOURCE_ART_WIDTH / SOURCE_ART_HEIGHT;
  const targetRatio = width / height;
  let sourceX = 0;
  let sourceY = 0;
  let sourceWidth = SOURCE_ART_WIDTH;
  let sourceHeight = SOURCE_ART_HEIGHT;

  if (targetRatio > sourceRatio) {
    sourceHeight = SOURCE_ART_WIDTH / targetRatio;
    sourceY = (SOURCE_ART_HEIGHT - sourceHeight) / 2;
  } else if (targetRatio < sourceRatio) {
    sourceWidth = SOURCE_ART_HEIGHT * targetRatio;
    sourceX = (SOURCE_ART_WIDTH - sourceWidth) / 2;
  }

  context.setTransform(1,0,0,1,0,0);
  // The 1024px source is usually reduced only a little to fit the viewport.
  // Bilinear sampling softens single-pixel stair steps without blurring the
  // scene as aggressively as a high-quality resample would.
  context.imageSmoothingEnabled=true;
  context.imageSmoothingQuality="low";
  context.fillStyle="#0b1012";
  context.fillRect(0,0,width,height);
  context.drawImage(
    scene.frames[index],
    sourceX,
    sourceY,
    sourceWidth,
    sourceHeight,
    0,
    0,
    width,
    height,
  );
}
