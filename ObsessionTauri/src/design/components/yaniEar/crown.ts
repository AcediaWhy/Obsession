import * as THREE from "three";

import type { EarTextures } from "./textures";
import type { EarMood } from "./types";

// Макушка Яни-ко. Три уровня форм, строго в этом порядке (правило shape hierarchy):
// крупная масса черепа с падением за края кадра → клоки и клинья по силуэту →
// продольные пряди и линии раздела в перекрытиях.
//
// Главный приём взят из разбора Guilty Gear Xrd: нормали для освещения отвязаны
// от геометрии рендера. Силуэт даёт рваная геометрия клиньев, а затенение
// считается по гладкому эллипсоиду-прокси, поэтому тень остаётся большими
// цельными областями и не крошится на пятна. Плюс тёмный контур из раздутого
// меша с BackSide — именно он отделяет бледную мяту от светлого фона, ровно как
// линия в аниме.

export type EarCrownFrame = {
  time: number;
  mood: EarMood;
  breath: number;
  earEnergy: number;
};

export type EarCrown = {
  group: THREE.Group;
  update: (frame: EarCrownFrame) => void;
  setDetail: (spikeRatio: number) => void;
  dispose: () => void;
};

// Череп совпадает с прежней сферой макушки, иначе поедет стыковка ушей:
// корни ушей на y ≈ 0.11 остаются перед поверхностью, а основание уха ниже
// y ≈ -0.36 уходит внутрь и прячется.
const SKULL = { halfWidth: 1.5, halfHeight: 1.06, halfDepth: 0.76, centerY: -0.64, centerZ: -0.72 };
// Ниже экватора масса не сужается, а расходится и уходит вниз за кадр.
const FALL = { spread: 1.72, drop: 1.6, extent: 0.9 };
const EQUATOR_V = 0.46;
const RINGS = 228;
const ROWS = 74;
const OUTLINE_WIDTH = 0.019;

const HAIR_LIT = new THREE.Color(0xd2e0d5);
const HAIR_SHADE = new THREE.Color(0xa9c2b3);
const HAIR_DEEP = new THREE.Color(0x86a394);
const HAIR_GLOSS = new THREE.Color(0xeef4ef);
const HAIR_LINE = new THREE.Color(0x38473f);

function crownHash(value: number): number {
  const hashed = Math.sin(value * 91.37 + 13.71) * 43758.5453;
  return hashed - Math.floor(hashed);
}

/** Разница азимутов с учётом замыкания круга. */
function angleDelta(a: number, b: number): number {
  return Math.atan2(Math.sin(a - b), Math.cos(a - b));
}

/** Профиль массы: радиус в единицах полуосей и мировая высота для строки v. */
function crownProfile(v: number): { radius: number; height: number } {
  if (v <= EQUATOR_V) {
    const theta = (v / EQUATOR_V) * Math.PI * 0.5;
    return {
      radius: Math.sin(theta),
      height: SKULL.centerY + SKULL.halfHeight * Math.cos(theta),
    };
  }
  const fall = ((v - EQUATOR_V) / (1 - EQUATOR_V)) * FALL.extent;
  return {
    radius: 1 + fall * FALL.spread,
    height: SKULL.centerY - SKULL.halfHeight * fall * FALL.drop,
  };
}

type CrownClump = {
  azimuth: number;
  width: number;
  height: number;
  center: number;
  span: number;
  lean: number;
  tone: number;
};

/**
 * Крупные клоки. Лепестки узкие по азимуту и высокие: именно они рвут силуэт на
 * светлом фоне, поэтому амплитуда идёт до 0.4 мировых единиц, а не до сотых.
 * Часть клоков крупная, часть мелкая — как на референсах.
 */
function crownClumps(): CrownClump[] {
  const count = 17;
  return Array.from({ length: count }, (_, index) => {
    const seedA = crownHash(index * 3.17 + 1.4);
    const seedB = crownHash(index * 7.91 + 5.2);
    const seedC = crownHash(index * 11.3 + 9.8);
    const big = seedB > 0.42;
    return {
      azimuth: ((index + 0.5 + (seedA - 0.5) * 0.62) / count) * Math.PI * 2,
      width: 0.115 + seedB * 0.085,
      height: big ? 0.24 + seedC * 0.2 : 0.1 + seedC * 0.1,
      center: 0.1 + seedA * 0.26,
      span: 0.17 + seedB * 0.14,
      lean: (seedC - 0.5) * 0.85,
      tone: seedA * 0.55 + seedB * 0.45,
    };
  });
}

