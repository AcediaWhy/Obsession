// Exports the manually authored raster directly; no browser or input photo.
import { readFileSync, writeFileSync } from "node:fs";
import { deflateSync } from "node:zlib";
import ts from "typescript";

const source = readFileSync(new URL("../../../src/design/components/axolotl/handPaintedLetsGoScene.ts", import.meta.url), "utf8");
const js = ts.transpileModule(source, { compilerOptions: { target: ts.ScriptTarget.ES2020, module: ts.ModuleKind.ESNext } }).outputText;
const drawing = await import(`data:text/javascript;base64,${Buffer.from(js).toString("base64")}`);
const scene = drawing.createPaintedScene();
const raster = drawing.renderPaintedScene(scene, 0);
const rgba = drawing.paintedSceneRgba(raster);

function crc32(bytes) {
  let crc = 0xffffffff;
  for (const byte of bytes) {
    crc ^= byte;
    for (let bit = 0; bit < 8; bit++) crc = (crc >>> 1) ^ (crc & 1 ? 0xedb88320 : 0);
  }
  return (crc ^ 0xffffffff) >>> 0;
}
function chunk(type, data) {
  const body = Buffer.concat([Buffer.from(type), data]);
  const size = Buffer.alloc(4); size.writeUInt32BE(data.length);
  const crc = Buffer.alloc(4); crc.writeUInt32BE(crc32(body));
  return Buffer.concat([size, body, crc]);
}
function png(scale) {
  const width = raster.width * scale, height = raster.height * scale;
  const raw = Buffer.alloc((width * 4 + 1) * height);
  for (let y = 0; y < height; y++) {
    for (let x = 0; x < width; x++) {
      const from = (Math.floor(y / scale) * raster.width + Math.floor(x / scale)) * 4;
      const to = y * (width * 4 + 1) + 1 + x * 4;
      raw.set(rgba.subarray(from, from + 4), to);
    }
  }
  const header = Buffer.alloc(13);
  header.writeUInt32BE(width); header.writeUInt32BE(height, 4); header[8] = 8; header[9] = 6;
  return Buffer.concat([Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]), chunk("IHDR", header), chunk("IDAT", deflateSync(raw)), chunk("IEND", Buffer.alloc(0))]);
}
for (const scale of [1, 3]) {
  const file = new URL(`hand-painted-${scale}x.png`, import.meta.url);
  writeFileSync(file, png(scale));
  console.log(file.pathname);
}
