import * as THREE from "three";

import type { EarMaterials } from "./materials";
import type { EarPose, EarSide } from "./types";

export type EarSourceGeometry = {
  front: THREE.BufferGeometry;
  back: THREE.BufferGeometry;
  rim: THREE.BufferGeometry;
  fold: THREE.BufferGeometry;
};

export type EarRig = {
  root: THREE.Group;
  applyPose: (pose: EarPose) => void;
  setDetail: (shellCount: number, hairRatio: number) => void;
  dispose: () => void;
};

const BONE_ORIGINS = [-0.55, -0.07, 0.38, 0.73] as const;
const HAIR_SEGMENTS = 4;
const HAIR_INDEX_COUNT = HAIR_SEGMENTS * 6;

type TuftSpec = {
  u: number;
  v: number;
  direction: THREE.Vector3;
  length: number;
  width: number;
  warmth: number;
};

function withSkinning(source: THREE.BufferGeometry): THREE.BufferGeometry {
  const geometry = source.clone();
  const uv = geometry.getAttribute("uv");
  const vertexCount = geometry.getAttribute("position").count;
  const indices = new Uint16Array(vertexCount * 4);
  const weights = new Float32Array(vertexCount * 4);
  for (let vertex = 0; vertex < vertexCount; vertex += 1) {
    const v = uv ? uv.getY(vertex) : 0;
    const scaled = Math.min(3, Math.max(0, v * 3));
    const low = Math.min(2, Math.floor(scaled));
    const high = Math.min(3, low + 1);
    const blend = scaled - low;
    const offset = vertex * 4;
    indices[offset] = low;
    indices[offset + 1] = high;
    weights[offset] = 1 - blend;
    weights[offset + 1] = blend;
  }
  geometry.setAttribute("skinIndex", new THREE.Uint16BufferAttribute(indices, 4));
  geometry.setAttribute("skinWeight", new THREE.Float32BufferAttribute(weights, 4));
  if (geometry.index && geometry.getAttribute("normal") && geometry.getAttribute("uv")) geometry.computeTangents();
  geometry.computeBoundingSphere();
  return geometry;
}

function shellGeometry(source: THREE.BufferGeometry, distance: number): THREE.BufferGeometry {
  const geometry = source.clone();
  const position = geometry.getAttribute("position") as THREE.BufferAttribute;
  const normal = geometry.getAttribute("normal") as THREE.BufferAttribute;
  const uv = geometry.getAttribute("uv") as THREE.BufferAttribute;
  for (let vertex = 0; vertex < position.count; vertex += 1) {
    const lift = distance * (0.48 + uv.getY(vertex) * 0.52);
    position.setXYZ(
      vertex,
      position.getX(vertex) + normal.getX(vertex) * lift,
      position.getY(vertex) + normal.getY(vertex) * lift,
      position.getZ(vertex) + normal.getZ(vertex) * lift,
    );
  }
  position.needsUpdate = true;
  geometry.computeBoundingSphere();
  return geometry;
}

function tipGeometry(source: THREE.BufferGeometry): THREE.BufferGeometry {
  const geometry = source.clone();
  const index = source.index;
  const uv = source.getAttribute("uv") as THREE.BufferAttribute;
  const position = geometry.getAttribute("position") as THREE.BufferAttribute;
  const normal = geometry.getAttribute("normal") as THREE.BufferAttribute;
  const selected: number[] = [];
  if (index) {
    for (let offset = 0; offset < index.count; offset += 3) {
      const a = index.getX(offset);
      const b = index.getX(offset + 1);
      const c = index.getX(offset + 2);
      if (Math.max(uv.getY(a), uv.getY(b), uv.getY(c)) >= 0.72) selected.push(a, b, c);
    }
  }
  geometry.setIndex(selected);
  for (let vertex = 0; vertex < position.count; vertex += 1) {
    position.setXYZ(
      vertex,
      position.getX(vertex) + normal.getX(vertex) * 0.0035,
      position.getY(vertex) + normal.getY(vertex) * 0.0035,
      position.getZ(vertex) + normal.getZ(vertex) * 0.0035,
    );
  }
  position.needsUpdate = true;
  if (geometry.index && geometry.getAttribute("normal") && geometry.getAttribute("uv")) geometry.computeTangents();
  geometry.computeBoundingSphere();
  return geometry;
}

function hairHash(value: number): number {
  const hashed = Math.sin(value * 127.13 + 71.7) * 43758.5453;
  return hashed - Math.floor(hashed);
}