// Пробор чуть правее фронтальной оси (фронт = PI/2), как на референсах.
const PART_AZIMUTH = Math.PI * 0.5 + 0.34;

type ClumpSample = { push: number; tone: number; seam: number };

/**
 * Поле клоков. push — смещение по нормали (только силуэт), tone — вариация тона
 * от клока к клоку, seam — насколько точка лежит МЕЖДУ клоками: там по правилу
 * иерархии форм живут самые тёмные значения и линии раздела.
 */
function clumpField(clumps: CrownClump[], phi: number, v: number): ClumpSample {
  let push = 0;
  let tone = 0;
  let strongest = 0;
  for (const clump of clumps) {
    const drift = clump.azimuth + (v - clump.center) * clump.lean;
    const angular = Math.exp(-Math.pow(angleDelta(phi, drift) / clump.width, 2));
    const vertical = Math.exp(-Math.pow((v - clump.center) / clump.span, 2));
    // Заострение: без него лепесток остаётся округлым бугром, а нужен клин.
    const lobe = Math.pow(angular, 2.3) * vertical;
    push += lobe * clump.height;
    tone += angular * vertical * clump.tone;
    strongest = Math.max(strongest, angular * vertical);
  }
  const topWindow = Math.exp(-Math.pow(v / 0.5, 2));
  // Между клоками масса продавлена: так они читаются отдельными прядями.
  push -= (1 - THREE.MathUtils.clamp(strongest, 0, 1)) * 0.075 * topWindow;
  // Крупная волна: масса не идеальный эллипс.
  push += (Math.sin(phi * 3.1 + 0.7) * 0.04 + Math.sin(phi * 5.3 - 1.4) * 0.024) * (0.4 + v);
  // Мелкая пила по кромке — «сломать слишком чистый контур».
  push += (Math.sin(phi * 37 + v * 5) * 0.5 + Math.sin(phi * 61 - v * 8) * 0.3) * 0.019 * topWindow;
  // Канавка пробора.
  push -= Math.exp(-Math.pow(angleDelta(phi, PART_AZIMUTH) / 0.1, 2))
    * Math.exp(-Math.pow(v / 0.3, 2)) * 0.075;
  return {
    push,
    tone: THREE.MathUtils.clamp(tone, 0, 1),
    seam: THREE.MathUtils.clamp(1 - strongest * 1.55, 0, 1),
  };
}

type CrownBuffers = {
  positions: number[];
  normals: number[];
  uvs: number[];
  structure: number[];
  domeIndices: number[];
  spikeIndices: number[];
};

/** Нормаль эллипсоида-прокси: именно она уходит в освещение. */
function skullNormal(x: number, y: number, z: number, target: THREE.Vector3): THREE.Vector3 {
  return target
    .set(
      x / (SKULL.halfWidth * SKULL.halfWidth),
      (y - SKULL.centerY) / (SKULL.halfHeight * SKULL.halfHeight),
      (z - SKULL.centerZ) / (SKULL.halfDepth * SKULL.halfDepth),
    )
    .normalize();
}

function structureAt(normal: THREE.Vector3, v: number, tone: number, seam: number): [number, number, number, number] {
  const topPlane = Math.pow(Math.max(0, normal.y), 1.4) * (0.34 + 0.66 * Math.max(0, normal.z));
  const sideFall = Math.pow(Math.abs(normal.x), 2.4) * 0.2;
  // Рампа тона считается по ВИДИМОЙ полосе: низ кадра приходится на v ≈ 0.66,
  // дальше масса уже за кадром, и уводить её в глубокую тень незачем.
  const depth = THREE.MathUtils.clamp(
    THREE.MathUtils.smoothstep(v, 0, 0.62) * 0.86 + sideFall,
    0,
    1,
  );
  return [topPlane, depth, tone, seam];
}

function buildDome(buffers: CrownBuffers, clumps: CrownClump[]): number {
  const columns = RINGS + 1;
  const normal = new THREE.Vector3();
  for (let row = 0; row <= ROWS; row += 1) {
    const v = row / ROWS;
    const profile = crownProfile(v);
    for (let column = 0; column < columns; column += 1) {
      const phi = (column / RINGS) * Math.PI * 2;
      const baseX = SKULL.halfWidth * profile.radius * Math.cos(phi);
      const baseZ = SKULL.centerZ + SKULL.halfDepth * profile.radius * Math.sin(phi);
      skullNormal(baseX, profile.height, baseZ, normal);
      const sample = clumpField(clumps, phi, v);
      buffers.positions.push(
        baseX + normal.x * sample.push,
        profile.height + normal.y * sample.push,
        baseZ + normal.z * sample.push,
      );
      buffers.normals.push(normal.x, normal.y, normal.z);
      buffers.uvs.push(column / RINGS, v);
      buffers.structure.push(...structureAt(normal, v, sample.tone, sample.seam));
    }
  }
  for (let row = 0; row < ROWS; row += 1) {
    for (let column = 0; column < RINGS; column += 1) {
      const a = row * columns + column;
      const b = a + 1;
      const c = a + columns;
      const d = c + 1;
      buffers.domeIndices.push(a, c, b, b, c, d);
    }
  }
  return (ROWS + 1) * columns;
}

