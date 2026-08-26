import * as THREE from "three";

import type { EarMood, EarPose, EarQuality } from "./types";

type AtmosphereUniforms = {
  uTime: { value: number };
  uMoodAge: { value: number };
  uQuality: { value: number };
  uResolution: { value: THREE.Vector2 };
  uPointer: { value: THREE.Vector2 };
  uMood: { value: THREE.Vector4 };
  uEarLeft: { value: THREE.Vector4 };
  uEarRight: { value: THREE.Vector4 };
};

export type EarAtmosphereFrame = {
  time: number;
  moodAge: number;
  mood: EarMood;
  quality: EarQuality;
  pointerX: number;
  pointerY: number;
  pointerActive: number;
  leftPose: EarPose;
  rightPose: EarPose;
  leftEnergy: number;
  rightEnergy: number;
};

// Камера сцены. Обе плоскости подгоняются ровно под усечённую пирамиду на своей
// глубине, поэтому field в шейдерах = экранные координаты: y от -0.5 до 0.5,
// x от -0.5*aspect до 0.5*aspect. Без этого композиция уезжает за кадр.
export type EarAtmosphereView = {
  fov: number;
  cameraY: number;
  cameraZ: number;
  targetY: number;
};

export type EarAtmosphere = {
  background: THREE.Mesh<THREE.PlaneGeometry, THREE.ShaderMaterial>;
  veil: THREE.Mesh<THREE.PlaneGeometry, THREE.ShaderMaterial>;
  update: (frame: EarAtmosphereFrame) => void;
  resize: (width: number, height: number) => void;
  dispose: () => void;
};

const DEFAULT_VIEW: EarAtmosphereView = { fov: 29, cameraY: 0.23, cameraZ: 5.2, targetY: 0.32 };
const BACKGROUND_Z = -0.72;
const BACKGROUND_OVERSCAN = 1.06;
const VEIL_Z = 1.05;
const VEIL_OVERSCAN = 1.025;

const VERTEX_SHADER = /* glsl */`
  varying vec2 vUv;

  void main() {
    vUv = uv;
    gl_Position = projectionMatrix * modelViewMatrix * vec4(position, 1.0);
  }
`;

const FIELD_GLSL = /* glsl */`
  float yaniHash(vec2 point) {
    vec3 p3 = fract(vec3(point.xyx) * 0.1031);
    p3 += dot(p3, p3.yzx + vec3(33.33));
    return fract((p3.x + p3.y) * p3.z);
  }

  float yaniNoise(vec2 point) {
    vec2 cell = floor(point);
    vec2 local = fract(point);
    local = local * local * (3.0 - 2.0 * local);
    return mix(
      mix(yaniHash(cell), yaniHash(cell + vec2(1.0, 0.0)), local.x),
      mix(yaniHash(cell + vec2(0.0, 1.0)), yaniHash(cell + vec2(1.0)), local.x),
      local.y
    );
  }

  float yaniFbm(vec2 point) {
    float value = 0.0;
    float amplitude = 0.52;
    mat2 rotation = mat2(0.82, -0.57, 0.57, 0.82);
    for (int octave = 0; octave < 5; octave += 1) {
      if (float(octave) >= uQuality) break;
      value += amplitude * yaniNoise(point);
      point = rotation * point * 2.03 + vec2(13.17);
      amplitude *= 0.5;
    }
    return value;
  }

  float yaniEllipse(vec2 point, vec2 center, vec2 radius) {
    vec2 value = (point - center) / radius;
    return exp(-dot(value, value) * 2.3);
  }

  // Широкая мягкая дуга: повёрнутая гауссова полоса, изогнутая по длине. Приём,
  // отличный от вертикальных штор Aurora — полосы идут наискось через кадр.
  float yaniRibbon(vec2 point, vec2 center, float angle, float reach, float thickness, float bend) {
    vec2 local = point - center;
    mat2 turn = mat2(cos(angle), -sin(angle), sin(angle), cos(angle));
    local = turn * local;
    local.y -= bend * local.x * local.x;
    float along = exp(-pow(local.x / reach, 2.0) * 1.35);
    float across = exp(-pow(local.y / thickness, 2.0) * 2.3);
    return along * across;
  }

  float yaniEvent(float time, float period, float width, float seed) {
    float cycle = floor(time / period);
    float strongest = 0.0;
    for (int offset = -1; offset <= 1; offset += 1) {
      float index = cycle + float(offset);
      float center = (index + 0.5) * period
        + (yaniHash(vec2(index * 19.0 + seed, seed)) - 0.5) * period * 0.36;
      float distanceToCenter = abs(time - center) / width;
      strongest = max(strongest, distanceToCenter < 1.0 ? 0.5 + 0.5 * cos(distanceToCenter * 3.14159265) : 0.0);
    }
    return strongest;
  }

  vec2 yaniFlow(vec2 point, float time, float gust) {
    float broad = yaniFbm(point * 1.25 + vec2(time * 0.035, -time * 0.024));
    float angle = broad * 6.2831853 + sin(point.y * 2.7 - time * 0.07) * 0.58;
    vec2 direction = vec2(cos(angle), sin(angle));
    direction += vec2(0.34 + gust * 0.42, 0.08 * sin(time * 0.11));
    return direction;
  }
`;

