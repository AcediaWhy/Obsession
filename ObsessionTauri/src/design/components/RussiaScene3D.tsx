import { useEffect, useMemo, useRef, useState } from "react";
import { Canvas, useFrame, useThree } from "@react-three/fiber";
import {
  EffectComposer,
  Bloom,
  DepthOfField,
  Vignette,
  Noise,
} from "@react-three/postprocessing";
import { BlendFunction } from "postprocessing";
import * as THREE from "three";

import { useDpiStore } from "../../store/dpiStore";
import { useProxyStore } from "../../store/proxyStore";
import { onRenderActiveChange, renderActive } from "../render";

// 3D-версия темы «Russia» на React Three Fiber. Тот же сюжет, что и у 2D
// `RussiaField` — зимняя ночь, одинокий натриевый фонарь, метель, — но с
// НАСТОЯЩЕЙ глубиной: экспоненциальный туман (атмосферная перспектива),
// освещённая плоскость земли (свет фонаря ложится тёплым пятном на снег),
// меш фонаря с реальным SpotLight, волюметрический конус на шейдере, снег
// частицами с расфокусом (DOF), силуэты домов, уходящие в туман.
//
// Состояние active/hot (DPI/прокси): в покое свет тускл и мерцает; при
// активации горит ровнее и теплее — «стало чуть спокойнее».
//
// Грузится лениво (three.js только на этой теме) и с откатом на 2D
// `RussiaField` через error boundary в `HeroField`.

// ─── Палитра (зеркалит 2D-версию) ──────────────────────────────────────────
const FOG_COLOR = 0x0a0c13; // глубокий стальной сумрак
const GROUND_COLOR = 0x0e131d; // тёмный снег в тени
const SNOW_COLOR = 0xcdd8eb; // холодные хлопья
const SODIUM_COOL = new THREE.Color(0xffcf8f); // натриевый свет в покое
const SODIUM_WARM = new THREE.Color(0xffca78); // теплее при активации

// Положение фонаря в мире (чуть правее центра, как lampX≈0.66 в 2D).
const LAMP = new THREE.Vector3(2.6, 0, -4);
const LAMP_H = 5.0; // высота столба
const BULB = new THREE.Vector3(LAMP.x - 0.55, LAMP_H, LAMP.z + 0.2); // лампа на «кобре»

// ─── Мягкий круглый спрайт снежинки (радиальный градиент, как softSprite 2D) ─
function makeFlakeTexture(): THREE.Texture {
  const s = 64;
  const c = document.createElement("canvas");
  c.width = c.height = s;
  const cc = c.getContext("2d")!;
  const g = cc.createRadialGradient(s / 2, s / 2, 0, s / 2, s / 2, s / 2);
  g.addColorStop(0, "rgba(255,255,255,1)");
  g.addColorStop(0.4, "rgba(255,255,255,0.55)");
  g.addColorStop(1, "rgba(255,255,255,0)");
  cc.fillStyle = g;
  cc.fillRect(0, 0, s, s);
  const tex = new THREE.CanvasTexture(c);
  tex.colorSpace = THREE.SRGBColorSpace;
  return tex;
}

// ─── Земля: тёмная снежная плоскость, ловит пятно света фонаря ──────────────
function Ground() {
  return (
    <mesh rotation-x={-Math.PI / 2} position-y={0} receiveShadow>
      <planeGeometry args={[300, 300]} />
      <meshStandardMaterial color={GROUND_COLOR} roughness={0.96} metalness={0} />
    </mesh>
  );
}