type SpikeSpec = {
  azimuth: number;
  v: number;
  length: number;
  width: number;
  thickness: number;
  droop: number;
  lean: number;
  tone: number;
  rank: number;
};

function crownSpikes(): SpikeSpec[] {
  // Отключено: торчащие пирамиды читались осколками стекла, а не прядями.
  // Силуэт теперь целиком на совести клоков самого купола (clumpField).
  return [];
}

/**
 * Клин — пирамида на прямоугольном основании, утопленном в купол. Замкнутый
 * объём нужен, чтобы контурная оболочка с BackSide работала и на клиньях.
 * Все пять вершин получают ОДНУ прокси-нормаль купола: клин затеняется как часть
 * массы, без собственной огранки.
 */
function buildSpikes(
  buffers: CrownBuffers,
  clumps: CrownClump[],
  specs: SpikeSpec[],
  vertexOffset: number,
): void {
  const normal = new THREE.Vector3();
  const tangent = new THREE.Vector3();
  const slope = new THREE.Vector3();
  const base = new THREE.Vector3();
  const direction = new THREE.Vector3();
  const apex = new THREE.Vector3();
  const centroid = new THREE.Vector3();
  let offset = vertexOffset;

  for (const spec of specs) {
    const profile = crownProfile(spec.v);
    const cos = Math.cos(spec.azimuth);
    const sin = Math.sin(spec.azimuth);
    const baseX = SKULL.halfWidth * profile.radius * cos;
    const baseZ = SKULL.centerZ + SKULL.halfDepth * profile.radius * sin;
    skullNormal(baseX, profile.height, baseZ, normal);
    const sample = clumpField(clumps, spec.azimuth, spec.v);
    const sink = sample.push - 0.035;
    base.set(baseX + normal.x * sink, profile.height + normal.y * sink, baseZ + normal.z * sink);
    tangent.set(-sin * SKULL.halfWidth, 0, cos * SKULL.halfDepth).normalize();
    slope.crossVectors(tangent, normal).normalize();
    if (slope.y > 0) slope.negate();
    direction
      .copy(normal)
      .multiplyScalar(0.18)
      .addScaledVector(slope, spec.droop)
      .addScaledVector(tangent, spec.lean * 0.5)
      .normalize();
    apex.copy(base).addScaledVector(direction, spec.length);
    centroid.addVectors(base, apex).multiplyScalar(0.5);

    const halfWidth = spec.width * 0.5;
    const halfThickness = spec.thickness * 0.5;
    const corners = [
      new THREE.Vector3().copy(base).addScaledVector(tangent, -halfWidth).addScaledVector(normal, -halfThickness),
      new THREE.Vector3().copy(base).addScaledVector(tangent, halfWidth).addScaledVector(normal, -halfThickness),
      new THREE.Vector3().copy(base).addScaledVector(tangent, halfWidth).addScaledVector(normal, halfThickness),
      new THREE.Vector3().copy(base).addScaledVector(tangent, -halfWidth).addScaledVector(normal, halfThickness),
    ];
    const baseStructure = structureAt(normal, spec.v, spec.tone, 0);
    const apexStructure: [number, number, number, number] = [
      Math.min(1, baseStructure[0] + 0.22),
      baseStructure[1] * 0.5,
      spec.tone,
      0,
    ];
    const u = spec.azimuth / (Math.PI * 2);
    corners.forEach((point, index) => {
      buffers.positions.push(point.x, point.y, point.z);
      buffers.normals.push(normal.x, normal.y, normal.z);
      buffers.uvs.push(u + (index === 1 || index === 2 ? 0.01 : -0.01), spec.v);
      buffers.structure.push(...baseStructure);
    });
    buffers.positions.push(apex.x, apex.y, apex.z);
    buffers.normals.push(normal.x, normal.y, normal.z);
    buffers.uvs.push(u, spec.v + spec.length * 0.55);
    buffers.structure.push(...apexStructure);

    const apexIndex = offset + 4;
    const addFace = (a: number, b: number, c: number, pa: THREE.Vector3, pb: THREE.Vector3, pc: THREE.Vector3) => {
      const edge0 = new THREE.Vector3().subVectors(pb, pa);
      const edge1 = new THREE.Vector3().subVectors(pc, pa);
      const faceNormal = edge0.cross(edge1);
      const outward = new THREE.Vector3().add(pa).add(pb).add(pc).multiplyScalar(1 / 3).sub(centroid);
      if (faceNormal.dot(outward) >= 0) buffers.spikeIndices.push(a, b, c);
      else buffers.spikeIndices.push(a, c, b);
    };
    for (let side = 0; side < 4; side += 1) {
      const next = (side + 1) % 4;
      addFace(offset + side, offset + next, apexIndex, corners[side], corners[next], apex);
    }
    addFace(offset, offset + 1, offset + 2, corners[0], corners[1], corners[2]);
    addFace(offset, offset + 2, offset + 3, corners[0], corners[2], corners[3]);
    offset += 5;
  }
}

