import * as THREE from "three";
import { RoundedBoxGeometry } from "three/addons/geometries/RoundedBoxGeometry.js";
import { createRenderLoop, type QualityTier } from "../../render";
import type { HelixState } from "./state";
import { fieldFragment, fieldVertex, helixFragment, helixVertex } from "./shaders";

export type HelixScene = { update(state: HelixState): void; dispose(): void };
const BAR_COUNT = 36;

export function createGlassHelix(canvas: HTMLCanvasElement, initial: HelixState, unavailable: () => void = () => {}): HelixScene {
  const renderer = new THREE.WebGLRenderer({ canvas, alpha: true, antialias: true,
    powerPreference: "low-power", preserveDrawingBuffer: false });
  renderer.setClearColor(0x000000, 0);
  renderer.outputColorSpace = THREE.SRGBColorSpace;
  const scene = new THREE.Scene();
  const camera = new THREE.OrthographicCamera(-4.2, 4.2, 1.7, -1.7, .1, 30);
  camera.position.z = 8;
  const shape = new RoundedBoxGeometry(.14, 1.72, .19, 3, .06);
  const geometry = new THREE.InstancedBufferGeometry();
  geometry.index = shape.index?.clone() ?? null;
  for (const name of ["position", "normal", "uv"]) geometry.setAttribute(name, shape.getAttribute(name).clone());
  shape.dispose();
  geometry.setAttribute("aSlot", new THREE.InstancedBufferAttribute(Float32Array.from({ length: BAR_COUNT }, (_, i) => i / (BAR_COUNT - 1)), 1));
  geometry.instanceCount = BAR_COUNT;
  const trail = Array.from({ length: 8 }, () => new THREE.Vector4(0, 0, 0, -100));
  const uniforms = {
    uTime: { value: 0 }, uOpen: { value: initial.motionOff ? 1.5 : 0 }, uClose: { value: 0 },
    uConfirm: { value: 0 }, uRestore: { value: 0 }, uSuccess: { value: 0 }, uFinishAge: { value: 0 },
    uCandidateAge: { value: 5 }, uAccent: { value: new THREE.Color(.65, .57, .91) },
    uPointer: { value: new THREE.Vector2(0, 0) }, uPointerPower: { value: 0 }, uTrail: { value: trail },
  };
  const material = new THREE.ShaderMaterial({ vertexShader: helixVertex, fragmentShader: helixFragment, uniforms });
  const bars = new THREE.Mesh(geometry, material);
  bars.frustumCulled = false;
  scene.add(bars);
  const fieldGeometry = new THREE.PlaneGeometry(9, 4.5);
  const fieldMaterial = new THREE.ShaderMaterial({ vertexShader: fieldVertex, fragmentShader: fieldFragment,
    uniforms, transparent: true, depthWrite: false });
  const field = new THREE.Mesh(fieldGeometry, fieldMaterial);
  field.position.z = -1.5;
  field.renderOrder = -1;
  scene.add(field);
  let state = initial;
  let disposed = false;
  let time = 0, openAge = initial.motionOff ? 1.5 : 0, stageAge = 0, candidateAge = 5, closeFrom = 0;
  let quality: QualityTier = "high";
  let width = 0, height = 0, skipTime = 0, trailIndex = 0;
  let frames = 0, sampleFrames = 0, sampleStart = 0, submissionMs = 0;
  const pointerTarget = new THREE.Vector2(0, 0);
  const pointerVelocity = new THREE.Vector2(0, 0);
  let pointerActive = false;

  const themeColor = () => {
    const channels = getComputedStyle(canvas).getPropertyValue("--accent").match(/[\d.]+/g)?.map(Number);
    if (channels?.length === 3) uniforms.uAccent.value.setRGB(channels[0] / 255, channels[1] / 255, channels[2] / 255, THREE.SRGBColorSpace);
  };
  const resize = () => {
    if (disposed) return;
    const rect = canvas.getBoundingClientRect();
    width = Math.max(1, rect.width); height = Math.max(1, rect.height);
    // Bound backing-store size even on 4K/high-density displays.
    const dpr = Math.min(window.devicePixelRatio || 1, quality === "high" ? 1.25 : quality === "balanced" ? 1 : .8,
      720 / width, 240 / height);
    renderer.setPixelRatio(dpr);
    renderer.setSize(width, height, false);
    camera.top = Math.max(1.4, 4.2 * height / width);
    camera.bottom = -camera.top;
    camera.right = camera.top * width / height;
    camera.left = -camera.right;
    camera.updateProjectionMatrix();
    loop.invalidate();
  };
  const move = (event: PointerEvent) => {
    if (state.motionOff || disposed) return;
    const rect = canvas.getBoundingClientRect();
    const x = ((event.clientX - rect.left) / rect.width - .5) * camera.right * 2;
    const y = (.5 - (event.clientY - rect.top) / rect.height) * camera.top * 2;
    const speed = Math.min(1, Math.hypot(x - pointerTarget.x, y - pointerTarget.y));
    pointerTarget.set(x, y);
    pointerActive = true;
    trail[trailIndex].set(x, y, .22 + speed * .65, time);
    trailIndex = (trailIndex + 1) % trail.length;
  };
  const leave = () => { pointerActive = false; };
  const terminal = () => ["success", "finished", "cancelled"].includes(state.stage);

  const loop = createRenderLoop((dt, now) => {
    if (disposed) return;
    skipTime += dt;
    if (!state.motionOff && quality === "low" && skipTime < 1 / 30 - .002) return;
    const step = state.motionOff ? 0 : Math.min(skipTime, .08);
    skipTime = 0;
    time += step; openAge += step; stageAge += step; candidateAge += step;
    uniforms.uTime.value = time;
    uniforms.uOpen.value = state.motionOff ? 1.5 : openAge;
    uniforms.uCandidateAge.value = candidateAge;
    uniforms.uFinishAge.value = stageAge;
    const smoothing = state.motionOff ? 1 : 1 - Math.exp(-step * 5);
    const restoring = state.stage === "restoring" || state.stage === "cancelling";
    uniforms.uConfirm.value += ((state.stage === "confirming" ? 1 : 0) - uniforms.uConfirm.value) * smoothing;
    uniforms.uRestore.value += ((restoring ? 1 : 0) - uniforms.uRestore.value) * smoothing;
    uniforms.uSuccess.value = state.stage === "success" ? 1 : 0;
    uniforms.uClose.value = terminal() ? Math.max(closeFrom, Math.max(0, stageAge - (state.stage === "success" ? .24 : 0)) * 1.3)
      : state.stage === "cancelling" ? Math.min(.55, stageAge * .8) : 0;
    if (!state.motionOff) {
      // Damped spring: bounded local deformation, no per-frame allocations.
      const mouse = uniforms.uPointer.value;
      pointerVelocity.x += ((pointerTarget.x - mouse.x) * 42 - pointerVelocity.x * 12) * step;
      pointerVelocity.y += ((pointerTarget.y - mouse.y) * 42 - pointerVelocity.y * 12) * step;
      mouse.x += pointerVelocity.x * step; mouse.y += pointerVelocity.y * step;
      uniforms.uPointerPower.value += ((pointerActive ? 1 : 0) - uniforms.uPointerPower.value) * smoothing;
    }
    const start = performance.now();
    renderer.render(scene, camera);
    submissionMs += performance.now() - start;
    frames++; sampleFrames++;
    if (import.meta.env.DEV && (!sampleStart || now - sampleStart >= 1000)) {
      canvas.dataset.frames = String(frames);
      canvas.dataset.drawCalls = String(renderer.info.render.calls);
      canvas.dataset.triangles = String(renderer.info.render.triangles);
      canvas.dataset.quality = quality;
      canvas.dataset.submissionMs = (submissionMs / sampleFrames).toFixed(2);
      canvas.dataset.fps = sampleStart ? (sampleFrames * 1000 / (now - sampleStart)).toFixed(1) : "warming";
      sampleStart = now; sampleFrames = 0; submissionMs = 0;
    }
  }, { role: "secondary", paused: initial.motionOff, maxDt: .05,
    onQualityChange: (tier) => { quality = tier; resize(); } });

  const resizeObserver = new ResizeObserver(resize);
  const themeObserver = new MutationObserver(() => { themeColor(); loop.invalidate(); });
  const contextLost = (event: Event) => { event.preventDefault(); dispose(); unavailable(); };
  const dispose = () => {
    if (disposed) return;
    disposed = true;
    loop.dispose(); resizeObserver.disconnect(); themeObserver.disconnect();
    canvas.removeEventListener("pointermove", move); canvas.removeEventListener("pointerleave", leave);
    canvas.removeEventListener("webglcontextlost", contextLost);
    geometry.dispose(); material.dispose(); fieldGeometry.dispose(); fieldMaterial.dispose();
    renderer.dispose(); renderer.forceContextLoss();
    canvas.width = 0; canvas.height = 0;
  };
  try {
    themeColor(); resize();
    let shaderFailed = false;
    renderer.debug.onShaderError = () => { shaderFailed = true; };
    renderer.compile(scene, camera);
    if (shaderFailed) throw new Error("Glass helix shaders could not compile");
    resizeObserver.observe(canvas);
    themeObserver.observe(document.documentElement, { attributes: true, attributeFilter: ["data-theme", "style", "class"] });
    canvas.addEventListener("pointermove", move, { passive: true });
    canvas.addEventListener("pointerleave", leave, { passive: true });
    canvas.addEventListener("webglcontextlost", contextLost);
    loop.start();
  } catch (error) { dispose(); throw error; }
  return { dispose, update: (next) => {
    if (disposed) return;
    if (terminal() && !["success", "finished", "cancelled"].includes(next.stage)) {
      openAge = 0; candidateAge = 5; closeFrom = 0;
    }
    if (next.stage !== state.stage && ["success", "finished", "cancelled"].includes(next.stage)) closeFrom = uniforms.uClose.value;
    if (next.stage !== state.stage) stageAge = 0;
    if (next.candidate !== state.candidate) candidateAge = 0;
    state = next;
    loop.setPaused(next.motionOff); loop.invalidate();
  } };
}