// ─── Фонарь: столб + «кобра» + светящаяся лампа ─────────────────────────────
function Lamp({ warmRef }: { warmRef: React.MutableRefObject<number> }) {
  const bulbMat = useRef<THREE.MeshStandardMaterial>(null);

  useFrame(() => {
    const w = warmRef.current;
    if (bulbMat.current) {
      bulbMat.current.emissiveIntensity = 1.6 + w * 1.4;
      bulbMat.current.emissive.copy(SODIUM_COOL).lerp(SODIUM_WARM, w);
    }
  });

  return (
    <group position={[LAMP.x, 0, LAMP.z]}>
      {/* Столб. */}
      <mesh position={[0, LAMP_H / 2, 0]} castShadow>
        <cylinderGeometry args={[0.06, 0.09, LAMP_H, 10]} />
        <meshStandardMaterial color={0x0a0d13} roughness={0.7} metalness={0.3} />
      </mesh>
      {/* Кронштейн-«кобра». */}
      <mesh position={[-0.3, LAMP_H - 0.05, 0.1]} rotation-z={0.5}>
        <cylinderGeometry args={[0.05, 0.05, 0.9, 8]} />
        <meshStandardMaterial color={0x0a0d13} roughness={0.7} metalness={0.3} />
      </mesh>
      {/* Плафон. */}
      <mesh position={[BULB.x - LAMP.x, BULB.y, BULB.z - LAMP.z]}>
        <sphereGeometry args={[0.16, 16, 16]} />
        <meshStandardMaterial
          ref={bulbMat}
          color={0x1a140c}
          emissive={SODIUM_COOL}
          emissiveIntensity={1.8}
        />
      </mesh>
    </group>
  );
}

// ─── Реальный свет фонаря + анимация мерцания/тепла ─────────────────────────
function LampLight({ warmRef }: { warmRef: React.MutableRefObject<number> }) {
  const spot = useRef<THREE.SpotLight>(null);
  const { scene } = useThree();
  const t = useRef(0);

  // Цель спотлайта — отдельный объект: его нужно добавить в сцену, иначе
  // three не учитывает направление света. Ставим на землю у основания фонаря.
  const target = useMemo(() => new THREE.Object3D(), []);
  useEffect(() => {
    target.position.set(LAMP.x - 1.4, 0, LAMP.z + 1.4);
    scene.add(target);
    if (spot.current) spot.current.target = target;
    return () => {
      scene.remove(target);
    };
  }, [scene, target]);

  useFrame((_, dt) => {
    t.current += dt;
    const w = warmRef.current;
    // Мерцание — заметное в покое, почти исчезает при активации.
    const flick =
      1 - (1 - w) * 0.16 * (0.5 + 0.5 * Math.sin(t.current * 9 + Math.sin(t.current * 3)));
    if (spot.current) {
      spot.current.intensity = (30 + w * 18) * flick;
      spot.current.color.copy(SODIUM_COOL).lerp(SODIUM_WARM, w);
    }
  });

  return (
    <spotLight
      ref={spot}
      position={[BULB.x, BULB.y, BULB.z]}
      angle={0.62}
      penumbra={0.75}
      distance={34}
      decay={1.4}
      intensity={30}
      color={SODIUM_COOL}
    />
  );
}

// ─── Волюметрический конус света (шейдер: ярче у лампы, тает к земле) ────────
function LightCone({ warmRef }: { warmRef: React.MutableRefObject<number> }) {
  const t = useRef(0);
  const material = useMemo(() => {
    return new THREE.ShaderMaterial({
      transparent: true,
      depthWrite: false,
      blending: THREE.AdditiveBlending,
      side: THREE.DoubleSide,
      uniforms: {
        uColor: { value: new THREE.Color(0xe6be78) },
        uIntensity: { value: 0.14 },
        uHeight: { value: LAMP_H },
      },
      vertexShader: /* glsl */ `
        varying float vY;
        void main() {
          vY = position.y;
          gl_Position = projectionMatrix * modelViewMatrix * vec4(position, 1.0);
        }
      `,
      fragmentShader: /* glsl */ `
        varying float vY;
        uniform vec3 uColor;
        uniform float uIntensity;
        uniform float uHeight;
        void main() {
          // position.y идёт от -h/2 (низ, у земли) до +h/2 (верх, у лампы).
          float a = clamp((vY + uHeight * 0.5) / uHeight, 0.0, 1.0);
          // Ярче у лампы, мягко гаснет к земле.
          float glow = pow(a, 1.6);
          gl_FragColor = vec4(uColor * glow, glow * uIntensity);
        }
      `,
    });
  }, []);

  useFrame((_, dt) => {
    t.current += dt;
    const w = warmRef.current;
    const flick =
      1 - (1 - w) * 0.16 * (0.5 + 0.5 * Math.sin(t.current * 9 + Math.sin(t.current * 3)));
    material.uniforms.uIntensity.value = (0.12 + w * 0.06) * flick;
  });

  useEffect(() => () => material.dispose(), [material]);

  // Конус: узкий у лампы (apex сверху), широкий у земли (base снизу).
  return (
    <mesh position={[LAMP.x - 0.6, LAMP_H / 2, LAMP.z + 0.5]} rotation-z={0.06}>
      <coneGeometry args={[2.3, LAMP_H, 40, 1, true]} />
      <primitive object={material} attach="material" />
    </mesh>
  );
}