function cubicBezier(p0: THREE.Vector3, p1: THREE.Vector3, p2: THREE.Vector3, p3: THREE.Vector3, t: number) {
  const inverse = 1 - t;
  return new THREE.Vector3(
    inverse ** 3 * p0.x + 3 * inverse ** 2 * t * p1.x + 3 * inverse * t ** 2 * p2.x + t ** 3 * p3.x,
    inverse ** 3 * p0.y + 3 * inverse ** 2 * t * p1.y + 3 * inverse * t ** 2 * p2.y + t ** 3 * p3.y,
    inverse ** 3 * p0.z + 3 * inverse ** 2 * t * p1.z + 3 * inverse * t ** 2 * p2.z + t ** 3 * p3.z,
  );
}

function sampleSurface(source: THREE.BufferGeometry, targetU: number, targetV: number) {
  const position = source.getAttribute("position") as THREE.BufferAttribute;
  const normal = source.getAttribute("normal") as THREE.BufferAttribute;
  const uv = source.getAttribute("uv") as THREE.BufferAttribute;
  let nearest = 0;
  let nearestDistance = Number.POSITIVE_INFINITY;
  for (let vertex = 0; vertex < uv.count; vertex += 1) {
    const du = uv.getX(vertex) - targetU;
    const dv = uv.getY(vertex) - targetV;
    const distance = du * du + dv * dv;
    if (distance < nearestDistance) {
      nearest = vertex;
      nearestDistance = distance;
    }
  }
  return {
    point: new THREE.Vector3(position.getX(nearest), position.getY(nearest), position.getZ(nearest)),
    normal: new THREE.Vector3(normal.getX(nearest), normal.getY(nearest), normal.getZ(nearest)),
  };
}

function animeTuftSpecs(): TuftSpec[] {
  const specs: TuftSpec[] = [];
  const outerHeights = [0.075, 0.12, 0.18, 0.25, 0.33, 0.41];
  const innerHeights = [0.065, 0.13, 0.21, 0.3];
  const baseUs = [0.08, 0.2, 0.8, 0.92];
  const count = Math.max(outerHeights.length, innerHeights.length, baseUs.length);
  for (let index = 0; index < count; index += 1) {
    const wobble = hairHash(index * 7.3 + 4) - 0.5;
    if (index < outerHeights.length) {
      const v = outerHeights[index];
      specs.push({
        u: 0.008 + (index % 2) * 0.016,
        v,
        direction: new THREE.Vector3(-0.72, -0.58 + v * 0.38 + wobble * 0.12, 0.23),
        length: 0.16 + index * 0.014 + hairHash(index + 12) * 0.055,
        width: 0.022 + hairHash(index + 41) * 0.013,
        warmth: 0.08,
      });
      specs.push({
        u: 0.026,
        v: v + 0.018,
        direction: new THREE.Vector3(-0.58, -0.64 + v * 0.32 - wobble * 0.1, 0.26),
        length: 0.13 + index * 0.012 + hairHash(index + 27) * 0.045,
        width: 0.018 + hairHash(index + 52) * 0.01,
        warmth: 0.12,
      });
    }
    if (index < innerHeights.length) {
      const v = innerHeights[index];
      specs.push({
        u: 0.982,
        v,
        direction: new THREE.Vector3(0.58, -0.7 + v * 0.42 - wobble * 0.12, 0.24),
        length: 0.14 + index * 0.013 + hairHash(index + 64) * 0.05,
        width: 0.021 + hairHash(index + 81) * 0.012,
        warmth: 0.2,
      });
    }
    if (index < baseUs.length) {
      const u = baseUs[index];
      specs.push({
        u,
        v: 0.018 + (index % 3) * 0.011,
        direction: new THREE.Vector3((u - 0.5) * 0.42, -0.94, 0.18 + hairHash(index + 9) * 0.08),
        length: 0.105 + hairHash(index + 93) * 0.055,
        width: 0.025 + hairHash(index + 103) * 0.015,
        warmth: 0.16,
      });
    }
  }
  return specs;
}