function glsl(color: THREE.Color): string {
  return `vec3(${color.r.toFixed(5)}, ${color.g.toFixed(5)}, ${color.b.toFixed(5)})`;
}

type GlossUniform = { value: number };

function createCrownMaterial(textures: EarTextures, gloss: GlossUniform): THREE.MeshPhysicalMaterial {
  // Нормал-мапа сознательно НЕ подключается: в Xrd мелочь идёт геометрией, а
  // текстурные нормали как раз и крошат затенение на пятна.
  const material = new THREE.MeshPhysicalMaterial({
    color: 0xffffff,
    map: textures.albedo,
    roughnessMap: textures.roughness,
    roughness: 0.74,
    metalness: 0,
    sheen: 0.34,
    sheenColor: HAIR_GLOSS.clone(),
    sheenRoughness: 0.62,
    specularIntensity: 0.4,
    specularColor: new THREE.Color(0xdfeae3),
  });
  material.onBeforeCompile = (shader) => {
    shader.uniforms.yaniCrownGloss = gloss;
    shader.vertexShader = shader.vertexShader.replace(
      "#include <common>",
      `#include <common>
      attribute vec4 aCrown;
      varying vec4 vCrown;`,
    );
    shader.vertexShader = shader.vertexShader.replace(
      "#include <begin_vertex>",
      `#include <begin_vertex>
      vCrown = aCrown;`,
    );
    shader.fragmentShader = shader.fragmentShader.replace(
      "#include <common>",
      `#include <common>
      uniform float yaniCrownGloss;
      varying vec4 vCrown;`,
    );
    shader.fragmentShader = shader.fragmentShader.replace(
      "#include <map_fragment>",
      `#include <map_fragment>
      float yaniTop = vCrown.x;
      float yaniDepth = vCrown.y;
      float yaniTone = vCrown.z;
      float yaniSeam = vCrown.w;
      float yaniGrain = texture2D(map, vec2(vMapUv.x * 3.4, vMapUv.y * 1.15)).g;
      vec3 yaniBody = mix(${glsl(HAIR_LIT)}, ${glsl(HAIR_SHADE)}, smoothstep(0.04, 0.7, yaniDepth));
      yaniBody = mix(yaniBody, ${glsl(HAIR_DEEP)}, smoothstep(0.6, 1.0, yaniDepth) * 0.88);
      yaniBody *= 0.97 + yaniTone * 0.06;
      float yaniStrand = pow(
        0.5 + 0.5 * sin(vMapUv.x * 208.0 + yaniGrain * 5.5 + vMapUv.y * 2.4),
        3.0
      );
      yaniBody *= 0.962 + yaniStrand * 0.076;
      yaniBody = mix(
        yaniBody,
        ${glsl(HAIR_LINE)},
        smoothstep(0.4, 0.94, yaniSeam) * 0.52 * (0.68 + yaniGrain * 0.32)
      );
      diffuseColor.rgb = yaniBody;`,
    );
    shader.fragmentShader = shader.fragmentShader.replace(
      "#include <lights_fragment_end>",
      `#include <lights_fragment_end>
      float yaniBandNoise = texture2D(map, vec2(vMapUv.x * 2.1, vMapUv.y * 0.7)).r;
      float yaniBand = exp(-pow((yaniDepth - (0.25 + yaniBandNoise * 0.06)) / 0.082, 2.0));
      float yaniGlossMask = yaniBand * pow(yaniTop, 1.1) * (0.55 + yaniBandNoise * 0.45);
      reflectedLight.directSpecular += ${glsl(HAIR_GLOSS)} * yaniGlossMask * yaniCrownGloss;`,
    );
  };
  material.customProgramCacheKey = () => "yani-crown-hair-v1";
  return material;
}