// Ночная комната позади головы: одна ментоловая лампа за макушкой, один тлеющий
// уголёк справа внизу, всё остальное — тьма. Силуэт волос читается только потому,
// что свет стоит ЗА ним, поэтому лампа не должна расползаться по всему кадру.
const BACKGROUND_FRAGMENT_SHADER = /* glsl */`
  precision highp float;

  uniform float uTime;
  uniform float uMoodAge;
  uniform float uQuality;
  uniform vec2 uResolution;
  uniform vec2 uPointer;
  uniform vec4 uMood;
  uniform vec4 uEarLeft;
  uniform vec4 uEarRight;
  varying vec2 vUv;

  ${FIELD_GLSL}

  void main() {
    vec2 uv = vUv;
    float aspect = uResolution.x / max(1.0, uResolution.y);
    vec2 field = vec2((uv.x - 0.5) * aspect, uv.y - 0.5);
    float busyMood = uMood.x;
    float scanningMood = uMood.y;
    float activeMood = uMood.z;
    float alarmMood = uMood.w;

    float breathing = 0.5 + 0.5 * sin(uTime * 0.36 + sin(uTime * 0.071) * 0.62);
    float livingPulse = 0.5 + 0.5 * sin(uTime * 0.82 + sin(uTime * 0.19) * 1.15);
    float gust = yaniEvent(uTime, 13.7, 1.65, 7.0);
    float warmPass = yaniEvent(uTime, 19.3, 3.4, 23.0);
    float ashEvent = yaniEvent(uTime, 11.9, 1.8, 41.0) * (0.22 + alarmMood * 0.78);
    float stateImpact = exp(-max(0.0, uMoodAge) * 1.75);
    float earMotion = clamp(uEarLeft.w + uEarRight.w, 0.0, 1.0);
    float flowTime = uTime * mix(1.0, 1.68, scanningMood) * mix(1.0, 0.64, alarmMood);

    vec2 world = field + uPointer * vec2(0.020, 0.012);
    vec2 flow = yaniFlow(world, flowTime, gust);
    vec2 hazeWorld = world + flow * (0.018 + breathing * 0.009);
    vec2 smokeWorld = world + flow * (0.042 + gust * 0.02);

    vec3 paperWarm = vec3(0.742, 0.712, 0.652);
    vec3 paperCool = vec3(0.775, 0.795, 0.768);
    vec3 mintLight = vec3(0.672, 0.812, 0.742);
    vec3 pinkLight = vec3(0.878, 0.732, 0.716);
    vec3 creamHot = vec3(0.912, 0.868, 0.782);
    vec3 smokeGrey = vec3(0.452, 0.492, 0.512);
    vec3 ashDark = vec3(0.318, 0.308, 0.284);
    vec3 sageInk = vec3(0.268, 0.336, 0.308);

    // База: белёсый верх → бежевый низ, плюс лёгкий тёплый уклон вправо.
    float vertical = smoothstep(-0.52, 0.5, field.y);
    vec3 color = mix(paperWarm, paperCool, vertical);
    color = mix(color, color * vec3(1.03, 0.995, 0.962), smoothstep(-0.6, 0.7, field.x) * 0.5);

    // Низкая частота: четыре широкие дуги наискось, дышат между кремом, мятой и розовым.
    float ribbonA = yaniRibbon(
      hazeWorld,
      vec2(-0.16 + sin(flowTime * 0.031) * 0.075, 0.145 + cos(flowTime * 0.024) * 0.035),
      0.34 + sin(flowTime * 0.019) * 0.05, 0.62, 0.135, 0.42
    );
    float ribbonB = yaniRibbon(
      hazeWorld,
      vec2(0.24 + cos(flowTime * 0.027 + 1.7) * 0.08, -0.055 + sin(flowTime * 0.021) * 0.04),
      -0.22 + cos(flowTime * 0.017) * 0.045, 0.54, 0.115, -0.5
    );
    float ribbonC = yaniRibbon(
      hazeWorld,
      vec2(-0.05 + sin(flowTime * 0.023 + 3.1) * 0.1, -0.28 + cos(flowTime * 0.029) * 0.03),
      0.13, 0.72, 0.165, 0.28
    );
    float ribbonD = yaniRibbon(
      hazeWorld,
      vec2(0.1 + cos(flowTime * 0.015 + 4.4) * 0.06, 0.34),
      -0.41, 0.46, 0.1, 0.6
    );
    float ribbonBreath = 0.78 + breathing * 0.22;
    color = mix(color, mintLight, ribbonA * (0.5 + scanningMood * 0.16) * ribbonBreath);
    color = mix(color, pinkLight, ribbonB * (0.42 + activeMood * 0.2) * (0.8 + livingPulse * 0.2));
    color = mix(color, creamHot, ribbonC * (0.44 + warmPass * 0.22) * ribbonBreath);
    color = mix(color, mix(mintLight, pinkLight, 0.45), ribbonD * (0.3 + earMotion * 0.16));

    // Крупная мягкая неровность бумаги — только по светлым полосам, чтобы база
    // осталась чистой.
    float lit = clamp(ribbonA + ribbonB + ribbonC + ribbonD, 0.0, 1.0);
    float paperGrain = yaniFbm(world * vec2(2.2, 2.75) + vec2(3.1, -1.4));
    color = mix(color, color * (1.0 - (paperGrain - 0.46) * 0.16), 0.55 + lit * 0.45);

    // Средняя частота: дым на светлом поле ЗАТЕМНЯЕТ и уводит в синеву,
    // а не светится. Мотив «煙とブルー».
    float smokeVeil = yaniFbm(smokeWorld * 3.1 + vec2(flowTime * 0.02, -flowTime * 0.028));
    float columnA = -0.2 + sin(smokeWorld.y * 4.6 + flowTime * 0.06 + smokeVeil * 2.4) * 0.12;
    float columnB = 0.26 + sin(smokeWorld.y * 3.4 - flowTime * 0.045 + smokeVeil * 1.7) * 0.095;
    float columnC = 0.02 + sin(smokeWorld.y * 5.8 + flowTime * 0.038 + smokeVeil * 2.9) * 0.15;
    float smokeA = exp(-abs(smokeWorld.x - columnA) * 6.2)
      * smoothstep(-0.46, 0.02, smokeWorld.y)
      * (1.0 - smoothstep(0.2, 0.52, smokeWorld.y));
    float smokeB = exp(-abs(smokeWorld.x - columnB) * 8.4)
      * smoothstep(-0.34, 0.1, smokeWorld.y)
      * (1.0 - smoothstep(0.26, 0.54, smokeWorld.y));
    float smokeC = exp(-abs(smokeWorld.x - columnC) * 11.5)
      * smoothstep(-0.5, -0.1, smokeWorld.y)
      * (1.0 - smoothstep(0.14, 0.46, smokeWorld.y));
    float smoke = (smokeA + smokeB * 0.72 + smokeC * 0.55) * (0.32 + smokeVeil * 0.68);
    color = mix(
      color,
      mix(smokeGrey, mix(smokeGrey, pinkLight, 0.4), activeMood * 0.5),
      clamp(smoke * (0.2 + gust * 0.08 + activeMood * 0.06), 0.0, 0.62)
    );

    // Высокая частота: пылинки и пепел на светлом фоне тоже тёмные.
    vec2 dustDrift = flow * 0.009 + vec2(flowTime * 0.0011, flowTime * (0.0026 + scanningMood * 0.005));
    vec2 dustUv = (uv + dustDrift) * vec2(118.0, 76.0);
    vec2 dustCell = floor(dustUv);
    float dustSeed = yaniHash(dustCell);
    vec2 dustPoint = vec2(yaniHash(dustCell + vec2(4.7)), yaniHash(dustCell + vec2(19.1)));
    float dust = smoothstep(0.1, 0.0, length(fract(dustUv) - dustPoint))
      * step(0.9 - scanningMood * 0.03, dustSeed)
      * max(0.0, 0.45 + 0.55 * sin(uTime * 0.34 + dustSeed * 18.0));
    color = mix(color, mix(sageInk, ashDark, dustSeed), dust * (0.3 + lit * 0.2));

    // Редкие хлопья пепла падают вниз.
    vec2 ashUv = (uv + vec2(-uTime * 0.003, uTime * 0.031)) * vec2(74.0, 43.0);
    vec2 ashCell = floor(ashUv);
    float ashSeed = yaniHash(ashCell + vec2(31.0));
    vec2 ashPoint = vec2(yaniHash(ashCell + vec2(8.0)), yaniHash(ashCell + vec2(17.0)));
    vec2 ashDelta = fract(ashUv) - ashPoint;
    float ashFlake = exp(-abs(ashDelta.x) * 74.0) * exp(-abs(ashDelta.y) * 18.0)
      * step(0.94, ashSeed) * ashEvent;
    color = mix(color, ashDark, ashFlake * 0.45);

    // Состояния. alarm выцветает и холодеет, смена состояния даёт мягкую вспышку.
    float grey = dot(color, vec3(0.299, 0.587, 0.114));
    color = mix(color, mix(vec3(grey), vec3(grey) * vec3(0.94, 1.0, 1.0), 0.6), alarmMood * 0.55);
    float transitionBloom = yaniEllipse(
      field,
      vec2(0.0, 0.06),
      vec2(0.46 + min(uMoodAge, 1.4) * 0.26, 0.32 + min(uMoodAge, 1.4) * 0.17)
    ) * stateImpact;
    color = mix(color, mix(creamHot, mintLight, alarmMood), transitionBloom * 0.2);
    color = mix(color, color * vec3(0.96, 0.975, 0.99), busyMood * 0.35);

    // Кадрирование высокого ключа: края не темнеют, а выцветают и уходят к белому —
    // так под панелями интерфейса остаётся спокойное поле.
    float edge = smoothstep(0.34, 1.16, length(field * vec2(0.9, 1.26)));
    float edgeGrey = dot(color, vec3(0.299, 0.587, 0.114));
    color = mix(color, mix(vec3(edgeGrey), paperCool, 0.62), edge * 0.72);
    gl_FragColor = vec4(color, 1.0);
  }
`;