function createHairRibbons(
  source: THREE.BufferGeometry,
  bones: THREE.Bone[],
  material: THREE.MeshPhysicalMaterial,
): { meshes: THREE.Mesh[]; fullIndexCounts: number[] } {
  const perBone = Array.from({ length: 4 }, () => ({
    positions: [] as number[], uvs: [] as number[], colors: [] as number[], indices: [] as number[],
  }));
  const viewDirection = new THREE.Vector3(0, 0, 1);

  animeTuftSpecs().forEach((spec, specIndex) => {
    const sample = sampleSurface(source, spec.u, spec.v);
    const boneIndex = Math.min(3, Math.floor(spec.v * 3.999));
    const target = perBone[boneIndex];
    const start = sample.point.clone();
    start.y -= BONE_ORIGINS[boneIndex];
    start.addScaledVector(sample.normal, 0.014);
    const direction = spec.direction.clone().normalize();
    const sideways = hairHash(specIndex * 11.3 + 8) - 0.5;
    const p0 = start;
    const p1 = start.clone().addScaledVector(direction, spec.length * 0.31).addScaledVector(sample.normal, spec.length * 0.08);
    const p2 = start.clone().addScaledVector(direction, spec.length * 0.72)
      .add(new THREE.Vector3(sideways * spec.length * 0.09, spec.length * 0.035, spec.length * 0.055));
    const p3 = start.clone().addScaledVector(direction, spec.length)
      .add(new THREE.Vector3(sideways * spec.length * 0.14, spec.length * 0.025, spec.length * 0.02));
    const strandStart = target.positions.length / 3;
    const brightness = 0.86 + hairHash(specIndex * 3.7 + 17) * 0.11;

    for (let section = 0; section <= HAIR_SEGMENTS; section += 1) {
      const t = section / HAIR_SEGMENTS;
      const previous = cubicBezier(p0, p1, p2, p3, Math.max(0, t - 0.018));
      const next = cubicBezier(p0, p1, p2, p3, Math.min(1, t + 0.018));
      const tangent = next.sub(previous).normalize();
      const widthVector = new THREE.Vector3().crossVectors(tangent, viewDirection);
      if (widthVector.lengthSq() < 1e-5) widthVector.set(1, 0, 0);
      widthVector.normalize();
      const point = cubicBezier(p0, p1, p2, p3, t);
      const width = THREE.MathUtils.lerp(spec.width, 0.0007, Math.pow(t, 0.72));
      const left = point.clone().addScaledVector(widthVector, -width);
      const right = point.clone().addScaledVector(widthVector, width);
      target.positions.push(left.x, left.y, left.z, right.x, right.y, right.z);
      target.uvs.push(0, t, 1, t);
      const red = THREE.MathUtils.lerp(0.68, 0.84, spec.warmth) * brightness;
      const green = THREE.MathUtils.lerp(0.89, 0.79, spec.warmth) * brightness;
      const blue = THREE.MathUtils.lerp(0.79, 0.68, spec.warmth) * brightness;
      target.colors.push(red, green, blue, red, green, blue);
    }
    for (let segment = 0; segment < HAIR_SEGMENTS; segment += 1) {
      const left0 = strandStart + segment * 2;
      const right0 = left0 + 1;
      const left1 = left0 + 2;
      const right1 = left0 + 3;
      target.indices.push(left0, right0, left1, right0, right1, left1);
    }
  });

  const meshes = perBone.map((data, boneIndex) => {
    const geometry = new THREE.BufferGeometry();
    geometry.setAttribute("position", new THREE.Float32BufferAttribute(data.positions, 3));
    geometry.setAttribute("uv", new THREE.Float32BufferAttribute(data.uvs, 2));
    geometry.setAttribute("color", new THREE.Float32BufferAttribute(data.colors, 3));
    geometry.setIndex(data.indices);
    geometry.computeVertexNormals();
    geometry.computeBoundingSphere();
    const mesh = new THREE.Mesh(geometry, material);
    mesh.name = `ear-root-tufts-${boneIndex}`;
    mesh.frustumCulled = false;
    mesh.renderOrder = 24 + boneIndex;
    bones[boneIndex].add(mesh);
    return mesh;
  });
  return { meshes, fullIndexCounts: meshes.map((mesh) => mesh.geometry.index?.count ?? 0) };
}