// ─── Снег: облако частиц, падает с ветром, рециклится ────────────────────────
function Snow({
  count,
  size,
  area,
  speed,
  opacity,
  texture,
}: {
  count: number;
  size: number;
  area: { x: number; y: number; z: [number, number] };
  speed: number;
  opacity: number;
  texture: THREE.Texture;
}) {
  const pointsRef = useRef<THREE.Points>(null);

  // Позиции + индивидуальные скорости/фаза покачивания.
  const { geometry, velocities, phases } = useMemo(() => {
    const positions = new Float32Array(count * 3);
    const vel = new Float32Array(count);
    const ph = new Float32Array(count);
    for (let i = 0; i < count; i++) {
      positions[i * 3] = (Math.random() - 0.5) * area.x;
      positions[i * 3 + 1] = Math.random() * area.y;
      positions[i * 3 + 2] = area.z[0] + Math.random() * (area.z[1] - area.z[0]);
      vel[i] = speed * (0.7 + Math.random() * 0.6);
      ph[i] = Math.random() * Math.PI * 2;
    }
    const geo = new THREE.BufferGeometry();
    geo.setAttribute("position", new THREE.BufferAttribute(positions, 3));
    return { geometry: geo, velocities: vel, phases: ph };
  }, [count, size, speed, area.x, area.y, area.z[0], area.z[1]]);

  const t = useRef(0);
  useFrame((_, dt) => {
    t.current += dt;
    const arr = geometry.attributes.position.array as Float32Array;
    for (let i = 0; i < count; i++) {
      const j = i * 3;
      arr[j + 1] -= velocities[i] * dt; // падение
      arr[j] -= velocities[i] * dt * 0.5; // ветер вбок
      arr[j] += Math.sin(t.current * 0.8 + phases[i]) * dt * 0.25; // покачивание
      if (arr[j + 1] < 0 || arr[j] < -area.x * 0.6) {
        arr[j] = (Math.random() - 0.5) * area.x;
        arr[j + 1] = area.y;
        arr[j + 2] = area.z[0] + Math.random() * (area.z[1] - area.z[0]);
      }
    }
    geometry.attributes.position.needsUpdate = true;
  });

  return (
    <points ref={pointsRef} geometry={geometry}>
      <pointsMaterial
        map={texture}
        size={size}
        color={SNOW_COLOR}
        transparent
        opacity={opacity}
        depthWrite={false}
        sizeAttenuation
      />
    </points>
  );
}

// ─── Силуэты домов вдали (наполняют кадр, тают в тумане = глубина) ───────────
function Silhouettes() {
  const boxes = useMemo(
    () => [
      { pos: [-9, 4, -26], size: [7, 8, 6] },
      { pos: [-2, 6, -34], size: [8, 12, 6] },
      { pos: [8, 5, -30], size: [7, 10, 6] },
      { pos: [15, 7, -38], size: [9, 14, 7] },
      { pos: [-16, 5, -32], size: [8, 10, 6] },
    ],
    [],
  );
  return (
    <group>
      {boxes.map((b, i) => (
        <mesh key={i} position={b.pos as [number, number, number]}>
          <boxGeometry args={b.size as [number, number, number]} />
          {/* Unlit — читаются как чистый силуэт, но подвержены туману. */}
          <meshBasicMaterial color={0x080a10} fog />
        </mesh>
      ))}
    </group>
  );
}

