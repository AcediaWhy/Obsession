import { useEffect, useRef, useState } from "react";
import * as THREE from "three";
import { GLTFLoader } from "three/addons/loaders/GLTFLoader.js";

import { createRenderLoop, type RenderLoop } from "../render";
import { createEarAtmosphere } from "./yaniEar/atmosphere";
import { createEarCrown } from "./yaniEar/crown";
import { createEarCrownAsset } from "./yaniEar/crownAsset";
import { createEarMaterials } from "./yaniEar/materials";
import { createEarSpringPose, earTarget, stepEarSpringPose } from "./yaniEar/motion";
import { earQualityProfile } from "./yaniEar/quality";
import { createEarRig, type EarSourceGeometry } from "./yaniEar/rig";
import { createEarTextures } from "./yaniEar/textures";
import type { EarMood, EarPose, EarQuality, EarSpringPose } from "./yaniEar/types";

type Props = {
  mood: EarMood;
  quality: EarQuality;
  paused?: boolean;
  variant?: "field" | "core";
  className?: string;
  debugCapture?: boolean;
};

function poseValues(springs: EarSpringPose): EarPose {
  return Object.fromEntries(
    (Object.keys(springs) as (keyof EarSpringPose)[]).map((key) => [key, springs[key].value]),
  ) as EarPose;
}

function poseEnergy(springs: EarSpringPose): number {
  const energy = Math.abs(springs.yaw.velocity) * 0.52
    + Math.abs(springs.pitch.velocity) * 0.38
    + Math.abs(springs.splay.velocity) * 0.34
    + Math.abs(springs.cup.velocity) * 0.24
    + Math.abs(springs.tip.velocity) * 0.7
    + Math.abs(springs.lift.velocity) * 1.8;
  return THREE.MathUtils.clamp(energy, 0, 1);
}

