import * as THREE from "three";

import type { EarTextures } from "./textures";

export type EarMaterials = {
  base: THREE.MeshPhysicalMaterial;
  back: THREE.MeshBasicMaterial;
  rim: THREE.MeshBasicMaterial;
  fold: THREE.MeshPhysicalMaterial;
  tip: THREE.MeshPhysicalMaterial;
  shells: THREE.MeshPhysicalMaterial[];
  hair: THREE.MeshPhysicalMaterial;
  ring: THREE.MeshPhysicalMaterial;
  setSssStrength: (strength: number) => void;
  dispose: () => void;
};

type SssUniform = { value: number };

function frontColorModel(
  material: THREE.MeshPhysicalMaterial,
  uniforms: Set<SssUniform>,
  shell: boolean,
  shellLevel: number,
): void {
  material.onBeforeCompile = (shader) => {
    const sssStrength: SssUniform = { value: 0.9 };
    uniforms.add(sssStrength);
    shader.uniforms.yaniSssStrength = sssStrength;
    shader.fragmentShader = shader.fragmentShader.replace(
      "#include <common>",
      `#include <common>
      uniform float yaniSssStrength;`,
    );
    shader.fragmentShader = shader.fragmentShader.replace(
      "#include <map_fragment>",
      `#include <map_fragment>
      float yaniCenter = mix(0.515, 0.555, smoothstep(0.12, 0.9, vMapUv.y));
      float yaniSigned = vMapUv.x - yaniCenter;
      float yaniHeightT = smoothstep(0.05, 0.98, vMapUv.y);
      float yaniOuterReach = mix(0.47, 0.075, pow(yaniHeightT, 1.55));
      float yaniInnerReach = mix(0.4, 0.055, pow(yaniHeightT, 1.55));
      float yaniInnerCoordinate = yaniSigned < 0.0
        ? abs(yaniSigned) / max(0.035, yaniOuterReach)
        : yaniSigned / max(0.035, yaniInnerReach);
      float yaniInner = (1.0 - smoothstep(0.78, 1.035, yaniInnerCoordinate))
        * smoothstep(0.055, 0.2, vMapUv.y)
        * (1.0 - smoothstep(0.86, 0.982, vMapUv.y));
      float yaniRim = smoothstep(0.74, 1.03, yaniInnerCoordinate);
      float yaniCavity = yaniInner
        * (1.0 - smoothstep(0.16, 0.88, yaniInnerCoordinate))
        * smoothstep(0.1, 0.34, vMapUv.y)
        * (1.0 - smoothstep(0.67, 0.94, vMapUv.y));
      float yaniBasalPocket = (1.0 - smoothstep(0.08, 0.31, vMapUv.x))
        * smoothstep(0.07, 0.18, vMapUv.y)
        * (1.0 - smoothstep(0.28, 0.44, vMapUv.y));
      float yaniThinCartilage = yaniInner
        * mix(0.38, 1.0, smoothstep(0.28, 0.97, vMapUv.y))
        * mix(1.0, 0.7, smoothstep(0.55, 1.0, yaniInnerCoordinate));
      float yaniBaseFade = smoothstep(0.008, 0.115, vMapUv.y);
      vec3 yaniDermis = vec3(0.34, 0.185, 0.155);
      vec3 yaniUnderfur = vec3(0.53, 0.34, 0.27);
      vec3 yaniInnerTone = mix(yaniUnderfur, yaniDermis, 0.58 + yaniCavity * 0.22);
      diffuseColor.rgb = mix(diffuseColor.rgb, yaniInnerTone, yaniInner * 0.86);
      diffuseColor.rgb *= 1.0 - yaniCavity * 0.24 - yaniBasalPocket * 0.14;
      diffuseColor.rgb *= mix(0.62, 1.0, smoothstep(0.02, 0.17, vMapUv.y));
      diffuseColor.rgb *= 0.97 + 0.055 * (texture2D(map, vMapUv * vec2(1.7, 2.35)).g - 0.5);
      diffuseColor.a *= yaniBaseFade;
      ${shell ? `
        float yaniTipFur = smoothstep(0.77, 0.985, vMapUv.y);
        float yaniShellCoverage = max(yaniRim, yaniTipFur * 0.68);
        diffuseColor.a *= mix(0.025, 1.0, yaniShellCoverage) * mix(1.0, 0.23, yaniInner);
      ` : ""}`,
    );
    if (!shell) {
      shader.fragmentShader = shader.fragmentShader.replace(
        "#include <lights_fragment_end>",
        `#include <lights_fragment_end>
        float yaniBackLight = 0.0;
        float yaniWrapLight = 0.0;
        #if NUM_DIR_LIGHTS > 1
          yaniBackLight = pow(saturate(dot(-normal, directionalLights[1].direction)), 1.45);
          yaniWrapLight = saturate((dot(normal, directionalLights[1].direction) + 0.34) / 1.34);
        #elif NUM_DIR_LIGHTS > 0
          yaniBackLight = pow(saturate(dot(-normal, directionalLights[0].direction)), 1.45);
          yaniWrapLight = saturate((dot(normal, directionalLights[0].direction) + 0.34) / 1.34);
        #endif
        vec3 yaniScatterColor = vec3(0.58, 0.19, 0.14);
        float yaniScatter = yaniThinCartilage
          * (yaniBackLight * 0.82 + yaniWrapLight * 0.16)
          * yaniSssStrength;
        reflectedLight.directDiffuse += yaniScatterColor * yaniScatter;
        reflectedLight.indirectDiffuse *= 1.0 - yaniCavity * 0.16;
        reflectedLight.directDiffuse *= 1.0 - yaniCavity * 0.09;`,
      );
    }
  };
  material.customProgramCacheKey = () => `yani-anatomical-${shell ? `shell-${shellLevel.toFixed(3)}` : "front"}-v4`;
}

