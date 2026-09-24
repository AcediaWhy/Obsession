import { useEffect, useRef, useState } from "react";
import * as THREE from "three";
import { GLTFLoader } from "three/addons/loaders/GLTFLoader.js";

import { createRenderLoop, renderStill, type RenderLoop } from "../render";
import {
  lerpYaniFraming,
  yaniCharacterViewFrame,
  yaniFramingForScreen,
  type YaniFraming,
} from "./yaniCharacterFrame";
import { earQualityProfile } from "./yaniEar/quality";
import type { EarMood, EarQuality, YaniArtPass } from "./yaniEar/types";

type Props = {
  mood: EarMood;
  quality: EarQuality;
  paused?: boolean;
  /**
   * Экран приложения. Задан — сцена кадрируется как поле темы (модель справа,
   * зум как у бывшего CSS-трансформа). Не задан (лаборатории) — камера смотрит
   * ровно на канвас, без выреза фрустума.
   */
  screen?: string;
  className?: string;
  debugCapture?: boolean;
  modelUrl?: string;
  art?: YaniArtPass;
};

// Длительность бывшего CSS-перехода `transition: transform 700ms ease` у
// .yani-character-field__model: композиция экрана переехала в камеру, значит и
// переход между композициями теперь считается здесь.
const FRAMING_TRANSITION_S = 0.7;

// Схемы света по художественному проходу. base/swing — это то, чем каждый кадр
// перетираются intensity ключевого и тёплого источников в цикле, поэтому
// значения живут здесь, а не в конструкторах.
type LightScheme = {
  exposure: number;
  envMapIntensity: number;
  hemisphere: { sky: number; ground: number; intensity: number };
  key: { color: number; position: [number, number, number]; base: number; swing: number };
  warm: {
    color: number;
    distance: number;
    decay: number;
    position: [number, number, number];
    base: number;
    swing: number;
  };
  rim: { color: number; intensity: number; position: [number, number, number] };
  // Окружение запекается в PMREM один раз за жизнь сцены и даёт все отражения
  // плюс половину ambient — то есть красить форму дешевле здесь, чем добавлять
  // источники. Полоски смотрят в центр модели.
  environment: {
    background: number;
    strips: readonly {
      color: number;
      position: [number, number, number];
      width: number;
      height: number;
    }[];
  };
};

const LIGHT_SCHEMES: Record<YaniArtPass, LightScheme> = {
  current: {
    exposure: 1.14,
    envMapIntensity: 1.08,
    hemisphere: { sky: 0xdaf1e5, ground: 0x302018, intensity: 1.55 },
    key: { color: 0xd8f0e3, position: [-2.8, 4.2, 4.8], base: 3.05, swing: 0.2 },
    warm: { color: 0xe29559, distance: 8, decay: 2.1, position: [2.4, -0.4, 2.6], base: 6.9, swing: 0.46 },
    rim: { color: 0x8fd0b2, intensity: 2.5, position: [2.5, 2.1, -3.4] },
    environment: {
      background: 0x10130f,
      strips: [
        { color: 0xb9e4cf, position: [-3.4, 1.4, 2.6], width: 0.72, height: 5.1 },
        { color: 0xf0c69b, position: [2.9, -0.5, 2.1], width: 1.05, height: 2.8 },
        { color: 0x789b89, position: [0.3, 3.2, -1.8], width: 3.4, height: 0.52 },
      ],
    },
  },
};

const CLIP_FOR_MOOD: Record<EarMood, "Idle" | "Active" | "Scanning" | "Alarm"> = {
  idle: "Idle",
  busy: "Active",
  active: "Active",
  scanning: "Scanning",
  alarm: "Alarm",
};

function disposeMaterial(material: THREE.Material) {
  Object.values(material).forEach((value) => {
    if (value instanceof THREE.Texture) value.dispose();
  });
  material.dispose();
}

