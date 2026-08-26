import { describe, expect, it } from "vitest";

import { createEarAtmosphere } from "./atmosphere";

describe("Yani ear atmosphere", () => {
  it("keeps the field and veil shader contracts in sync", () => {
    const atmosphere = createEarAtmosphere();
    const expectedUniforms = ["uEarLeft", "uEarRight", "uMood", "uMoodAge", "uPointer", "uQuality", "uResolution", "uTime"];
    const pose = { yaw: 0.2, pitch: -0.1, splay: 0.05, cup: 0.1, tip: 0.3, lift: 0.02, ringSwing: 0.04 };

    expect(Object.keys(atmosphere.background.material.uniforms).sort()).toEqual(expectedUniforms);
    expect(Object.keys(atmosphere.veil.material.uniforms).sort()).toEqual(expectedUniforms);

    atmosphere.resize(800, 600);
    atmosphere.update({
      time: 3.5,
      moodAge: 0.4,
      mood: "alarm",
      quality: "balanced",
      pointerX: 1,
      pointerY: -1,
      pointerActive: 1,
      leftPose: pose,
      rightPose: { ...pose, yaw: -0.15 },
      leftEnergy: 0.7,
      rightEnergy: 0.4,
    });
    expect(atmosphere.background.material.uniforms.uResolution.value.toArray()).toEqual([800, 600]);
    expect(atmosphere.background.material.uniforms.uMood.value.toArray()).toEqual([0, 0, 0, 1]);
    expect(atmosphere.veil.material.uniforms.uMood.value.toArray()).toEqual([0, 0, 0, 1]);
    expect(atmosphere.background.material.uniforms.uEarLeft.value.toArray()).toEqual([0.2, -0.1, 0.3, 0.7]);
    expect(atmosphere.background.material.uniforms.uEarRight.value.toArray()).toEqual([-0.15, -0.1, 0.3, 0.4]);
    expect(atmosphere.background.material.uniforms.uMoodAge.value).toBe(0.4);
    expect(atmosphere.background.material.uniforms.uQuality.value).toBe(4);
    expect(atmosphere.background.material.uniforms.uTime.value).toBe(3.5);

    atmosphere.dispose();
  });
});