export function YaniEarScene({
  mood,
  quality,
  paused = false,
  variant = "field",
  className = "",
  debugCapture = false,
}: Props) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const loopRef = useRef<RenderLoop | null>(null);
  const stateRef = useRef({ mood, paused, quality });
  stateRef.current = { mood, paused, quality };
  const [failed, setFailed] = useState(false);
  const [debugFrame, setDebugFrame] = useState<string | null>(null);

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    let cancelled = false;
    let resizeObserver: ResizeObserver | null = null;
    let cleanupScene: (() => void) | null = null;
    let retryTimer: number | null = null;
    let retryAttempt = 0;

    const initialize = async () => {
      let profile = earQualityProfile(quality);
      const loader = new GLTFLoader();
      const [gltf, crownScene] = await Promise.all([
        loader.loadAsync("/yani/ear-surface.gltf"),
        loader.loadAsync("/yani/yani-crown.glb")
          .then((asset) => asset.scene)
          .catch((error) => {
            console.warn("Yani crown GLB unavailable; using procedural fallback", error);
            return null;
          }),
      ]);
      if (cancelled) {
        [gltf.scene, crownScene].forEach((loadedScene) => loadedScene?.traverse((object) => {
          if (!(object instanceof THREE.Mesh)) return;
          object.geometry.dispose();
          const loadedMaterials = Array.isArray(object.material) ? object.material : [object.material];
          loadedMaterials.forEach((material) => material.dispose());
        }));
        return;
      }
      const renderer = new THREE.WebGLRenderer({
        canvas,
        alpha: true,
        antialias: true,
        depth: true,
        powerPreference: "high-performance",
        premultipliedAlpha: true,
        preserveDrawingBuffer: debugCapture,
      });
      renderer.outputColorSpace = THREE.SRGBColorSpace;
      renderer.toneMapping = THREE.ACESFilmicToneMapping;
      renderer.toneMappingExposure = variant === "core" ? 1.12 : 1.18;
      if (debugCapture) {
        renderer.debug.onShaderError = (gl, program, vertexShader, fragmentShader) => {
          canvas.dataset.shaderError = [
            gl.getProgramInfoLog(program),
            gl.getShaderInfoLog(vertexShader),
            gl.getShaderInfoLog(fragmentShader),
          ].filter(Boolean).join("\n");
        };
      }
      renderer.setClearColor(0x000000, 0);
      renderer.shadowMap.enabled = true;
      renderer.shadowMap.type = THREE.PCFSoftShadowMap;
      renderer.localClippingEnabled = true;

      const scene = new THREE.Scene();
      const atmosphere = variant === "field" ? createEarAtmosphere() : null;
      if (atmosphere) scene.add(atmosphere.background, atmosphere.veil);
      const pmremGenerator = new THREE.PMREMGenerator(renderer);
      const environmentScene = new THREE.Scene();
      environmentScene.background = new THREE.Color(0x060806);
      const environmentAssets: Array<THREE.Mesh<THREE.BufferGeometry, THREE.MeshBasicMaterial>> = [];
      const addEnvironmentStrip = (
        color: number,
        position: THREE.Vector3,
        width: number,
        height: number,
      ) => {
        const panel = new THREE.Mesh(
          new THREE.PlaneGeometry(width, height),
          new THREE.MeshBasicMaterial({ color, side: THREE.DoubleSide }),
        );
        panel.position.copy(position);
        panel.lookAt(0, 0.35, 0);
        environmentScene.add(panel);
        environmentAssets.push(panel);
      };
      addEnvironmentStrip(0xb9e4cf, new THREE.Vector3(-3.4, 1.4, 2.6), 0.72, 5.1);
      addEnvironmentStrip(0xf0d0a9, new THREE.Vector3(2.9, -0.5, 2.1), 1.05, 2.8);
      addEnvironmentStrip(0x7fae98, new THREE.Vector3(0.3, 3.2, -1.8), 3.4, 0.52);
      const environmentTarget = pmremGenerator.fromScene(environmentScene, 0.04, 0.1, 12);
      scene.environment = environmentTarget.texture;
      const camera = new THREE.PerspectiveCamera(variant === "core" ? 31 : 29, 1, 0.1, 30);
      camera.position.set(0, variant === "core" ? 0.31 : 0.23, variant === "core" ? 4.75 : 5.2);
      camera.lookAt(0, 0.32, 0);

      const hemisphere = new THREE.HemisphereLight(0xd9f1e4, 0x372116, 1.24);
      scene.add(hemisphere);
      const keyLight = new THREE.DirectionalLight(0xd8f0e3, 2.72);
      keyLight.position.set(-2.8, 4.5, 4.8);
      keyLight.target.position.set(0, 0.35, 0);
      keyLight.castShadow = true;
      keyLight.shadow.mapSize.set(profile.shadowSize, profile.shadowSize);
      keyLight.shadow.camera.left = -2.25;
      keyLight.shadow.camera.right = 2.25;
      keyLight.shadow.camera.top = 2.25;
      keyLight.shadow.camera.bottom = -1.38;
      keyLight.shadow.camera.near = 0.5;
      keyLight.shadow.camera.far = 12;
      keyLight.shadow.bias = 0.00015;
      keyLight.shadow.normalBias = 0.018;
      scene.add(keyLight, keyLight.target);
      const warmLight = new THREE.PointLight(0xd78b4e, 6.2, 8, 2.1);
      warmLight.position.set(2.3, -0.7, 2.5);
      scene.add(warmLight);
      const rimLight = new THREE.DirectionalLight(0x8fd0b2, 2.2);
      rimLight.position.set(2.5, 2.2, -3.4);
      rimLight.target.position.set(0, 0.6, 0);
      scene.add(rimLight, rimLight.target);
      const fiberLight = new THREE.RectAreaLight(0xc8ead8, 4.2, 0.72, 4.6);
      fiberLight.position.set(-2.35, 1.25, 2.65);
      fiberLight.lookAt(0, 0.45, 0);
      scene.add(fiberLight);

      const shadowPlane = new THREE.Mesh(
        new THREE.PlaneGeometry(7.8, 5.2),
        new THREE.ShadowMaterial({ color: 0x06100c, opacity: variant === "core" ? 0.08 : 0.12 }),
      );
      shadowPlane.position.set(0, 0.2, -0.55);
      shadowPlane.receiveShadow = true;
      scene.add(shadowPlane);

      const geometryByName = (name: string): THREE.BufferGeometry => {
        const mesh = gltf.scene.getObjectByName(name);
        if (!(mesh instanceof THREE.Mesh)) throw new Error(`Yani ear glTF is missing ${name}`);
        return mesh.geometry;
      };
      const sourceGeometry: EarSourceGeometry = {
        front: geometryByName("YaniEarFront"),
        back: geometryByName("YaniEarBack"),
        rim: geometryByName("YaniEarRim"),
        fold: geometryByName("YaniEarFold"),
      };
      const textures = createEarTextures(profile.textureSize);
      const materials = createEarMaterials(textures);
      const crown = crownScene ? createEarCrownAsset(crownScene, textures) : createEarCrown(textures);
      crown.group.visible = variant === "field";
      crown.setDetail(profile.hairRatio);
      scene.add(crown.group);

      const left = createEarRig(sourceGeometry, materials, -1, true);
      const right = createEarRig(sourceGeometry, materials, 1, false);
      Object.values(sourceGeometry).forEach((geometry) => geometry.dispose());

      const spacing = variant === "core" ? 0.62 : 0.7;
      const scale = variant === "core" ? 0.84 : 0.86;
      left.root.position.set(-spacing, variant === "core" ? 0.04 : 0.11, variant === "core" ? 0.035 : -0.1);
      right.root.position.set(spacing, variant === "core" ? 0.02 : 0.085, variant === "core" ? -0.015 : -0.12);
      left.root.scale.set(scale, scale * 1.015, scale);
      right.root.scale.set(scale * 0.98, scale * 0.99, scale);
      left.root.rotation.set(-0.025, 0.075, 0.43);
      right.root.rotation.set(-0.012, -0.065, -0.43);
      scene.add(left.root, right.root);
      left.setDetail(profile.shellCount, profile.hairRatio);
      right.setDetail(profile.shellCount, profile.hairRatio);

      const initialLeft = earTarget({ time: 0, mood, side: -1, pointerX: 0, pointerY: 0, pointerActive: 0 });
      const initialRight = earTarget({ time: 0, mood, side: 1, pointerX: 0, pointerY: 0, pointerActive: 0 });
      let leftSprings = createEarSpringPose(initialLeft);
      let rightSprings = createEarSpringPose(initialRight);
      let elapsed = 0;
      let observedMood = mood;
      let moodBeganAt = 0;
      let capturedMood: EarMood | null = null;
      let appliedQuality = quality;
      let leftMotionEnergy = 0;
      let rightMotionEnergy = 0;
      const pointer = { x: 0, y: 0, targetActive: 0, active: 0 };

      const onPointerMove = (event: PointerEvent) => {
        const rect = canvas.getBoundingClientRect();
        pointer.x = ((event.clientX - rect.left) / Math.max(1, rect.width) - 0.5) * 2;
        pointer.y = -((event.clientY - rect.top) / Math.max(1, rect.height) - 0.5) * 2;
        pointer.targetActive = 1;
        if (debugCapture) {
          capturedMood = null;
          moodBeganAt = elapsed - 0.6;
        }
      };
      const onPointerLeave = () => {
        pointer.targetActive = 0;
        if (debugCapture) {
          capturedMood = null;
          moodBeganAt = elapsed - 0.6;
        }
      };
      if (variant === "field") {
        window.addEventListener("pointermove", onPointerMove);
        window.addEventListener("blur", onPointerLeave);
        document.documentElement.addEventListener("pointerleave", onPointerLeave);
      } else {
        canvas.addEventListener("pointermove", onPointerMove);
        canvas.addEventListener("pointerleave", onPointerLeave);
      }

      const resize = () => {
        const rect = canvas.getBoundingClientRect();
        const pixelRatio = Math.min(window.devicePixelRatio || 1, profile.pixelRatio);
        renderer.setPixelRatio(pixelRatio);
        renderer.setSize(Math.max(1, rect.width), Math.max(1, rect.height), false);
        atmosphere?.resize(Math.max(1, rect.width), Math.max(1, rect.height));
        camera.aspect = Math.max(1, rect.width) / Math.max(1, rect.height);
        camera.updateProjectionMatrix();
      };
      resizeObserver = typeof ResizeObserver === "undefined" ? null : new ResizeObserver(resize);
      resizeObserver?.observe(canvas);
      resize();

      const loop = createRenderLoop((dt) => {
        const state = stateRef.current;
        if (state.quality !== appliedQuality) {
          profile = earQualityProfile(state.quality);
          left.setDetail(profile.shellCount, profile.hairRatio);
          right.setDetail(profile.shellCount, profile.hairRatio);
          crown.setDetail(profile.hairRatio);
          keyLight.shadow.mapSize.set(profile.shadowSize, profile.shadowSize);
          keyLight.shadow.map?.dispose();
          keyLight.shadow.map = null;
          appliedQuality = state.quality;
          resize();
        }
        const effectiveDt = state.paused ? 0 : dt;
        elapsed += effectiveDt;
        if (state.mood !== observedMood) {
          observedMood = state.mood;
          moodBeganAt = elapsed;
          capturedMood = null;
        }
        const pointerResponse = 1 - Math.exp(-Math.min(dt, 0.1) * (pointer.targetActive ? 11 : 3.4));
        pointer.active += (pointer.targetActive - pointer.active) * pointerResponse;
        const leftTarget = earTarget({
          time: elapsed,
          mood: state.mood,
          side: -1,
          pointerX: pointer.x,
          pointerY: pointer.y,
          pointerActive: pointer.active,
        });
        const rightTarget = earTarget({
          time: elapsed,
          mood: state.mood,
          side: 1,
          pointerX: pointer.x,
          pointerY: pointer.y,
          pointerActive: pointer.active,
        });
        leftSprings = stepEarSpringPose(leftSprings, leftTarget, effectiveDt);
        rightSprings = stepEarSpringPose(rightSprings, rightTarget, effectiveDt);
        const leftPose = poseValues(leftSprings);
        const rightPose = poseValues(rightSprings);
        left.applyPose(leftPose);
        right.applyPose(rightPose);
        const energyResponse = 1 - Math.exp(-Math.min(effectiveDt, 0.1) * 7.5);
        leftMotionEnergy += (poseEnergy(leftSprings) - leftMotionEnergy) * energyResponse;
        rightMotionEnergy += (poseEnergy(rightSprings) - rightMotionEnergy) * energyResponse;
        materials.setSssStrength(
          state.mood === "active" ? 1.48 : state.mood === "alarm" ? 0.34 : state.mood === "idle" ? 0.72 : 0.96,
        );
        const sceneBreath = 0.5 + Math.sin(elapsed * 0.36 + Math.sin(elapsed * 0.071) * 0.62) * 0.5;
        crown.update({
          time: elapsed,
          mood: state.mood,
          breath: sceneBreath,
          earEnergy: Math.max(leftMotionEnergy, rightMotionEnergy),
        });
        warmLight.intensity = (state.mood === "active" ? 8.1 : state.mood === "alarm" ? 2.3 : 6.05)
          + Math.sin(elapsed * 0.72) * 0.34
          + sceneBreath * 0.42;
        warmLight.position.x = 2.3 + Math.sin(elapsed * 0.11 + 1.7) * 0.24;
        warmLight.position.y = -0.7 + sceneBreath * 0.12;
        keyLight.intensity = 2.6 + sceneBreath * 0.34 + (state.mood === "scanning" ? 0.22 : 0);
        keyLight.position.x = -2.8 + Math.sin(elapsed * 0.14) * 0.34;
        rimLight.intensity = (state.mood === "scanning" ? 3.1 : state.mood === "alarm" ? 1.25 : 2.2)
          + sceneBreath * 0.2;
        rimLight.position.x = 2.5 + Math.sin(elapsed * 0.13 + 0.8) * 0.28;
        fiberLight.position.x = -2.35 + Math.sin(elapsed * 0.24) * 0.22;
        fiberLight.position.y = 1.25 + Math.sin(elapsed * 0.17 + 1.2) * 0.08;
        fiberLight.lookAt(0, 0.45, 0);
        atmosphere?.update({
          time: elapsed,
          moodAge: elapsed - moodBeganAt,
          mood: state.mood,
          quality: state.quality,
          pointerX: pointer.x,
          pointerY: pointer.y,
          pointerActive: pointer.active,
          leftPose,
          rightPose,
          leftEnergy: leftMotionEnergy,
          rightEnergy: rightMotionEnergy,
        });
        renderer.render(scene, camera);
        if (debugCapture && elapsed - moodBeganAt > 0.55 && capturedMood !== observedMood) {
          const frame = canvas.toDataURL("image/png");
          canvas.dataset.frameCapture = frame;
          setDebugFrame(frame);
          capturedMood = observedMood;
        }
      }, { role: variant === "core" ? "hero" : "field", paused: stateRef.current.paused });
      loopRef.current = loop;
      loop.start();

      cleanupScene = () => {
        if (variant === "field") {
          window.removeEventListener("pointermove", onPointerMove);
          window.removeEventListener("blur", onPointerLeave);
          document.documentElement.removeEventListener("pointerleave", onPointerLeave);
        } else {
          canvas.removeEventListener("pointermove", onPointerMove);
          canvas.removeEventListener("pointerleave", onPointerLeave);
        }
        loop.dispose();
        loopRef.current = null;
        left.dispose();
        right.dispose();
        materials.dispose();
        textures.dispose();
        atmosphere?.dispose();
        crown.dispose();
        environmentAssets.forEach((asset) => {
          asset.geometry.dispose();
          asset.material.dispose();
        });
        environmentTarget.dispose();
        pmremGenerator.dispose();
        shadowPlane.geometry.dispose();
        (shadowPlane.material as THREE.Material).dispose();
        renderer.dispose();
        delete canvas.dataset.frameCapture;
        delete canvas.dataset.shaderError;
        canvas.width = 0;
        canvas.height = 0;
      };
    };

    const tryInitialize = () => {
      initialize().catch((error) => {
        if (cancelled) return;
        const message = error instanceof Error ? error.message : String(error);
        const transientContextFailure = /precision|webgl|context/i.test(message);
        if (transientContextFailure && retryAttempt < 4) {
          retryAttempt += 1;
          retryTimer = window.setTimeout(tryInitialize, 180 * retryAttempt);
          return;
        }
        console.error("Yani ear laboratory failed to initialize", error);
        setFailed(true);
      });
    };
    tryInitialize();
    return () => {
      cancelled = true;
      if (retryTimer !== null) window.clearTimeout(retryTimer);
      resizeObserver?.disconnect();
      cleanupScene?.();
    };
  }, [debugCapture, variant]);

  useEffect(() => {
    loopRef.current?.setPaused(paused);
    loopRef.current?.invalidate();
  }, [mood, paused, quality]);

  if (failed) {
    return <div className={`yani-ear-error ${className}`}>WebGL2 renderer unavailable</div>;
  }
  return (
    <>
      <canvas
        ref={canvasRef}
        aria-label={variant === "core" ? "Yani ear core prototype" : "Yani ear background prototype"}
        data-yani-ear-scene={variant}
        data-mood={mood}
        data-quality={quality}
        className={`${className}${debugFrame ? " yani-ear-capture-source" : ""}`}
      />
      {debugCapture && debugFrame && (
        <img src={debugFrame} aria-hidden="true" className={`${className} yani-ear-capture-frame`} />
      )}
    </>
  );
}