// Байты GLB кэшируются в куче, а разбор идёт заново на каждый монтаж. Сцена
// размонтируется при уходе в трей и при смене темы, то есть загрузка повторяется
// регулярно: без кэша каждый показ окна — это снова 3 МБ с диска. Кэшировать
// разобранную сцену нельзя, её геометрии и текстуры освобождаются на выходе;
// а 2×2048² текстуры, оставленные в GPU «на будущее», как раз и держали бы
// память в трее, которую размонтирование освобождает.
THREE.Cache.enabled = true;
const sharedLoader = new GLTFLoader();

// Геометрия + материалы + текстуры поддерева. Вызывается при выпуске сцены
// (потеря контекста) — на обычном размонтировании поля сцена больше не
// разбирается, она живёт в персистентном кэше модуля.
function disposeSubtree(root: THREE.Object3D) {
  const materials = new Set<THREE.Material>();
  root.traverse((object) => {
    if (!(object instanceof THREE.Mesh)) return;
    object.geometry.dispose();
    (Array.isArray(object.material) ? object.material : [object.material]).forEach((material) =>
      materials.add(material),
    );
  });
  materials.forEach(disposeMaterial);
}

// Сцена Yani сохраняет canvas, WebGLRenderer и модель между монтированиями.
// В CDP-замере от 2026-08-29 повторное создание WebGL-контекста добавляло
// процессу рендеринга 13–46 МБ приватной памяти за переключение темы.
// При потере контекста сцена помечается broken и создаётся заново.
type YaniStage = {
  canvas: HTMLCanvasElement;
  renderer: THREE.WebGLRenderer;
  scene: THREE.Scene;
  camera: THREE.PerspectiveCamera;
  mixer: THREE.AnimationMixer;
  actions: Map<string, THREE.AnimationAction>;
  character: THREE.Object3D;
  scheme: LightScheme;
  keyLight: THREE.DirectionalLight;
  warmLight: THREE.PointLight;
  // Достаточно для дыхания (position.y каждый кадр): -centerY*fitScale + 0.08.
  breathBase: number;
  environmentTarget: THREE.WebGLRenderTarget;
  broken: boolean;
};

const stageCache = new Map<string, Promise<YaniStage>>();

function releaseStage(stage: YaniStage) {
  stage.broken = true;
  try {
    // Поля могут быть null: сцена умеет падать ДО миксера/модели (нет WebGL,
    // CSP срезал fetch GLB, упал PMREM) — выпуск не должен маскировать ту
    // ошибку своей собственной.
    if (stage.mixer) {
      stage.mixer.stopAllAction();
      stage.mixer.uncacheRoot(stage.character);
    }
    if (stage.character) disposeSubtree(stage.character);
    stage.environmentTarget?.dispose();
    stage.renderer?.dispose();
  } finally {
    // Сцена выпускается только вместе с контекстом (потеря/пересборка), поэтому
    // добить контекст здесь — то, ради чего releaseRenderer существовал раньше.
    stage.canvas.width = 0;
    stage.canvas.height = 0;
    stage.renderer?.forceContextLoss();
  }
}