function createOutlineMaterial(): THREE.MeshBasicMaterial {
  const material = new THREE.MeshBasicMaterial({
    color: HAIR_LINE.clone(),
    side: THREE.BackSide,
  });
  material.onBeforeCompile = (shader) => {
    shader.vertexShader = shader.vertexShader.replace(
      "#include <common>",
      `#include <common>
      attribute vec3 aOutline;`,
    );
    shader.vertexShader = shader.vertexShader.replace(
      "#include <begin_vertex>",
      `#include <begin_vertex>
      transformed += aOutline * ${OUTLINE_WIDTH.toFixed(4)};`,
    );
  };
  material.customProgramCacheKey = () => "yani-crown-outline-v1";
  return material;
}

const SPIKE_INDICES_PER_WEDGE = 18;

export function createEarCrown(textures: EarTextures): EarCrown {
  const clumps = crownClumps();
  const specs = crownSpikes();
  const buffers: CrownBuffers = {
    positions: [], normals: [], uvs: [], structure: [], domeIndices: [], spikeIndices: [],
  };
  const domeVertexCount = buildDome(buffers, clumps);
  buildSpikes(buffers, clumps, specs, domeVertexCount);
  const indices = [...buffers.domeIndices, ...buffers.spikeIndices];

  const geometry = new THREE.BufferGeometry();
  geometry.setAttribute("position", new THREE.Float32BufferAttribute(buffers.positions, 3));
  geometry.setAttribute("normal", new THREE.Float32BufferAttribute(buffers.normals, 3));
  geometry.setAttribute("uv", new THREE.Float32BufferAttribute(buffers.uvs, 2));
  geometry.setAttribute("aCrown", new THREE.Float32BufferAttribute(buffers.structure, 4));
  geometry.setIndex(indices);

  // Контур раздувается по НАСТОЯЩЕЙ нормали поверхности: по прокси-нормали клинья
  // вытянулись бы в длину вместо того, чтобы потолстеть.
  const helper = new THREE.BufferGeometry();
  helper.setAttribute("position", new THREE.Float32BufferAttribute(buffers.positions, 3));
  helper.setIndex(indices);
  helper.computeVertexNormals();
  geometry.setAttribute(
    "aOutline",
    new THREE.Float32BufferAttribute(
      Array.from(helper.getAttribute("normal").array as Float32Array),
      3,
    ),
  );
  helper.dispose();
  geometry.computeBoundingSphere();

  const gloss: GlossUniform = { value: 0.5 };
  const crownMaterial = createCrownMaterial(textures, gloss);
  const outlineMaterial = createOutlineMaterial();

  const crown = new THREE.Mesh(geometry, crownMaterial);
  crown.name = "yani-crown";
  crown.castShadow = false;
  // Самозатенение волос выключено намеренно: честная тень от ушей роняет на массу
  // пятно в форме уха и читается как грязь.
  crown.receiveShadow = false;
  crown.renderOrder = 30;
  crown.frustumCulled = false;
  const outline = new THREE.Mesh(geometry, outlineMaterial);
  outline.name = "yani-crown-outline";
  outline.renderOrder = 29;
  outline.frustumCulled = false;
  outline.visible = false;

  const group = new THREE.Group();
  group.name = "yani-crown-group";
  group.add(outline, crown);

  const domeIndexCount = buffers.domeIndices.length;
  const spikeIndexCount = buffers.spikeIndices.length;

  return {
    group,
    update: (frame) => {
      const moodLift = frame.mood === "alarm" ? -0.032 : frame.mood === "active" ? 0.014 : 0;
      group.position.y = moodLift + frame.breath * 0.009;
      group.rotation.z = Math.sin(frame.time * 0.12) * 0.004;
      gloss.value = (frame.mood === "alarm" ? 0.24 : frame.mood === "scanning" ? 0.72 : 0.5)
        + frame.breath * 0.12
        + frame.earEnergy * 0.16;
    },
    setDetail: (spikeRatio) => {
      const wedges = Math.round(
        (spikeIndexCount / SPIKE_INDICES_PER_WEDGE) * THREE.MathUtils.clamp(spikeRatio, 0, 1),
      );
      geometry.setDrawRange(0, domeIndexCount + wedges * SPIKE_INDICES_PER_WEDGE);
    },
    dispose: () => {
      geometry.dispose();
      crownMaterial.dispose();
      outlineMaterial.dispose();
    },
  };
}