function foldColorModel(material: THREE.MeshPhysicalMaterial): void {
  material.onBeforeCompile = (shader) => {
    shader.fragmentShader = shader.fragmentShader.replace(
      "#include <map_fragment>",
      `#include <map_fragment>
      float yaniFoldAcross = smoothstep(0.02, 0.35, vMapUv.x);
      float yaniFoldHeight = smoothstep(0.07, 0.16, vMapUv.y)
        * (1.0 - smoothstep(0.32, 0.43, vMapUv.y));
      vec3 yaniFoldDermis = vec3(0.39, 0.105, 0.095);
      diffuseColor.rgb = mix(diffuseColor.rgb, yaniFoldDermis, yaniFoldAcross * yaniFoldHeight * 0.72);
      diffuseColor.rgb *= mix(0.68, 1.0, smoothstep(0.075, 0.2, vMapUv.y));
      float yaniFoldSilhouette = smoothstep(0.018, 0.055, vMapUv.x)
        * (1.0 - smoothstep(0.29, 0.365, vMapUv.x))
        * smoothstep(0.073, 0.115, vMapUv.y)
        * (1.0 - smoothstep(0.335, 0.385, vMapUv.y));
      diffuseColor.a *= yaniFoldSilhouette;`,
    );
  };
  material.customProgramCacheKey = () => "yani-anatomical-fold-v2";
}

function tipColorModel(material: THREE.MeshPhysicalMaterial): void {
  material.onBeforeCompile = (shader) => {
    shader.fragmentShader = shader.fragmentShader.replace(
      "#include <map_fragment>",
      `#include <map_fragment>
      float yaniTipBreak = 0.765
        + sin(vMapUv.x * 18.0 + 0.7) * 0.018
        + sin(vMapUv.x * 41.0) * 0.009;
      float yaniTipMask = smoothstep(yaniTipBreak, yaniTipBreak + 0.055, vMapUv.y);
      vec3 yaniCharcoal = vec3(0.028, 0.042, 0.037);
      diffuseColor.rgb = mix(diffuseColor.rgb, yaniCharcoal, 0.94);
      diffuseColor.a *= yaniTipMask;`,
    );
  };
  material.customProgramCacheKey = () => "yani-anime-charcoal-tip-v1";
}