function buildStage(modelUrl: string, art: YaniArtPass, debugCapture: boolean): Promise<YaniStage> {
  return (async () => {
    const canvas = document.createElement("canvas");
    const renderer = new THREE.WebGLRenderer({
      canvas,
      alpha: true,
      antialias: true,
      depth: true,
      premultipliedAlpha: true,
      preserveDrawingBuffer: debugCapture,
    });
    const stage: YaniStage = {
      canvas,
      renderer,
      scene: new THREE.Scene(),
      camera: new THREE.PerspectiveCamera(29, 1, 0.1, 30),
      mixer: null as unknown as THREE.AnimationMixer,
      actions: new Map(),
      character: null as unknown as THREE.Object3D,
      scheme: LIGHT_SCHEMES[art],
      keyLight: null as unknown as THREE.DirectionalLight,
      warmLight: null as unknown as THREE.PointLight,
      breathBase: 0,
      environmentTarget: null as unknown as THREE.WebGLRenderTarget,
      broken: false,
    };
    try {
      renderer.outputColorSpace = THREE.SRGBColorSpace;
      renderer.toneMapping = THREE.ACESFilmicToneMapping;
      const scheme = stage.scheme;
      renderer.toneMappingExposure = scheme.exposure;
      renderer.setClearColor(0x000000, 0);
      if (debugCapture) {
        renderer.debug.onShaderError = (gl, program, vertexShader, fragmentShader) => {
          canvas.dataset.shaderError = [
            gl.getProgramInfoLog(program),
            gl.getShaderInfoLog(vertexShader),
            gl.getShaderInfoLog(fragmentShader),
          ].filter(Boolean).join("\n");
        };
      }

      const { scene, camera } = stage;
      camera.position.set(3.4, 2.4, 3.4);
      camera.lookAt(0, 0.18, 0);

      const hemisphere = new THREE.HemisphereLight(
        scheme.hemisphere.sky,
        scheme.hemisphere.ground,
        scheme.hemisphere.intensity,
      );
      const key = new THREE.DirectionalLight(scheme.key.color, scheme.key.base);
      key.position.set(...scheme.key.position);
      const warm = new THREE.PointLight(
        scheme.warm.color,
        scheme.warm.base,
        scheme.warm.distance,
        scheme.warm.decay,
      );
      warm.position.set(...scheme.warm.position);
      stage.keyLight = key;
      stage.warmLight = warm;
      const rim = new THREE.DirectionalLight(scheme.rim.color, scheme.rim.intensity);
      rim.position.set(...scheme.rim.position);
      // RectAreaLight здесь не было смысла: без RectAreaLightUniformsLib.init()
      // three не заполняет LTC-таблицы, свет не даёт вклада — но добавляет в
      // шейдер весь LTC-путь (две выборки текстур и четыре form factor'а на
      // фрагмент). Мягкую подсветку слева уже даёт environment из PMREM.
      scene.add(hemisphere, key, warm, rim);

      const environmentScene = new THREE.Scene();
      environmentScene.background = new THREE.Color(scheme.environment.background);
      const addStrip = (color: number, position: THREE.Vector3, width: number, height: number) => {
        const strip = new THREE.Mesh(
          new THREE.PlaneGeometry(width, height),
          new THREE.MeshBasicMaterial({ color, side: THREE.DoubleSide }),
        );
        strip.position.copy(position);
        strip.lookAt(0, 0.2, 0);
        environmentScene.add(strip);
      };
      for (const strip of scheme.environment.strips) {
        addStrip(strip.color, new THREE.Vector3(...strip.position), strip.width, strip.height);
      }
      const pmrem = new THREE.PMREMGenerator(renderer);
      const target = pmrem.fromScene(environmentScene, 0.04, 0.1, 12);
      stage.environmentTarget = target;
      scene.environment = target.texture;
      // Окружение — один снимок на всю жизнь сцены: генератор и его временные
      // цели, как и сами полоски, больше не нужны и освобождаются сразу.
      pmrem.dispose();
      disposeSubtree(environmentScene);

      const gltf = await sharedLoader.loadAsync(modelUrl);
      const character = gltf.scene;
      stage.character = character;
      character.name = "yani-character-animated";
      const bounds = new THREE.Box3().setFromObject(character);
      const center = bounds.getCenter(new THREE.Vector3());
      const size = bounds.getSize(new THREE.Vector3());
      const fitScale = 1.58 / Math.max(size.y, size.x * 0.82, 0.001);
      character.position.set(-center.x * fitScale, -center.y * fitScale + 0.08, -center.z * fitScale);
      character.scale.setScalar(fitScale);
      character.traverse((object) => {
        if (!(object instanceof THREE.Mesh)) return;
        if (/^(Cube|Icosphere)$/.test(object.name)) {
          object.visible = false;
          return;
        }
        object.castShadow = false;
        object.receiveShadow = false;
        object.frustumCulled = false;
        const materials = Array.isArray(object.material) ? object.material : [object.material];
        materials.forEach((material) => {
          if (material instanceof THREE.MeshStandardMaterial) {
            material.envMapIntensity = scheme.envMapIntensity;
          }
          // `doubleSided: true` в GLB — дефолт экспорта Blender, а не решение:
          // персонаж замкнут, задние грани всё равно перекрыты передними, но
          // без culling они шейдятся. Плоскому мешу контактной тени culling
          // противопоказан — квад может смотреть от камеры.
          if (material.name !== "YaniContactShadowMaterial") {
            material.side = THREE.FrontSide;
          }
        });
      });
      scene.add(character);
      stage.breathBase = -center.y * fitScale + 0.08;
      canvas.dataset.modelReady = "true";
      canvas.dataset.modelSize = `${size.x.toFixed(3)},${size.y.toFixed(3)},${size.z.toFixed(3)}`;

      stage.mixer = new THREE.AnimationMixer(character);
      stage.actions = new Map(gltf.animations.map((clip) => [clip.name, stage.mixer.clipAction(clip)]));
      canvas.addEventListener("webglcontextlost", () => {
        stage.broken = true;
      });
      return stage;
    } catch (error) {
      releaseStage(stage);
      throw error;
    }
  })();
}

