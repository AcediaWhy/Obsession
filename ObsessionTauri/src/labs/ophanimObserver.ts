import * as THREE from 'three';
import type { ObsessionVisualPhase } from '../design/obsessionVisualState';

const TAU = Math.PI * 2;
const clamp = (x: number) => Math.max(0, Math.min(1, x));
const ease = (x: number) => { const v = clamp(x); return v * v * (3 - 2 * v); };

export type ObserverOptions = {
  phase: ObsessionVisualPhase;
  rings: boolean;
  eyes: boolean;
  strength: number;
};

/** Solid cylindrical bands with eye textures and depth-tested PNG occlusion. */
export function createOphanimObserver(canvas: HTMLCanvasElement, cat: HTMLImageElement) {
  const renderer = new THREE.WebGLRenderer({ canvas, alpha: true, antialias: true, powerPreference: 'low-power' });
  renderer.setSize(canvas.width, canvas.height, false);
  renderer.setClearColor(0x000000, 0);
  renderer.outputColorSpace = THREE.SRGBColorSpace;
  renderer.toneMapping = THREE.NoToneMapping;
  const scene = new THREE.Scene();
  const camera = new THREE.OrthographicCamera(-256, 256, 256, -256, 1, 1600);
  camera.position.set(0, 0, 900);
  scene.add(new THREE.HemisphereLight(0xfff2d6, 0x595066, 2.3));
  const key = new THREE.DirectionalLight(0xffe4ae, 3.5);
  key.position.set(-220, 320, 380); scene.add(key);
  const fill = new THREE.DirectionalLight(0xc8c3ed, 1.8);
  fill.position.set(260, -40, 220); scene.add(fill);
  const rim = new THREE.DirectionalLight(0xffd37b, 3);
  rim.position.set(20, 200, -300); scene.add(rim);

  const catTexture = new THREE.Texture(cat);
  catTexture.colorSpace = THREE.SRGBColorSpace;
  catTexture.magFilter = THREE.NearestFilter;
  catTexture.minFilter = THREE.LinearMipmapLinearFilter;
  catTexture.needsUpdate = true;
  const catMesh = new THREE.Mesh(new THREE.PlaneGeometry(342, 342), new THREE.MeshBasicMaterial({ map: catTexture, alphaTest: .1 }));
  catMesh.position.set(0, 18, 0); scene.add(catMesh);
  const bandRoot = new THREE.Group(); scene.add(bandRoot);
  const small = canvas.width < 400;
  const columns = small ? 14 : 22;
  const rows = small ? 1 : 2;
  const configs = [
    { radius: 166, width: 38, x: 1.18, z: .62, speed: .18 },
    { radius: 184, width: 38, x: 1.30, z: -.78, speed: -.14 },
    { radius: 205, width: 42, x: .28, z: -.10, speed: .12 },
  ];
  const bands = configs.map((config, index) => {
    const tilt = new THREE.Group(), spin = new THREE.Group();
    tilt.add(spin); bandRoot.add(tilt);
    const art = document.createElement('canvas'); art.width = 2048; art.height = 192;
    const ctx = art.getContext('2d');
    if (!ctx) throw new Error('Canvas для рисунка глаз недоступен');
    const texture = new THREE.CanvasTexture(art);
    texture.colorSpace = THREE.SRGBColorSpace;
    texture.anisotropy = Math.min(4, renderer.capabilities.getMaxAnisotropy());
    const metal = new THREE.MeshStandardMaterial({ map: texture, metalness: .48, roughness: .42 });
    const innerMetal = metal.clone(); innerMetal.side = THREE.BackSide;
    const edgeMetal = new THREE.MeshStandardMaterial({ color: '#d9bb79', metalness: .70, roughness: .30 });
    const darkEdge = new THREE.MeshStandardMaterial({ color: '#79603a', metalness: .60, roughness: .38 });
    const thickness = 7;
    spin.add(new THREE.Mesh(new THREE.CylinderGeometry(config.radius, config.radius, config.width, 128, 1, true), metal));
    spin.add(new THREE.Mesh(new THREE.CylinderGeometry(config.radius - thickness, config.radius - thickness, config.width, 128, 1, true), innerMetal));
    for (const sign of [-1, 1]) {
      const lip = new THREE.Mesh(new THREE.RingGeometry(config.radius - thickness, config.radius, 128), edgeMetal);
      lip.rotation.x = -sign * Math.PI / 2; lip.position.y = sign * config.width / 2; spin.add(lip);
      const rail = new THREE.Mesh(new THREE.TorusGeometry(config.radius - .8, 1.8, 6, 128), edgeMetal);
      rail.rotation.x = Math.PI / 2; rail.position.y = sign * (config.width / 2 - .8); spin.add(rail);
      const inset = new THREE.Mesh(new THREE.TorusGeometry(config.radius - thickness + .5, 1.1, 5, 128), darkEdge);
      inset.rotation.x = Math.PI / 2; inset.position.y = sign * (config.width / 2 - 1); spin.add(inset);
    }
    return { config, index, tilt, spin, ctx, art, texture, metal, innerMetal };
  });

  let disposed = false, time = 0, orbit = 0, entered = 0, speed = 1, vigilance = .3, alarm = 0;
  let phase: ObsessionVisualPhase = 'idle';
  let textureAt = -Infinity, shownEyes = true;

  function paintBand(band: typeof bands[number], eyes: boolean) {
    const { ctx, art, index } = band;
    const w = art.width, h = art.height;
    const background = ctx.createLinearGradient(0, 0, 0, h);
    background.addColorStop(0, '#ead398'); background.addColorStop(.08, '#876037');
    background.addColorStop(.22, '#c8a569'); background.addColorStop(.50, '#dfc38a');
    background.addColorStop(.85, '#b78c51'); background.addColorStop(1, '#f1dca7');
    ctx.fillStyle = background; ctx.fillRect(0, 0, w, h);
    ctx.strokeStyle = '#6f4e2e'; ctx.lineWidth = 3;
    for (const y of [12, h - 12]) { ctx.beginPath(); ctx.moveTo(0, y); ctx.lineTo(w, y); ctx.stroke(); }
    if (rows > 1) { ctx.fillStyle = '#805c3570'; ctx.fillRect(0, h / 2 - 1, w, 2); }
    for (let row = 0; row < rows; row++) {
      for (let col = 0; col < columns; col++) {
        const cell = w / columns, x = (col + .5) * cell;
        const y = (row + .5) * (h - 28) / rows + 14;
        const rx = cell * .43, ry = (h - 28) / rows * .40;
        ctx.save(); ctx.translate(x, y);
        ctx.fillStyle = '#6e4c2f';
        ctx.beginPath(); ctx.ellipse(0, 3, rx + 3, ry + 3, 0, 0, TAU); ctx.fill();
        ctx.fillStyle = '#f2d8a0';
        ctx.beginPath(); ctx.ellipse(0, -1, rx + 1, ry + 1, 0, 0, TAU); ctx.fill();
        ctx.fillStyle = '#956f40';
        ctx.beginPath(); ctx.ellipse(0, 1, rx - 3, ry - 3, 0, 0, TAU); ctx.fill();
        if (eyes) {
          const blinkTime = (time + col * .83 + row * 1.3 + index * 2.7) % 10.6;
          const blink = 1 - Math.sin(clamp((blinkTime - 8.8) / .32) * Math.PI);
          const wake = phase === 'engaging' ? ease((time - entered - (col % 6) * .14 - index * .2) / .6) : vigilance;
          ctx.scale(1, Math.max(.055, (.60 + wake * .4) * blink));
          ctx.beginPath(); ctx.moveTo(-rx + 4, 0);
          ctx.bezierCurveTo(-rx * .30, -ry * 1.28, rx * .32, -ry * 1.28, rx - 4, 0);
          ctx.bezierCurveTo(rx * .32, ry * 1.28, -rx * .32, ry * 1.28, -rx + 4, 0);
          ctx.closePath(); ctx.fillStyle = '#fcf4df'; ctx.fill();
          ctx.strokeStyle = '#533d2c'; ctx.lineWidth = 3; ctx.stroke(); ctx.clip();
          const gaze = phase === 'scanning' ? Math.sin(time * .7) * rx * .21 : 0;
          const iris = ctx.createRadialGradient(gaze - 2, -2, 2, gaze, 0, ry * .94);
          iris.addColorStop(0, alarm > .3 ? '#401518' : '#242b39');
          iris.addColorStop(.55, alarm > .3 ? '#aa4339' : '#657f87');
          iris.addColorStop(.82, alarm > .3 ? '#ce7c54' : '#a9b9b1');
          iris.addColorStop(1, '#3b443f');
          ctx.fillStyle = iris; ctx.beginPath(); ctx.ellipse(gaze, 0, ry * .84, ry * .94, 0, 0, TAU); ctx.fill();
          ctx.fillStyle = '#16181d'; ctx.beginPath(); ctx.ellipse(gaze, 0, ry * .34, ry * .60, 0, 0, TAU); ctx.fill();
          ctx.fillStyle = '#fffdf3'; ctx.beginPath(); ctx.arc(gaze - ry * .20, -ry * .30, ry * .12, 0, TAU); ctx.fill();
        }
        ctx.restore();
      }
    }
    band.texture.needsUpdate = true;
  }

  function render(dt: number, options: ObserverOptions) {
    if (disposed) return;
    const changed = options.phase !== phase;
    if (changed) { phase = options.phase; entered = time; }
    const step = Math.min(.06, Math.max(0, dt)); time += step;
    const targetSpeed = phase === 'focused' ? 0 : phase === 'engaging' ? 1.65 : phase === 'scanning' ? 1.15 : .65;
    speed += (targetSpeed - speed) * (1 - Math.exp(-step * 2));
    vigilance += ((phase === 'idle' ? .25 : 1) - vigilance) * (1 - Math.exp(-step * 3));
    alarm += ((phase === 'fault' ? 1 : 0) - alarm) * (1 - Math.exp(-step * 4));
    orbit += step * speed * options.strength;
    bandRoot.visible = options.rings; bandRoot.position.y = -8;
    const updateTexture = changed || shownEyes !== options.eyes || time - textureAt > .09;
    for (const band of bands) {
      const { config, index } = band;
      band.tilt.rotation.set(config.x + Math.sin(orbit * .27 + index) * .10, 0, config.z + Math.sin(orbit * .21 + index * 1.6) * .08);
      band.spin.rotation.y = orbit * config.speed + index * .37;
      band.tilt.scale.setScalar(1 + alarm * .035);
      band.metal.color.setRGB(1, 1 - alarm * .25, 1 - alarm * .35);
      band.innerMetal.color.copy(band.metal.color);
      if (updateTexture) paintBand(band, options.eyes);
    }
    if (updateTexture) { textureAt = time; shownEyes = options.eyes; }
    catMesh.position.y = 18 + Math.sin(time * .85) * 2.5 * options.strength;
    catMesh.scale.setScalar(1 + Math.sin(time * 1.4) * .004 * options.strength);
    renderer.render(scene, camera);
    canvas.dataset.phase = phase; canvas.dataset.time = time.toFixed(3);
    canvas.dataset.ready = 'true'; canvas.dataset.geometry = 'solid-eye-bands';
  }

  return { render, dispose() {
    if (disposed) return;
    disposed = true;
    const geometries = new Set<THREE.BufferGeometry>(), materials = new Set<THREE.Material>();
    scene.traverse(object => {
      if (object instanceof THREE.Mesh) {
        geometries.add(object.geometry);
        for (const material of Array.isArray(object.material) ? object.material : [object.material]) materials.add(material);
      }
    });
    geometries.forEach(geometry => geometry.dispose()); materials.forEach(material => material.dispose());
    catTexture.dispose(); bands.forEach(band => { band.texture.dispose(); band.art.width = band.art.height = 0; });
    renderer.clear(); renderer.dispose();
  } };
}
