import * as THREE from "three";

export type EarTextures = {
  albedo: THREE.DataTexture;
  normal: THREE.DataTexture;
  roughness: THREE.DataTexture;
  density: THREE.DataTexture;
  hairAlpha: THREE.DataTexture;
  anisotropy: THREE.DataTexture;
  dispose: () => void;
};

function hash2(x: number, y: number): number {
  const value = Math.sin(x * 127.1 + y * 311.7 + 19.19) * 43758.5453;
  return value - Math.floor(value);
}

function smoothstep(value: number): number {
  return value * value * (3 - 2 * value);
}

function valueNoise(x: number, y: number): number {
  const ix = Math.floor(x);
  const iy = Math.floor(y);
  const fx = smoothstep(x - ix);
  const fy = smoothstep(y - iy);
  const a = hash2(ix, iy);
  const b = hash2(ix + 1, iy);
  const c = hash2(ix, iy + 1);
  const d = hash2(ix + 1, iy + 1);
  const top = a + (b - a) * fx;
  const bottom = c + (d - c) * fx;
  return top + (bottom - top) * fy;
}

function heightAt(x: number, y: number): number {
  const softTufts = valueNoise(x * 0.075, y * 0.3) - 0.5;
  const shortNap = valueNoise(x * 0.34 + y * 0.025, y * 1.18) - 0.5;
  const pores = hash2(Math.floor(x * 0.82), Math.floor(y * 0.82)) - 0.5;
  return 0.5 + softTufts * 0.17 + shortNap * 0.075 + pores * 0.025;
}

function makeTexture(
  data: Uint8Array,
  size: number,
  colorSpace: THREE.ColorSpace = THREE.NoColorSpace,
): THREE.DataTexture {
  const texture = new THREE.DataTexture(data, size, size, THREE.RGBAFormat, THREE.UnsignedByteType);
  texture.wrapS = THREE.RepeatWrapping;
  texture.wrapT = THREE.RepeatWrapping;
  texture.minFilter = THREE.LinearMipmapLinearFilter;
  texture.magFilter = THREE.LinearFilter;
  texture.generateMipmaps = true;
  texture.colorSpace = colorSpace;
  texture.anisotropy = 8;
  texture.needsUpdate = true;
  return texture;
}

export function createEarTextures(size: number): EarTextures {
  const safeSize = Math.max(64, Math.round(size));
  const albedo = new Uint8Array(safeSize * safeSize * 4);
  const normal = new Uint8Array(albedo.length);
  const roughness = new Uint8Array(albedo.length);
  const density = new Uint8Array(albedo.length);
  const hairAlpha = new Uint8Array(albedo.length);
  const anisotropy = new Uint8Array(albedo.length);

  for (let y = 0; y < safeSize; y += 1) {
    for (let x = 0; x < safeSize; x += 1) {
      const offset = (y * safeSize + x) * 4;
      const height = heightAt(x, y);
      const dx = heightAt(x + 1, y) - heightAt(x - 1, y);
      const dy = heightAt(x, y + 1) - heightAt(x, y - 1);
      const nz = 1;
      const length = Math.hypot(dx * 1.8, dy * 0.72, nz);
      const grain = hash2(x * 0.37, y * 0.29);
      const colorNoise = Math.max(-1, Math.min(1, (height - 0.5) * 0.55 + (grain - 0.5) * 0.055));

      albedo[offset] = Math.round(156 + colorNoise * 24);
      albedo[offset + 1] = Math.round(198 + colorNoise * 21);
      albedo[offset + 2] = Math.round(178 + colorNoise * 22);
      albedo[offset + 3] = 255;

      normal[offset] = Math.round((0.5 - (dx * 1.28) / length * 0.5) * 255);
      normal[offset + 1] = Math.round((0.5 - (dy * 0.54) / length * 0.5) * 255);
      normal[offset + 2] = Math.round((0.5 + nz / length * 0.5) * 255);
      normal[offset + 3] = 255;

      const rough = Math.round(198 + grain * 26 + Math.abs(height - 0.5) * 28);
      roughness[offset] = rough;
      roughness[offset + 1] = rough;
      roughness[offset + 2] = rough;
      roughness[offset + 3] = 255;

      const clump = valueNoise(x * 0.22 + y * 0.018, y * 0.105);
      const filament = valueNoise(x * 0.92, y * 0.16);
      const tuft = clump * 0.46 + filament * 0.28 + grain * 0.26;
      const alpha = Math.round(Math.max(0, Math.min(1, (tuft - 0.31) / 0.56)) * 255);
      density[offset] = alpha;
      density[offset + 1] = alpha;
      density[offset + 2] = alpha;
      density[offset + 3] = 255;

      const u = x / Math.max(1, safeSize - 1);
      const v = y / Math.max(1, safeSize - 1);
      const strandCenter = 0.5 + Math.sin(v * 10.7) * 0.028 * (1 - v);
      const strandWidth = 0.44 * Math.pow(1 - v, 0.68) + 0.016;
      const strandDistance = Math.abs(u - strandCenter);
      const strandBody = 1 - Math.max(0, Math.min(1,
        (strandDistance - strandWidth * 0.58) / Math.max(0.001, strandWidth * 0.42),
      ));
      const rootFade = Math.max(0, Math.min(1, v / 0.075));
      const tipFade = 1 - Math.max(0, Math.min(1, (v - 0.84) / 0.16));
      const split = 0.76 + valueNoise(x * 0.18, y * 0.28) * 0.24;
      const hair = Math.round(strandBody * rootFade * tipFade * split * 255);
      hairAlpha[offset] = hair;
      hairAlpha[offset + 1] = hair;
      hairAlpha[offset + 2] = hair;
      hairAlpha[offset + 3] = 255;

      const directionWobble = Math.sin(v * 18.3 + valueNoise(x * 0.05, y * 0.05) * 2.4) * 0.1;
      anisotropy[offset] = Math.round((0.5 + directionWobble * 0.5) * 255);
      anisotropy[offset + 1] = 246;
      anisotropy[offset + 2] = Math.round((0.72 + grain * 0.2) * 255);
      anisotropy[offset + 3] = 255;
    }
  }

  const textures = {
    albedo: makeTexture(albedo, safeSize, THREE.SRGBColorSpace),
    normal: makeTexture(normal, safeSize),
    roughness: makeTexture(roughness, safeSize),
    density: makeTexture(density, safeSize),
    hairAlpha: makeTexture(hairAlpha, safeSize),
    anisotropy: makeTexture(anisotropy, safeSize),
  };
  return {
    ...textures,
    dispose: () => Object.values(textures).forEach((texture) => texture.dispose()),
  };
}