function acquireStage(modelUrl: string, art: YaniArtPass, debugCapture: boolean): Promise<YaniStage> {
  const key = `${modelUrl}|${art}|${debugCapture ? "capture" : "live"}`;
  let pending = stageCache.get(key);
  if (!pending) {
    pending = buildStage(modelUrl, art, debugCapture).then(
      (stage) => {
        if (!stage.broken) return stage;
        // Контекст сцены потерян: выпускаем мёртвый этап и собираем новый.
        releaseStage(stage);
        const retry = buildStage(modelUrl, art, debugCapture);
        stageCache.set(key, retry);
        return retry;
      },
      (error) => {
        // Упавшую сборку из кэша выкидываем: следующий маунт пробует заново,
        // а не наследует чужой rejection навсегда.
        stageCache.delete(key);
        throw error;
      },
    );
    stageCache.set(key, pending);
  }
  return pending;
}

/** Выгрузить все живые сцены (трей-выгрузка): контексты и модель уходят из
 *  памяти, возврат в тему пересобирает стейдж один раз (~1 с). */
export function releaseYaniStages(): void {
  for (const pending of stageCache.values()) {
    void pending
      .then((stage) => {
        if (!stage.broken) releaseStage(stage);
      })
      .catch(() => {});
  }
  stageCache.clear();
}