// ─── Параллакс камеры за курсором + плавный lerp тепла ──────────────────────
function Rig({ warmRef }: { warmRef: React.MutableRefObject<number> }) {
  const { camera } = useThree();
  const dpiActive = useDpiStore((s) => s.active);
  const proxyRunning = useProxyStore((s) => s.running);
  const hot = dpiActive || proxyRunning;
  const hotRef = useRef(hot);
  hotRef.current = hot;

  const target = useRef({ x: 0, y: 0 });
  const lookAt = useMemo(() => new THREE.Vector3(1.2, 1.6, -12), []);

  useEffect(() => {
    const onMove = (e: PointerEvent) => {
      target.current.x = (e.clientX / window.innerWidth) * 2 - 1;
      target.current.y = (e.clientY / window.innerHeight) * 2 - 1;
    };
    window.addEventListener("pointermove", onMove);
    return () => window.removeEventListener("pointermove", onMove);
  }, []);

  useFrame((_, dt) => {
    // Плавно тянемся к hot-состоянию (экспоненциальное сглаживание).
    const k = 1 - Math.exp(-dt * 2.0);
    warmRef.current += ((hotRef.current ? 1 : 0) - warmRef.current) * k;

    // Мягкий параллакс: камера чуть смещается против курсора.
    const px = target.current.x * 0.6;
    const py = target.current.y * 0.35;
    camera.position.x += (px - camera.position.x) * 0.05;
    camera.position.y += (1.7 - py - camera.position.y) * 0.05;
    camera.lookAt(lookAt);
  });

  return null;
}

// ─── Постобработка: bloom лампы, DOF, виньетка, зерно плёнки ─────────────────
function Effects() {
  return (
    <EffectComposer multisampling={0}>
      <DepthOfField focusDistance={0.02} focalLength={0.04} bokehScale={2.4} />
      <Bloom
        intensity={0.95}
        luminanceThreshold={0.32}
        luminanceSmoothing={0.25}
        mipmapBlur
      />
      <Vignette offset={0.32} darkness={0.82} eskil={false} />
      <Noise blendFunction={BlendFunction.OVERLAY} opacity={0.06} />
    </EffectComposer>
  );
}

// ─── Корневой компонент ─────────────────────────────────────────────────────
export default function RussiaScene3D() {
  const [active, setActive] = useState(renderActive());
  const warmRef = useRef(0);
  const flakeTex = useMemo(() => makeFlakeTexture(), []);
  useEffect(() => () => flakeTex.dispose(), [flakeTex]);

  // Пауза кадров, когда окно скрыто (единый источник со 2D-темами).
  useEffect(() => onRenderActiveChange(setActive), []);

  return (
    <div className="pointer-events-none absolute inset-0 overflow-hidden">
      {/* Небо — тот же холодный стальной градиент, что и в 2D-версии. */}
      <div className="absolute inset-0 bg-[radial-gradient(ellipse_at_60%_28%,#12151f_0%,#0a0c13_46%,#050609_100%)]" />

      <Canvas
        className="absolute inset-0"
        frameloop={active ? "always" : "never"}
        dpr={[1, 1.5]}
        gl={{ antialias: true, alpha: true, powerPreference: "high-performance" }}
        camera={{ position: [0, 1.7, 7], fov: 48, near: 0.1, far: 200 }}
      >
        <fogExp2 attach="fog" args={[FOG_COLOR, 0.032]} />

        {/* Базовая подсветка, чтобы сцена не была совсем чёрной вне конуса. */}
        <hemisphereLight args={[0x2a3550, 0x06080d, 0.35]} />
        <ambientLight intensity={0.08} />

        <Ground />
        <Silhouettes />
        <Lamp warmRef={warmRef} />
        <LampLight warmRef={warmRef} />
        <LightCone warmRef={warmRef} />

        {/* Дальний мелкий снег + ближний крупный (его размывает DOF). */}
        <Snow
          count={1400}
          size={0.14}
          area={{ x: 70, y: 34, z: [-45, 8] }}
          speed={3.2}
          opacity={0.55}
          texture={flakeTex}
        />
        <Snow
          count={60}
          size={1.1}
          area={{ x: 24, y: 16, z: [3, 8] }}
          speed={4.5}
          opacity={0.22}
          texture={flakeTex}
        />

        <Rig warmRef={warmRef} />
        <Effects />
      </Canvas>

      {/* Снежная земля у нижней кромки — мягкая посадка кадра. */}
      <div className="absolute inset-x-0 bottom-0 h-[14%] bg-[linear-gradient(to_top,rgba(40,48,66,0.55),transparent)]" />
    </div>
  );
}