// Вуаль лежит ПЕРЕД ушами, поэтому она больше не рисует ни голову, ни макушку —
// иначе в кадре два разных силуэта. Здесь только то, что действительно висит в
// воздухе: дымка у пола, две струйки дыма и выбившиеся волоски над макушкой.
const VEIL_FRAGMENT_SHADER = /* glsl */`
  precision highp float;

  uniform float uTime;
  uniform float uMoodAge;
  uniform float uQuality;
  uniform vec2 uResolution;
  uniform vec2 uPointer;
  uniform vec4 uMood;
  uniform vec4 uEarLeft;
  uniform vec4 uEarRight;
  varying vec2 vUv;

  ${FIELD_GLSL}

  void main() {
    vec2 uv = vUv;
    float aspect = uResolution.x / max(1.0, uResolution.y);
    vec2 field = vec2((uv.x - 0.5) * aspect, uv.y - 0.5);
    float scanningMood = uMood.y;
    float activeMood = uMood.z;
    float alarmMood = uMood.w;
    float breathing = 0.5 + 0.5 * sin(uTime * 0.36 + 0.8);
    float gust = yaniEvent(uTime, 13.7, 1.65, 7.0);
    float flowTime = uTime * mix(1.0, 1.68, scanningMood);
    vec2 flow = yaniFlow(field + uPointer * 0.012, flowTime, gust);

    // Дымка у пола, чуть плотнее при движении ушей.
    float hazeNoise = yaniFbm(
      field * vec2(2.6, 3.4) + flow * 0.5 + vec2(flowTime * 0.016, -flowTime * 0.012)
    );
    float haze = (1.0 - smoothstep(-0.42, 0.2, field.y)) * (0.34 + hazeNoise * 0.66);

    // Две струйки дыма поднимаются мимо ушей.
    vec2 smokeWorld = field + flow * (0.05 + gust * 0.028);
    float smokeNoise = yaniFbm(smokeWorld * 4.4 + vec2(flowTime * 0.022, -flowTime * 0.017));
    float pathA = -0.29 + sin(smokeWorld.y * 5.4 + flowTime * 0.07 + smokeNoise * 2.2) * 0.085;
    float pathB = 0.33 + sin(smokeWorld.y * 4.0 - flowTime * 0.05 + smokeNoise * 1.8) * 0.07;
    float wispA = exp(-abs(smokeWorld.x - pathA) * 24.0)
      * smoothstep(-0.4, -0.04, smokeWorld.y)
      * (1.0 - smoothstep(0.16, 0.5, smokeWorld.y));
    float wispB = exp(-abs(smokeWorld.x - pathB) * 30.0)
      * smoothstep(-0.3, 0.05, smokeWorld.y)
      * (1.0 - smoothstep(0.22, 0.52, smokeWorld.y));
    float wisp = (wispA + wispB * 0.72) * (0.4 + smokeNoise * 0.6);

    // Волоски, выбившиеся из макушки. crownArc повторяет купол головы, чтобы они
    // начинались от силуэта, а не висели в пустоте.
    float crownArc = 0.034 - field.x * field.x * 0.3;
    float flyaway = 0.0;
    for (int strand = 0; strand < 5; strand += 1) {
      float index = float(strand);
      float seed = yaniHash(vec2(index * 13.7 + 2.3, 4.1));
      float baseX = -0.35 + index * 0.175 + (seed - 0.5) * 0.055;
      float reach = 0.07 + seed * 0.08;
      float root = crownArc - 0.012;
      float rise = clamp((field.y - root) / reach, 0.0, 1.0);
      float centerX = baseX
        + rise * (seed - 0.45) * 0.26
        + sin(field.y * 24.0 + flowTime * (0.32 + seed * 0.45) + index) * 0.0085;
      flyaway += exp(-abs(field.x - centerX) * mix(160.0, 430.0, rise))
        * step(root, field.y)
        * (1.0 - rise * rise)
        * (0.55 + 0.45 * sin(flowTime * 0.55 + index * 2.1));
    }
    flyaway = clamp(flyaway, 0.0, 1.0) * (0.72 + gust * 0.28);

    // Шлейф за движущимся ухом.
    vec2 leftCenter = vec2(-0.35 + uEarLeft.x * 0.09, 0.07 + uEarLeft.y * 0.04);
    vec2 rightCenter = vec2(0.35 + uEarRight.x * 0.09, 0.07 + uEarRight.y * 0.04);
    float leftWake = exp(-length((field - leftCenter) * vec2(1.0, 1.45)) * 6.4) * uEarLeft.w;
    float rightWake = exp(-length((field - rightCenter) * vec2(1.0, 1.45)) * 6.4) * uEarRight.w;
    float stateWake = exp(-max(0.0, uMoodAge) * 1.4);

    // Высокий ключ переворачивает знаки: дымка светлеет к белому, дым и волоски
    // читаются ТЁМНЫМИ линиями по светлому полю.
    vec3 hazeColor = mix(vec3(0.795, 0.800, 0.782), vec3(0.700, 0.812, 0.752), hazeNoise * 0.55);
    vec3 wispColor = mix(vec3(0.438, 0.478, 0.502), vec3(0.560, 0.472, 0.408), activeMood * 0.45);
    vec3 hairColor = mix(vec3(0.196, 0.258, 0.236), vec3(0.288, 0.352, 0.320), 0.3 + breathing * 0.14);
    vec3 color = mix(hazeColor, wispColor, clamp(wisp * 0.8, 0.0, 1.0));
    color = mix(color, hairColor, clamp(flyaway * 1.4, 0.0, 1.0));
    color = mix(color, vec3(0.735, 0.845, 0.788), clamp((leftWake + rightWake) * 0.5, 0.0, 0.6));
    color = mix(color, vec3(0.512, 0.548, 0.556), alarmMood * stateWake * 0.45);

    float alpha = haze * (0.042 + hazeNoise * 0.03)
      + wisp * (0.075 + activeMood * 0.055 + gust * 0.028)
      + flyaway * 0.72
      + (leftWake + rightWake) * 0.028;
    alpha *= 1.0 - alarmMood * 0.2;
    gl_FragColor = vec4(color, clamp(alpha, 0.0, 0.74));
  }
`;

