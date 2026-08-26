import * as THREE from "three";

import type { EarCrown, EarCrownFrame } from "./crown";
import type { EarTextures } from "./textures";

type AnimatedClump = {
  object: THREE.Mesh;
  baseRotation: THREE.Quaternion;
  phase: number;
  side: number;
  layer: "back" | "front" | "socket" | "cap";
  detailThreshold: number;
};

function disposeMaterial(material: THREE.Material | THREE.Material[]) {
  (Array.isArray(material) ? material : [material]).forEach((entry) => entry.dispose());
}

function namePhase(name: string): number {
  let hash = 17;
  for (let index = 0; index < name.length; index += 1) hash = (hash * 31 + name.charCodeAt(index)) % 997;
  return hash / 997 * Math.PI * 2;
}

function detailThreshold(name: string): number {
  if (name === "CrownScalpCap" || /Part_|Sweep_|Root_|Back_.*Outer/.test(name)) return 0;
  if (/Back_.*Mid/.test(name)) return 0.28;
  if (/Side_|Fringe_/.test(name)) return 0.5;
  return 0.8;
}

function layerFor(object: THREE.Mesh): AnimatedClump["layer"] {
  if (object.name === "CrownScalpCap") return "cap";
  const authoredLayer = object.userData.yani_layer;
  if (authoredLayer === "back" || authoredLayer === "socket") return authoredLayer;
  return "front";
}

function createHairMaterial(textures: EarTextures, shade: boolean): THREE.MeshPhysicalMaterial {
  const material = new THREE.MeshPhysicalMaterial({
    name: shade ? "Yani crown shade" : "Yani crown mint",
    color: shade ? 0x91ad9e : 0xacc8b8,
    map: textures.albedo,
    normalMap: textures.normal,
    normalScale: new THREE.Vector2(0.16, 0.31),
    roughnessMap: textures.roughness,
    roughness: shade ? 0.59 : 0.48,
    metalness: 0,
    anisotropy: shade ? 0.52 : 0.72,
    anisotropyMap: textures.anisotropy,
    sheen: shade ? 0.28 : 0.44,
    sheenColor: new THREE.Color(shade ? 0xb8d0c2 : 0xe0eee4),
    sheenRoughness: 0.58,
    specularIntensity: shade ? 0.38 : 0.52,
    specularColor: new THREE.Color(0xe1eee6),
  });
  material.customProgramCacheKey = () => `yani-crown-glb-${shade ? "shade" : "mint"}-v1`;
  return material;
}

function createScalpMaterial(textures: EarTextures): THREE.MeshPhysicalMaterial {
  const material = createHairMaterial(textures, true);
  material.name = "Yani crown directional scalp";
  material.color.setHex(0x9db8a9);
  material.anisotropy = 0.78;
  material.sheen = 0.5;
  material.onBeforeCompile = (shader) => {
    shader.fragmentShader = shader.fragmentShader.replace(
      "#include <map_fragment>",
      `#include <map_fragment>
      #ifdef USE_MAP
        float yaniPart = abs(vMapUv.x - 0.5);
        float yaniFlow = (vMapUv.x - 0.5) * 54.0
          + (vMapUv.y - 0.18) * (vMapUv.x - 0.5) * 17.0;
        float yaniStrand = pow(0.5 + 0.5 * cos(yaniFlow), 9.0);
        float yaniPartShade = 1.0 - smoothstep(0.0, 0.018, yaniPart);
        diffuseColor.rgb *= 1.0 - yaniStrand * 0.052 - yaniPartShade * 0.07;
      #endif`,
    );
  };
  material.customProgramCacheKey = () => "yani-crown-directional-scalp-v1";
  return material;
}

export function createEarCrownAsset(source: THREE.Group, textures: EarTextures): EarCrown {
  const mintMaterial = createHairMaterial(textures, false);
  const shadeMaterial = createHairMaterial(textures, true);
  const capMaterial = createScalpMaterial(textures);
  const authoredMaterials = new Set<THREE.Material>();
  const clumps: AnimatedClump[] = [];

  // The crown is authored against the existing Three.js ear coordinates
  // (Y-up) while Blender stores the mesh Z-up. Its glTF exporter performs the
  // usual axis conversion, so rotate the imported authoring root back once.
  // Keeping this correction on the root preserves every clump's local pivot.
  source.rotation.x += Math.PI / 2;
  const basePosition = source.position.clone();

  source.name = "yani-crown-glb";
  source.traverse((object) => {
    if (!(object instanceof THREE.Mesh)) return;
    const oldMaterials = Array.isArray(object.material) ? object.material : [object.material];
    oldMaterials.forEach((material) => authoredMaterials.add(material));
    const layer = layerFor(object);
    object.material = layer === "cap" ? capMaterial : layer === "back" ? shadeMaterial : mintMaterial;
    object.castShadow = false;
    object.receiveShadow = false;
    object.frustumCulled = false;
    object.renderOrder = layer === "cap" ? 18 : layer === "back" ? 20 : layer === "front" ? 23 : 25;
    if (
      !object.geometry.getAttribute("tangent")
      && object.geometry.index
      && object.geometry.getAttribute("normal")
      && object.geometry.getAttribute("uv")
    ) object.geometry.computeTangents();
    clumps.push({
      object,
      baseRotation: object.quaternion.clone(),
      phase: namePhase(object.name),
      side: object.name.includes("_L") ? -1 : object.name.includes("_R") ? 1 : 0,
      layer,
      detailThreshold: detailThreshold(object.name),
    });
  });
  authoredMaterials.forEach((material) => disposeMaterial(material));

  const motionEuler = new THREE.Euler();
  const motionRotation = new THREE.Quaternion();

  return {
    group: source,
    update: (frame: EarCrownFrame) => {
      const moodLift = frame.mood === "alarm" ? -0.028 : frame.mood === "active" ? 0.012 : 0;
      source.position.y = basePosition.y + moodLift + frame.breath * 0.008;
      source.rotation.z = Math.sin(frame.time * 0.12) * 0.0035;
      const scanTwitch = frame.mood === "scanning" ? Math.sin(frame.time * 7.3) * 0.006 : 0;
      const alarmFold = frame.mood === "alarm" ? 0.014 : 0;
      const activeFlow = frame.mood === "active" ? 1.35 : 1;
      clumps.forEach((clump) => {
        if (clump.layer === "cap") return;
        const layerGain = clump.layer === "socket" ? 0.45 : clump.layer === "front" ? 1 : 0.62;
        const sway = Math.sin(frame.time * (0.31 + activeFlow * 0.04) + clump.phase) * 0.0055 * layerGain;
        const energyKick = frame.earEnergy * 0.012 * layerGain * Math.sin(frame.time * 2.1 + clump.phase);
        motionEuler.set(
          alarmFold * layerGain + energyKick * 0.35,
          clump.side * (sway * 0.55 + scanTwitch * layerGain),
          sway + energyKick,
        );
        motionRotation.setFromEuler(motionEuler);
        clump.object.quaternion.copy(clump.baseRotation).multiply(motionRotation);
      });
    },
    setDetail: (ratio) => {
      const safeRatio = THREE.MathUtils.clamp(ratio, 0, 1);
      clumps.forEach((clump) => { clump.object.visible = safeRatio + 1e-6 >= clump.detailThreshold; });
    },
    dispose: () => {
      source.traverse((object) => {
        if (object instanceof THREE.Mesh) object.geometry.dispose();
      });
      mintMaterial.dispose();
      shadeMaterial.dispose();
      capMaterial.dispose();
    },
  };
}