export function createEarMaterials(textures: EarTextures, maxShells = 8): EarMaterials {
  const sssUniforms = new Set<SssUniform>();
  const base = new THREE.MeshPhysicalMaterial({
    color: 0xffffff,
    map: textures.albedo,
    normalMap: textures.normal,
    normalScale: new THREE.Vector2(0.36, 0.54),
    roughnessMap: textures.roughness,
    roughness: 0.62,
    metalness: 0,
    anisotropy: 0.78,
    anisotropyMap: textures.anisotropy,
    anisotropyRotation: 0,
    sheen: 0.48,
    sheenColor: new THREE.Color(0xb8dfca),
    sheenRoughness: 0.7,
    specularIntensity: 0.72,
    specularColor: new THREE.Color(0xd8eee2),
    transparent: true,
    alphaTest: 0.018,
    side: THREE.DoubleSide,
  });
  frontColorModel(base, sssUniforms, false, 0);

  const back = new THREE.MeshBasicMaterial({
    color: 0x527463,
    side: THREE.DoubleSide,
  });
  const rim = new THREE.MeshBasicMaterial({
    color: 0x78a891,
    side: THREE.DoubleSide,
  });

  const fold = new THREE.MeshPhysicalMaterial({
    color: 0xffffff,
    map: textures.albedo,
    normalMap: textures.normal,
    normalScale: new THREE.Vector2(0.22, 0.34),
    roughnessMap: textures.roughness,
    roughness: 0.7,
    anisotropy: 0.42,
    anisotropyMap: textures.anisotropy,
    sheen: 0.32,
    sheenColor: new THREE.Color(0xd7b5a6),
    sheenRoughness: 0.78,
    transparent: true,
    alphaTest: 0.015,
    side: THREE.DoubleSide,
  });
  foldColorModel(fold);

  const tip = new THREE.MeshPhysicalMaterial({
    color: 0x26322d,
    map: textures.albedo,
    normalMap: textures.normal,
    normalScale: new THREE.Vector2(0.3, 0.46),
    roughnessMap: textures.roughness,
    roughness: 0.66,
    anisotropy: 0.82,
    anisotropyMap: textures.anisotropy,
    sheen: 0.5,
    sheenColor: new THREE.Color(0x718d80),
    sheenRoughness: 0.62,
    transparent: true,
    alphaTest: 0.035,
    polygonOffset: true,
    polygonOffsetFactor: -2,
    polygonOffsetUnits: -2,
    side: THREE.DoubleSide,
  });
  tipColorModel(tip);

  const shells = Array.from({ length: maxShells }, (_, index) => {
    const level = (index + 1) / maxShells;
    const material = new THREE.MeshPhysicalMaterial({
      color: 0xffffff,
      map: textures.albedo,
      normalMap: textures.normal,
      normalScale: new THREE.Vector2(0.26, 0.42),
      roughnessMap: textures.roughness,
      roughness: 0.68,
      metalness: 0,
      anisotropy: 0.84,
      anisotropyMap: textures.anisotropy,
      sheen: 0.74,
      sheenColor: new THREE.Color(0xd4ebde),
      sheenRoughness: 0.6,
      alphaMap: textures.density,
      alphaTest: 0.48 + level * 0.25,
      depthWrite: true,
      side: THREE.DoubleSide,
    });
    frontColorModel(material, sssUniforms, true, level);
    return material;
  });

  const hair = new THREE.MeshPhysicalMaterial({
    color: 0xb9d5c5,
    vertexColors: true,
    alphaMap: textures.hairAlpha,
    alphaTest: 0.018,
    alphaToCoverage: true,
    transparent: true,
    opacity: 0.38,
    depthWrite: false,
    roughness: 0.54,
    metalness: 0,
    anisotropy: 0.92,
    anisotropyMap: textures.anisotropy,
    sheen: 0.48,
    sheenColor: new THREE.Color(0xf1f0dc),
    sheenRoughness: 0.48,
    specularIntensity: 0.72,
    specularColor: new THREE.Color(0xf7f1d9),
    side: THREE.DoubleSide,
  });

  const ring = new THREE.MeshPhysicalMaterial({
    color: 0xd9ac50,
    metalness: 0.96,
    roughness: 0.16,
    clearcoat: 0.55,
    clearcoatRoughness: 0.12,
  });

  const disposable = [base, back, rim, fold, tip, ...shells, hair, ring];
  return {
    base,
    back,
    rim,
    fold,
    tip,
    shells,
    hair,
    ring,
    setSssStrength: (strength) => {
      const safe = THREE.MathUtils.clamp(strength, 0, 1.6);
      sssUniforms.forEach((uniform) => { uniform.value = safe; });
    },
    dispose: () => disposable.forEach((material) => material.dispose()),
  };
}
