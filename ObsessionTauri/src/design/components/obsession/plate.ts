export type ObsessionPlate = {
  size: number;
  color: Uint8Array;
  normal: Uint8Array;
};

function fract(value: number): number {
  return value - Math.floor(value);
}

function hash(x: number, y: number): number {
  return fract(Math.sin(x * 127.1 + y * 311.7) * 43758.5453123);
}

function smoothNoise(x: number, y: number): number {
  const ix = Math.floor(x);
  const iy = Math.floor(y);
  const fx = x - ix;
  const fy = y - iy;
  const ux = fx * fx * (3 - 2 * fx);
  const uy = fy * fy * (3 - 2 * fy);
  const a = hash(ix, iy);
  const b = hash(ix + 1, iy);
  const c = hash(ix, iy + 1);
  const d = hash(ix + 1, iy + 1);
  return a + (b - a) * ux + (c - a) * uy + (a - b - c + d) * ux * uy;
}

function heightAt(x: number, y: number, size: number): number {
  const u = x / size;
  const v = y / size;
  const dx = u - 0.79;
  const dy = v - 0.25;
  const radius = Math.sqrt(dx * dx + dy * dy);
  const stress = Math.sin(radius * 92 - smoothNoise(u * 5, v * 5) * 2.2) * 0.18;
  const brushed = Math.sin((u * 1.6 + v) * 118) * 0.035;
  return smoothNoise(u * 7.5, v * 7.5) * 0.55 + stress + brushed;
}

/** Deterministic optical color/normal plate, generated once and uploaded to the GPU. */
export function createObsessionPlate(size = 192): ObsessionPlate {
  const color = new Uint8Array(size * size * 4);
  const normal = new Uint8Array(size * size * 4);
  for (let y = 0; y < size; y += 1) {
    for (let x = 0; x < size; x += 1) {
      const index = (y * size + x) * 4;
      const u = x / size;
      const v = y / size;
      const h = heightAt(x, y, size);
      const inclusion = Math.max(0, 1 - Math.hypot(u - 0.77, v - 0.24) * 4.1);
      const sheen = Math.max(0, Math.sin((u * 0.72 + v) * 24 + h * 4)) * 3;
      color[index] = Math.round(4 + h * 3 + sheen + inclusion * 9);
      color[index + 1] = Math.round(5 + h * 3 + sheen + inclusion * 2);
      color[index + 2] = Math.round(7 + h * 4 + sheen + inclusion * 4);
      color[index + 3] = 255;

      const left = heightAt(x - 1, y, size);
      const right = heightAt(x + 1, y, size);
      const up = heightAt(x, y - 1, size);
      const down = heightAt(x, y + 1, size);
      const nx = (left - right) * 0.75;
      const ny = (up - down) * 0.75;
      const nz = 1 / Math.sqrt(nx * nx + ny * ny + 1);
      normal[index] = Math.round((nx * nz * 0.5 + 0.5) * 255);
      normal[index + 1] = Math.round((ny * nz * 0.5 + 0.5) * 255);
      normal[index + 2] = Math.round((nz * 0.5 + 0.5) * 255);
      normal[index + 3] = 255;
    }
  }
  return { size, color, normal };
}