function createUniforms(): AtmosphereUniforms {
  return {
    uTime: { value: 0 },
    uMoodAge: { value: 0 },
    uQuality: { value: 5 },
    uResolution: { value: new THREE.Vector2(1000, 680) },
    uPointer: { value: new THREE.Vector2() },
    uMood: { value: new THREE.Vector4() },
    uEarLeft: { value: new THREE.Vector4() },
    uEarRight: { value: new THREE.Vector4() },
  };
}

/** Высота видимой области на глубине z — по ней плоскость подгоняется под кадр. */
function visibleHeightAt(view: EarAtmosphereView, z: number): number {
  return 2 * Math.tan(THREE.MathUtils.degToRad(view.fov / 2)) * (view.cameraZ - z);
}

/** Точка, в которую смотрит камера на глубине z: центр плоскости обязан лежать на луче. */
function viewRayY(view: EarAtmosphereView, z: number): number {
  return view.cameraY + ((view.cameraZ - z) / view.cameraZ) * (view.targetY - view.cameraY);
}

export function createEarAtmosphere(view: EarAtmosphereView = DEFAULT_VIEW): EarAtmosphere {
  const backgroundUniforms = createUniforms();
  const veilUniforms = createUniforms();
  const backgroundMaterial = new THREE.ShaderMaterial({
    vertexShader: VERTEX_SHADER,
    fragmentShader: BACKGROUND_FRAGMENT_SHADER,
    uniforms: backgroundUniforms,
    depthTest: false,
    depthWrite: false,
  });
  const veilMaterial = new THREE.ShaderMaterial({
    vertexShader: VERTEX_SHADER,
    fragmentShader: VEIL_FRAGMENT_SHADER,
    uniforms: veilUniforms,
    transparent: true,
    blending: THREE.NormalBlending,
    depthTest: false,
    depthWrite: false,
  });
  const background = new THREE.Mesh(new THREE.PlaneGeometry(1, 1), backgroundMaterial);
  background.position.set(0, viewRayY(view, BACKGROUND_Z), BACKGROUND_Z);
  background.renderOrder = -100;
  background.frustumCulled = false;
  const veil = new THREE.Mesh(new THREE.PlaneGeometry(1, 1), veilMaterial);
  veil.position.set(0, viewRayY(view, VEIL_Z), VEIL_Z);
  veil.renderOrder = 100;
  veil.frustumCulled = false;
  const pointerTarget = new THREE.Vector2();
  const uniformSets = [backgroundUniforms, veilUniforms] as const;

  return {
    background,
    veil,
    update: (frame) => {
      pointerTarget.set(frame.pointerX * frame.pointerActive, frame.pointerY * frame.pointerActive);
      uniformSets.forEach((uniforms) => {
        uniforms.uTime.value = frame.time;
        uniforms.uMoodAge.value = frame.moodAge;
        uniforms.uQuality.value = frame.quality === "high" ? 5 : frame.quality === "balanced" ? 4 : 3;
        uniforms.uMood.value.set(
          frame.mood === "busy" ? 1 : 0,
          frame.mood === "scanning" ? 1 : 0,
          frame.mood === "active" ? 1 : 0,
          frame.mood === "alarm" ? 1 : 0,
        );
        uniforms.uEarLeft.value.set(
          frame.leftPose.yaw,
          frame.leftPose.pitch,
          frame.leftPose.tip,
          frame.leftEnergy,
        );
        uniforms.uEarRight.value.set(
          frame.rightPose.yaw,
          frame.rightPose.pitch,
          frame.rightPose.tip,
          frame.rightEnergy,
        );
        uniforms.uPointer.value.lerp(pointerTarget, 0.075);
      });
    },
    resize: (width, height) => {
      const safeWidth = Math.max(1, width);
      const safeHeight = Math.max(1, height);
      const aspect = safeWidth / safeHeight;
      backgroundUniforms.uResolution.value.set(safeWidth, safeHeight);
      veilUniforms.uResolution.value.set(safeWidth, safeHeight);
      const backgroundHeight = visibleHeightAt(view, BACKGROUND_Z) * BACKGROUND_OVERSCAN;
      background.scale.set(backgroundHeight * aspect, backgroundHeight, 1);
      const veilHeight = visibleHeightAt(view, VEIL_Z) * VEIL_OVERSCAN;
      veil.scale.set(veilHeight * aspect, veilHeight, 1);
    },
    dispose: () => {
      background.geometry.dispose();
      veil.geometry.dispose();
      backgroundMaterial.dispose();
      veilMaterial.dispose();
    },
  };
}
