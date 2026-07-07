import { useEffect, useMemo, useRef } from "react";
import * as THREE from "three";
import { Canvas, useFrame, useThree } from "@react-three/fiber";
import { EffectComposer, Bloom } from "@react-three/postprocessing";

import { useDpiStore } from "../../store/dpiStore";
import { useProxyStore } from "../../store/proxyStore";

// «Жидкий свет / шёлк» — полноэкранный процедурный шейдер. Никакой геометрии:
// переливающаяся текучая поверхность (domain-warp FBM), псевдо-нормали дают
// объёмный блеск/складки, бегущий свет — шёлковую иридесценцию. Абстрактно —
// поэтому ничто не может выглядеть «низкополигонально/инородно». Параллакс за
// курсором + лёгкий bloom. При активном обходе теплеет и разгорается.

const pointer = { x: 0, y: 0 };

const vertexShader = /* glsl */ `
  varying vec2 vUv;
  void main() {
    vUv = uv;
    gl_Position = vec4(position.xy, 0.0, 1.0); // fullscreen quad
  }
`;

const fragmentShader = /* glsl */ `
  precision highp float;
  varying vec2 vUv;
  uniform float uTime;
  uniform float uAspect;
  uniform float uWarm;
  uniform vec2 uPointer;

  float hash(vec2 p) {
    p = fract(p * vec2(123.34, 456.21));
    p += dot(p, p + 45.32);
    return fract(p.x * p.y);
  }
  float noise(vec2 p) {
    vec2 i = floor(p), f = fract(p);
    float a = hash(i), b = hash(i + vec2(1.0, 0.0));
    float c = hash(i + vec2(0.0, 1.0)), d = hash(i + vec2(1.0, 1.0));
    vec2 u = f * f * (3.0 - 2.0 * f);
    return mix(mix(a, b, u.x), mix(c, d, u.x), u.y);
  }
  float fbm(vec2 p) {
    float s = 0.0, a = 0.5;
    mat2 m = mat2(1.6, 1.2, -1.2, 1.6);
    for (int i = 0; i < 5; i++) { s += a * noise(p); p = m * p; a *= 0.5; }
    return s;
  }
  vec3 pal(float t) {
    return 0.5 + 0.5 * cos(6.28318 * (vec3(1.0) * t + vec3(0.0, 0.33, 0.66)));
  }

  void main() {
    vec2 uv = vUv;
    uv.x *= uAspect;
    vec2 p = uv * 2.4 + uPointer * 0.25;
    float t = uTime * 0.06;

    // Двойной domain-warp — текучие «складки шёлка».
    vec2 q = vec2(fbm(p + t), fbm(p + vec2(3.2, 1.7) - t));
    vec2 r = vec2(
      fbm(p + 3.0 * q + vec2(1.7, 9.2) + 0.15 * t),
      fbm(p + 3.0 * q + vec2(8.3, 2.8) - 0.12 * t)
    );
    float f = fbm(p + 3.0 * r);

    // Псевдо-нормаль из высотного поля — объёмный блеск.
    float e = 0.0028;
    float fx = fbm(p + 3.0 * r + vec2(e, 0.0)) - f;
    float fy = fbm(p + 3.0 * r + vec2(0.0, e)) - f;
    vec3 n = normalize(vec3(-fx, -fy, e * 8.0));

    vec3 L = normalize(vec3(cos(uTime * 0.15) * 0.6, sin(uTime * 0.12) * 0.6, 0.8));
    float diff = clamp(dot(n, L) * 0.5 + 0.5, 0.0, 1.0);
    float spec = pow(clamp(dot(reflect(-L, n), vec3(0.0, 0.0, 1.0)), 0.0, 1.0), 22.0);

    float hue = f * 1.2 + r.x * 0.6 + n.x * 0.3 + uWarm * 0.12;
    vec3 col = pal(hue);
    col = mix(vec3(0.035, 0.045, 0.11), col, 0.32 + 0.68 * smoothstep(0.2, 0.85, f));
    col *= 0.5 + 0.95 * diff;
    col += spec * (0.6 + uWarm * 0.5) * mix(vec3(0.8, 0.9, 1.0), vec3(1.0, 0.85, 0.6), uWarm);

    // Тёплый сдвиг при активации.
    col = mix(col, col * vec3(1.15, 0.98, 0.9) + vec3(0.05, 0.02, 0.0), uWarm * 0.4);

    // Зерно + виньетка.
    col += (hash(vUv * vec2(1920.0, 1080.0) + uTime) - 0.5) * 0.02;
    vec2 vc = vUv - 0.5;
    col *= 1.0 - dot(vc, vc) * 0.6;

    gl_FragColor = vec4(col, 1.0);
  }
`;

function Silk({ hotRef }: { hotRef: React.MutableRefObject<boolean> }) {
  const matRef = useRef<THREE.ShaderMaterial>(null);
  const { size } = useThree();
  const uniforms = useMemo(
    () => ({
      uTime: { value: 0 },
      uAspect: { value: 1 },
      uWarm: { value: 0 },
      uPointer: { value: new THREE.Vector2(0, 0) },
    }),
    [],
  );

  useFrame((state, dt) => {
    const u = matRef.current?.uniforms;
    if (!u) return;
    u.uTime.value = state.clock.elapsedTime;
    u.uAspect.value = size.width / size.height;
    const target = hotRef.current ? 1 : 0;
    u.uWarm.value += (target - u.uWarm.value) * Math.min(1, dt * 2);
    u.uPointer.value.x += (pointer.x - u.uPointer.value.x) * 0.04;
    u.uPointer.value.y += (pointer.y - u.uPointer.value.y) * 0.04;
  });

  return (
    <mesh frustumCulled={false}>
      <planeGeometry args={[2, 2]} />
      <shaderMaterial
        ref={matRef}
        vertexShader={vertexShader}
        fragmentShader={fragmentShader}
        uniforms={uniforms}
        depthTest={false}
        depthWrite={false}
      />
    </mesh>
  );
}

function Scene() {
  const dpiActive = useDpiStore((s) => s.active);
  const proxyRunning = useProxyStore((s) => s.running);
  const hotRef = useRef(false);
  hotRef.current = dpiActive || proxyRunning;

  return (
    <>
      <Silk hotRef={hotRef} />
      <EffectComposer multisampling={0} enableNormalPass={false}>
        <Bloom intensity={0.6} luminanceThreshold={0.55} luminanceSmoothing={0.5} mipmapBlur />
      </EffectComposer>
    </>
  );
}

export default function JapanScene3D() {
  useEffect(() => {
    const onMove = (e: PointerEvent) => {
      pointer.x = (e.clientX / window.innerWidth) * 2 - 1;
      pointer.y = -((e.clientY / window.innerHeight) * 2 - 1);
    };
    window.addEventListener("pointermove", onMove);
    return () => window.removeEventListener("pointermove", onMove);
  }, []);

  return (
    <div className="absolute inset-0" style={{ pointerEvents: "none" }}>
      <Canvas flat dpr={[1, 1.25]} gl={{ antialias: false, powerPreference: "high-performance" }}>
        <Scene />
      </Canvas>
    </div>
  );
}
