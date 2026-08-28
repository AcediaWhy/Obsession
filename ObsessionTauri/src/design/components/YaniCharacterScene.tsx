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

// Геометрия + материалы + текстуры поддерева. Вызывается и на обычном
// размонтировании, и когда окно скрылось, пока GLB ещё грузился: иначе 2×2048²
// текстуры персонажа оставались висеть до сборки мусора.
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

// renderer.dispose() отдаёт программы, текстуры и цели рендера, но не буфер
// отрисовки самого канваса — а это самый крупный кусок (4× MSAA цвет + глубина
// на всё поле). Обнуление размера канваса освобождает его сразу: важно для
// ухода в трей, где сцена размонтируется целиком.
//
// forceContextLoss() добивает то, что не отдаёт ни dispose(), ни обнуление
// размера: сам контекст с его декодером команд в GPU-процессе, transfer buffer,
// состоянием устройства и кэшем программ. Иначе всё это ждёт сборки мусора, а в
// трее её не бывает — рендерер задушен, аллокаций нет, idle-задач нет.
//
// Ровно так делает rain/pipeline.ts, и опасение про StrictMode (эффект идёт
// дважды по одному канвасу, второй рендерер получит потерянный контекст) на
// живом приложении не подтвердилось: сцена собирается на том же элементе и
// после потери контекста. Проверено сравнением с принудительным 2D-фолбэком —
// картинки разные, значит работает настоящий конвейер.
function releaseRenderer(renderer: THREE.WebGLRenderer, canvas: HTMLCanvasElement) {
  renderer.dispose();
  canvas.width = 0;
  canvas.height = 0;
  renderer.forceContextLoss();
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
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const loopRef = useRef<RenderLoop | null>(null);
  const stateRef = useRef({ mood, quality, paused, screen });
  stateRef.current = { mood, quality, paused, screen };
  const [failed, setFailed] = useState(false);
  const [debugFrame, setDebugFrame] = useState<string | null>(null);

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    let cancelled = false;
    let renderer: THREE.WebGLRenderer | null = null;
    let resizeObserver: ResizeObserver | null = null;
    let cleanupScene: (() => void) | null = null;
    let environmentTarget: THREE.WebGLRenderTarget | null = null;

    const initialize = async () => {
      // Каким проходом собрана живая сцена: канвас переиспользуется между
      // проходами, и по селекту в лабе этого не видно.
      canvas.dataset.pass = art;
      let profile = earQualityProfile(stateRef.current.quality);
      const activeRenderer = new THREE.WebGLRenderer({
        canvas,
        alpha: true,
        antialias: true,
        depth: true,
        powerPreference: "high-performance",
        premultipliedAlpha: true,
        preserveDrawingBuffer: debugCapture,
      });
      renderer = activeRenderer;
      activeRenderer.outputColorSpace = THREE.SRGBColorSpace;
      activeRenderer.toneMapping = THREE.ACESFilmicToneMapping;
      const scheme = LIGHT_SCHEMES[art];
      activeRenderer.toneMappingExposure = scheme.exposure;
      activeRenderer.setClearColor(0x000000, 0);
      if (debugCapture) {
        activeRenderer.debug.onShaderError = (gl, program, vertexShader, fragmentShader) => {
          canvas.dataset.shaderError = [
            gl.getProgramInfoLog(program),
            gl.getShaderInfoLog(vertexShader),
            gl.getShaderInfoLog(fragmentShader),
          ].filter(Boolean).join("\n");
        };
      }

      const scene = new THREE.Scene();
      const camera = new THREE.PerspectiveCamera(29, 1, 0.1, 30);
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
      const pmrem = new THREE.PMREMGenerator(activeRenderer);
      // Цель PMREM держим в scope эффекта, а не только внутри initialize: она
      // создаётся ДО await за GLB, и при смене темы во время загрузки уборка
      // раньше её не видела — cubemap утекал до смерти контекста.
      const target = pmrem.fromScene(environmentScene, 0.04, 0.1, 12);
      environmentTarget = target;
      scene.environment = target.texture;
      // Окружение — один снимок на всю жизнь сцены: генератор и его временные
      // цели, как и сами полоски, больше не нужны и освобождаются сразу.
      pmrem.dispose();
      disposeSubtree(environmentScene);

      const gltf = await sharedLoader.loadAsync(modelUrl);
      if (cancelled) {
        disposeSubtree(gltf.scene);
        return;
      }
      const character = gltf.scene;
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
      canvas.dataset.modelReady = "true";
      canvas.dataset.modelSize = `${size.x.toFixed(3)},${size.y.toFixed(3)},${size.z.toFixed(3)}`;

      const mixer = new THREE.AnimationMixer(character);
      const actions = new Map(gltf.animations.map((clip) => [clip.name, mixer.clipAction(clip)]));
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
        activeRenderer.setPixelRatio(pixelRatio);
        fieldWidth = Math.max(1, rect.width);
        fieldHeight = Math.max(1, rect.height);
        activeRenderer.setSize(fieldWidth, fieldHeight, false);
        applyFrame();
      };
      resizeObserver = typeof ResizeObserver === "undefined" ? null : new ResizeObserver(resize);
      resizeObserver?.observe(canvas);
      resize();

      const loop = createRenderLoop((dt) => {
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
        character.position.y = -center.y * fitScale + 0.08 + breath;
        // Эти две строки перетирают конструкторские intensity каждый кадр,
        // поэтому базы и амплитуды берутся из схемы прохода.
        warm.intensity = scheme.warm.base + Math.sin(elapsed * 0.61) * scheme.warm.swing;
        key.intensity = scheme.key.base + Math.sin(elapsed * 0.37 + 1.2) * scheme.key.swing;
        activeRenderer.render(scene, camera);
        if (debugCapture && elapsed > 0.55 && capturedMood !== state.mood) {
          const frame = canvas.toDataURL("image/png");
          canvas.dataset.frameCapture = frame;
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

      cleanupScene = () => {
        loop.dispose();
        loopRef.current = null;
        mixer.stopAllAction();
        mixer.uncacheRoot(character);
        disposeSubtree(character);
        environmentTarget?.dispose();
        environmentTarget = null;
        releaseRenderer(activeRenderer, canvas);
        renderer = null;
      };
    };

    initialize().catch((error) => {
      console.error("Yani character scene failed", error);
      if (!cancelled) {
        canvas.dataset.modelError = error instanceof Error ? error.message : String(error);
        setFailed(true);
      }
    });
    return () => {
      cancelled = true;
      resizeObserver?.disconnect();
      if (cleanupScene) {
        cleanupScene();
      } else {
        // Уборка до того, как сцена собралась: за GLB ушли в await, а цель PMREM
        // и рендерер уже созданы. Раньше эта ветка отпускала только рендерер, и
        // при смене темы во время загрузки модели cubemap оставался висеть.
        environmentTarget?.dispose();
        environmentTarget = null;
        if (renderer) {
          releaseRenderer(renderer, canvas);
          renderer = null;
        }
      }
    };
  }, [art, debugCapture, modelUrl]);

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
      <canvas
        ref={canvasRef}
        aria-label="Animated Yani Neko character prototype"
        data-yani-character-scene="field"
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