export function YaniCharacterScene({
  mood,
  quality,
  paused = false,
  screen,
  className = "",
  debugCapture = false,
  modelUrl = "/yani/yani-character.glb",
  art = "current",
}: Props) {
  const containerRef = useRef<HTMLDivElement>(null);
  const loopRef = useRef<RenderLoop | null>(null);
  const stateRef = useRef({ mood, quality, paused, screen });
  stateRef.current = { mood, quality, paused, screen };
  const [failed, setFailed] = useState(false);
  const [debugFrame, setDebugFrame] = useState<string | null>(null);

  useEffect(() => {
    const container = containerRef.current;
    if (!container) return;
    let cancelled = false;
    let loop: RenderLoop | null = null;
    let boundCanvas: HTMLCanvasElement | null = null;
    let onContextLost: ((event: Event) => void) | null = null;

    const initialize = async () => {
      const stage = await acquireStage(modelUrl, art, debugCapture);
      if (cancelled) return;
      const canvas = stage.canvas;
      // Каким проходом собрана живая сцена: канвас переиспользуется между
      // проходами, и по селекту в лабе этого не видно.
      canvas.dataset.pass = art;
      canvas.className = className;
      container.appendChild(canvas);
      boundCanvas = canvas;
      onContextLost = (event: Event) => {
        event.preventDefault();
        stage.broken = true;
        setFailed(true);
      };
      canvas.addEventListener("webglcontextlost", onContextLost);

      const { renderer, scene, camera, mixer, actions, character, scheme, keyLight, warmLight } = stage;
      let profile = earQualityProfile(stateRef.current.quality);
      let currentClipName = CLIP_FOR_MOOD[stateRef.current.mood];
      let currentAction = actions.get(currentClipName) ?? null;
      currentAction?.reset().fadeIn(0).play();
      let appliedQuality = stateRef.current.quality;
      let elapsed = 0;
      let capturedMood: EarMood | null = null;

      // Композиция кадра: канвас лежит ровно по полю, а бывшая CSS-рамка
      // (120%×108% + translateX/scale) воспроизводится вырезом фрустума. Раньше
      // 38% отрисованных пикселей обрезал overflow:hidden поля.
      let framingScreen = stateRef.current.screen;
      let framing: YaniFraming = yaniFramingForScreen(framingScreen ?? "overview");
      let framingFrom = framing;
      let framingProgress = 1;
      let fieldWidth = 1;
      let fieldHeight = 1;

      const applyFrame = () => {
        if (framingScreen === undefined) {
          // Лаборатории смотрят на сцену целиком: канвас и есть весь кадр.
          camera.clearViewOffset();
          camera.aspect = fieldWidth / fieldHeight;
          camera.updateProjectionMatrix();
          return;
        }
        const frame = yaniCharacterViewFrame(fieldWidth, fieldHeight, framing);
        // setViewOffset сам ставит aspect = fullWidth/fullHeight и вызывает
        // updateProjectionMatrix, поэтому отдельно их трогать не нужно.
        camera.setViewOffset(
          frame.fullWidth,
          frame.fullHeight,
          frame.offsetX,
          frame.offsetY,
          frame.width,
          frame.height,
        );
      };

      const resize = () => {
        const rect = canvas.getBoundingClientRect();
        // Больше 1.5 device-пикселя на CSS-пиксель здесь не читается, а 4× MSAA
        // от такого буфера стоит десятки мегабайт.
        const pixelRatio = Math.min(window.devicePixelRatio || 1, profile.pixelRatio, 1.5);
        renderer.setPixelRatio(pixelRatio);
        fieldWidth = Math.max(1, rect.width);
        fieldHeight = Math.max(1, rect.height);
        renderer.setSize(fieldWidth, fieldHeight, false);
        applyFrame();
      };
      const resizeObserver = typeof ResizeObserver === "undefined" ? null : new ResizeObserver(resize);
      resizeObserver?.observe(canvas);
      resize();

      loop = createRenderLoop((dt) => {
        const state = stateRef.current;
        if (state.quality !== appliedQuality) {
          profile = earQualityProfile(state.quality);
          appliedQuality = state.quality;
          resize();
        }
        const desiredClipName = CLIP_FOR_MOOD[state.mood];
        if (desiredClipName !== currentClipName) {
          const nextAction = actions.get(desiredClipName) ?? null;
          currentAction?.fadeOut(0.22);
          nextAction?.reset().fadeIn(0.22).play();
          currentClipName = desiredClipName;
          currentAction = nextAction;
          capturedMood = null;
        }
        // Смена экрана: под reduce-motion CSS сбрасывал transition в 0.01ms, то
        // есть прыгал в цель — повторяем это, иначе замерший кадр остался бы на
        // полпути между композициями. Лаборатории (screen === undefined) вообще
        // не кадрируются, поэтому и переход им не нужен.
        if (state.screen !== framingScreen) {
          framingFrom = framing;
          framingScreen = state.screen;
          framingProgress = state.screen === undefined || renderStill() ? 1 : 0;
          if (framingProgress === 1) {
            framing = yaniFramingForScreen(state.screen ?? "overview");
            applyFrame();
          }
        }
        if (framingProgress < 1 && framingScreen !== undefined) {
          framingProgress = Math.min(1, framingProgress + Math.max(0, dt) / FRAMING_TRANSITION_S);
          // smoothstep вместо cubic-bezier(.25,.1,.25,1): на 3% сдвига разница
          // между кривыми не читается, а считается это одной строкой.
          const eased = framingProgress * framingProgress * (3 - 2 * framingProgress);
          framing = lerpYaniFraming(framingFrom, yaniFramingForScreen(framingScreen), eased);
          applyFrame();
        }
        const effectiveDt = state.paused ? 0 : Math.min(dt, 0.1);
        elapsed += effectiveDt;
        mixer.update(effectiveDt);
        const breath = Math.sin(elapsed * 0.7) * 0.008;
        character.position.y = stage.breathBase + breath;
        // Эти две строки перетирают конструкторские intensity каждый кадр,
        // поэтому базы и амплитуды берутся из схемы прохода.
        warmLight.intensity = scheme.warm.base + Math.sin(elapsed * 0.61) * scheme.warm.swing;
        keyLight.intensity = scheme.key.base + Math.sin(elapsed * 0.37 + 1.2) * scheme.key.swing;
        renderer.render(scene, camera);
        if (debugCapture && elapsed > 0.55 && capturedMood !== state.mood) {
          const frame = canvas.toDataURL("image/png");
          canvas.dataset.frameCapture = frame;
          canvas.classList.add("yani-ear-capture-source");
          setDebugFrame(frame);
          capturedMood = state.mood;
        }
      }, {
        role: "field",
        paused: stateRef.current.paused,
        // Каденция — по герцовке монитора, без собственного капа: замер показал,
        // что 60→180 кадров сцены стоит всего +0.5 п.п. CPU (модель дешёвая:
        // 2 меша, 31k треугольников, канвас 0.88 Мп). Тир качества всё равно
        // снизит частоту сам, если машина начнёт не успевать.
      });
      loopRef.current = loop;
      loop.start();

      // Канвас мог быть обнулён при прошлом размонтировании — после bind и
      // resize() буфер снова живой, но первый кадр просим нарисовать сразу.
      loop.invalidate();
    };

    initialize().catch((error) => {
      console.error("Yani character scene failed", error);
      if (!cancelled) {
        setFailed(true);
      }
    });
    return () => {
      cancelled = true;
      if (boundCanvas && onContextLost) {
        boundCanvas.removeEventListener("webglcontextlost", onContextLost);
      }
      loop?.dispose();
      loopRef.current = null;
      // Сцена остаётся жить в кэше модуля (gl-сессия темы); drawing buffer
      // отпускаем, чтобы скрытое поле не держало полноэкранный буфер в трее.
      if (boundCanvas) {
        boundCanvas.width = 0;
        boundCanvas.height = 0;
      }
    };
  }, [art, className, debugCapture, modelUrl]);

  // invalidate() как в YaniNekoField: под reduce-motion цикл рисует один кадр и
  // замирает, поэтому смену настроения, качества или экрана нужно попросить
  // перерисовать вручную — иначе стоп-кадр остаётся от прошлого состояния.
  useEffect(() => {
    loopRef.current?.setPaused(paused);
    loopRef.current?.invalidate();
  }, [mood, paused, quality, screen]);

  if (failed) return <div className={`yani-ear-error ${className}`}>Animated Yani GLB unavailable</div>;
  return (
    <>
      {/* Контейнер не участвует в раскладке: канвас персистентен и позиционируется
          собственным классом (например .yani-character-field__model) относительно
          общего предка, как и раньше. */}
      <div
        ref={containerRef}
        aria-hidden="true"
        data-yani-character-scene="field"
        data-mood={mood}
        data-quality={quality}
        style={{ display: "contents" }}
      />
      {debugCapture && debugFrame && (
        <img src={debugFrame} aria-hidden="true" className={`${className} yani-ear-capture-frame`} />
      )}
    </>
  );
}