export function createEarRig(source: EarSourceGeometry, materials: EarMaterials, side: EarSide, withRings: boolean): EarRig {
  const root = new THREE.Group();
  const content = new THREE.Group();
  content.scale.x = side < 0 ? 1 : -1;
  root.add(content);

  const bones = Array.from({ length: 4 }, () => new THREE.Bone());
  bones[0].name = "ear-base";
  bones[1].name = "ear-lower-cup";
  bones[2].name = "ear-upper-cup";
  bones[3].name = "ear-tip";
  bones[0].position.y = BONE_ORIGINS[0];
  bones[1].position.y = BONE_ORIGINS[1] - BONE_ORIGINS[0];
  bones[2].position.y = BONE_ORIGINS[2] - BONE_ORIGINS[1];
  bones[3].position.y = BONE_ORIGINS[3] - BONE_ORIGINS[2];
  bones[0].add(bones[1]);
  bones[1].add(bones[2]);
  bones[2].add(bones[3]);
  content.add(bones[0]);
  const skeleton = new THREE.Skeleton(bones);

  const geometries = {
    front: withSkinning(source.front), back: withSkinning(source.back),
    rim: withSkinning(source.rim), fold: withSkinning(source.fold),
  };
  const makeSkinnedMesh = (geometry: THREE.BufferGeometry, material: THREE.Material, name: string) => {
    const mesh = new THREE.SkinnedMesh(geometry, material);
    mesh.name = name;
    mesh.castShadow = false;
    mesh.receiveShadow = false;
    mesh.frustumCulled = false;
    content.add(mesh);
    return mesh;
  };
  const frontMesh = makeSkinnedMesh(geometries.front, materials.base, side < 0 ? "left-ear-front" : "right-ear-front");
  const backMesh = makeSkinnedMesh(geometries.back, materials.back, "ear-back");
  const rimMesh = makeSkinnedMesh(geometries.rim, materials.rim, "ear-rounded-rim");
  const foldMesh = makeSkinnedMesh(geometries.fold, materials.fold, "ear-basal-fold");
  foldMesh.renderOrder = 9;

  const shellMeshes = materials.shells.map((material, index) => {
    const level = (index + 1) / materials.shells.length;
    const geometry = shellGeometry(geometries.front, level * 0.032);
    const mesh = makeSkinnedMesh(geometry, material, `ear-fur-shell-${index + 1}`);
    mesh.renderOrder = index + 1;
    return mesh;
  });
  const charcoalTipGeometry = side > 0 ? tipGeometry(geometries.front) : null;
  const charcoalTipMesh = charcoalTipGeometry ? makeSkinnedMesh(charcoalTipGeometry, materials.tip, "right-ear-charcoal-tip") : null;
  if (charcoalTipMesh) charcoalTipMesh.renderOrder = 18;
  const hairRibbons = createHairRibbons(geometries.front, bones, materials.hair);

  const ringPivots: THREE.Group[] = [];
  const ringGeometry = withRings ? new THREE.TorusGeometry(0.055, 0.01, 18, 44) : null;
  if (ringGeometry) {
    for (let index = 0; index < 2; index += 1) {
      const pivot = new THREE.Group();
      pivot.position.set(-0.4 + index * 0.04, 0.18 + index * 0.1, 0.09);
      pivot.rotation.y = -0.42;
      pivot.rotation.x = 0.1;
      const ring = new THREE.Mesh(ringGeometry, materials.ring);
      ring.castShadow = true;
      pivot.add(ring);
      bones[1].add(pivot);
      ringPivots.push(pivot);
    }
  }

  root.updateMatrixWorld(true);
  const skinnedMeshes = [frontMesh, backMesh, rimMesh, foldMesh, ...shellMeshes];
  if (charcoalTipMesh) skinnedMeshes.push(charcoalTipMesh);
  frontMesh.bind(skeleton);
  skinnedMeshes.slice(1).forEach((mesh) => mesh.bind(skeleton, frontMesh.bindMatrix));

  return {
    root,
    applyPose: (pose) => {
      content.position.y = pose.lift;
      content.rotation.y = pose.yaw;
      content.rotation.x = pose.pitch;
      content.rotation.z = -pose.splay;
      bones[1].rotation.x = -pose.cup * 0.24;
      bones[1].rotation.z = pose.tip * 0.045;
      bones[2].rotation.x = pose.cup * 0.55;
      bones[2].rotation.z = pose.tip * 0.24;
      bones[3].rotation.x = -pose.cup * 0.18 - 0.012;
      bones[3].rotation.z = pose.tip * 0.55;
      hairRibbons.meshes.forEach((mesh, index) => {
        mesh.rotation.z = -pose.tip * (0.012 + index * 0.003) - pose.yaw * 0.02;
        mesh.rotation.x = Math.abs(pose.cup) * 0.012;
      });
      ringPivots.forEach((pivot, index) => {
        pivot.rotation.z = pose.ringSwing * (1 + index * 0.16);
        pivot.rotation.x = Math.abs(pose.ringSwing) * 0.28 + 0.1;
      });
    },
    setDetail: (shellCount, hairRatio) => {
      const safeShells = Math.max(1, Math.min(shellMeshes.length, Math.round(shellCount)));
      const visible = new Set(Array.from({ length: safeShells }, (_, index) =>
        Math.round(index * (shellMeshes.length - 1) / Math.max(1, safeShells - 1)),
      ));
      shellMeshes.forEach((mesh, index) => { mesh.visible = visible.has(index); });
      const safeHairRatio = THREE.MathUtils.clamp(hairRatio, 0.08, 1);
      hairRibbons.meshes.forEach((mesh, index) => {
        const fullCount = hairRibbons.fullIndexCounts[index];
        const strandCount = Math.floor(fullCount * safeHairRatio / HAIR_INDEX_COUNT);
        mesh.geometry.setDrawRange(0, strandCount * HAIR_INDEX_COUNT);
      });
    },
    dispose: () => {
      Object.values(geometries).forEach((geometry) => geometry.dispose());
      shellMeshes.forEach((mesh) => mesh.geometry.dispose());
      hairRibbons.meshes.forEach((mesh) => mesh.geometry.dispose());
      charcoalTipGeometry?.dispose();
      ringGeometry?.dispose();
    },
  };
}
