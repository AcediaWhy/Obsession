#version 460 core

#include <flutter/runtime_effect.glsl>

uniform vec2 uResolution;
uniform float uTime;
uniform float uIntensity;
uniform float uSpeed;
uniform vec3 uAccentColor;
uniform float uRadius;
uniform float uDiskBrightness;
uniform float uLensIntensity;
uniform vec2 uMouse;

out vec4 fragColor;

const float PI = 3.14159265359;
const float TAU = 6.28318530718;

float hash(vec2 p) {
    return fract(sin(dot(p, vec2(127.1, 311.7))) * 43758.5453);
}

float hash3(vec3 p) {
    return fract(sin(dot(p, vec3(127.1, 311.7, 74.7))) * 43758.5453);
}

float noise(vec2 p) {
    vec2 i = floor(p);
    vec2 f = fract(p);
    f = f * f * (3.0 - 2.0 * f);
    float a = hash(i);
    float b = hash(i + vec2(1.0, 0.0));
    float c = hash(i + vec2(0.0, 1.0));
    float d = hash(i + vec2(1.0, 1.0));
    return mix(mix(a, b, f.x), mix(c, d, f.x), f.y);
}

float noise3(vec3 p) {
    vec3 i = floor(p);
    vec3 f = fract(p);
    f = f * f * (3.0 - 2.0 * f);
    float n = hash3(i);
    float nx = hash3(i + vec3(1.0, 0.0, 0.0));
    float ny = hash3(i + vec3(0.0, 1.0, 0.0));
    float nxy = hash3(i + vec3(1.0, 1.0, 0.0));
    float nz = hash3(i + vec3(0.0, 0.0, 1.0));
    float nxz = hash3(i + vec3(1.0, 0.0, 1.0));
    float nyz = hash3(i + vec3(0.0, 1.0, 1.0));
    float nxyz = hash3(i + vec3(1.0, 1.0, 1.0));
    return mix(
        mix(mix(n, nx, f.x), mix(ny, nxy, f.x), f.y),
        mix(mix(nz, nxz, f.x), mix(nyz, nxyz, f.x), f.y),
        f.z
    );
}

float fbm(vec2 p) {
    float v = 0.0;
    float amp = 0.5;
    for (int i = 0; i < 5; i++) {
        v += amp * noise(p);
        p *= 2.0;
        amp *= 0.5;
    }
    return v;
}

float fbm3(vec3 p) {
    float v = 0.0;
    float amp = 0.5;
    for (int i = 0; i < 4; i++) {
        v += amp * noise3(p);
        p *= 2.0;
        amp *= 0.5;
    }
    return v;
}

// Starfield with multiple layers, nebulae, and twinkle
vec3 starfield(vec2 uv, float t) {
    vec3 col = vec3(0.015, 0.02, 0.035);

    // Nebula clouds
    float neb1 = fbm(uv * 1.5 + t * 0.02);
    float neb2 = fbm(uv * 2.5 - t * 0.03 + 10.0);
    vec3 nebula = mix(
        vec3(0.05, 0.03, 0.08),
        vec3(0.02, 0.04, 0.07),
        neb2
    ) * neb1 * 0.4;
    col += nebula;

    // Distant stars
    for (int layer = 0; layer < 3; layer++) {
        float scale = 80.0 + float(layer) * 120.0;
        float h = hash(uv * scale + float(layer) * 50.0);
        float twinkle = sin(t * (1.5 + float(layer) * 0.8) + hash(uv * scale) * TAU) * 0.5 + 0.5;
        float star = smoothstep(0.998 - float(layer) * 0.001, 1.0, h);
        vec3 starColor = mix(
            vec3(0.8, 0.9, 1.0),
            vec3(1.0, 0.85, 0.7),
            hash(uv * scale * 1.3)
        );
        col += star * starColor * (0.3 + 0.7 * twinkle) * (0.6 - float(layer) * 0.15);
    }

    return col;
}

// Newtonian gravitational lensing with smooth falloff
vec2 gravitationalLens(vec2 uv, vec2 center, float radius, float strength) {
    vec2 delta = uv - center;
    float dist = length(delta);
    if (dist < radius * 0.05) return uv;

    float deflection = strength * radius / (dist + radius * 0.15);
    deflection *= smoothstep(radius * 12.0, radius * 0.8, dist);
    return uv - normalize(delta + 0.0001) * deflection;
}

// Chromatic aberration near the black hole
vec3 chromaticStarfield(vec2 uv, vec2 center, float radius, float strength) {
    vec2 delta = uv - center;
    float dist = length(delta);
    float aberration = smoothstep(radius * 5.0, radius * 0.6, dist) * strength * 0.04;
    vec2 dir = normalize(delta + 0.0001);

    float r = starfield(uv - dir * aberration * 2.0, uTime * 0.2).r;
    float g = starfield(uv, uTime * 0.2).g;
    float b = starfield(uv + dir * aberration * 2.0, uTime * 0.2).b;

    return vec3(r, g, b);
}

// Einstein ring: background stars wrapped around the black hole
vec3 einsteinRing(vec2 uv, vec2 center, float radius, float strength) {
    vec2 delta = uv - center;
    float dist = length(delta);
    float ringWidth = radius * 0.2;
    float ringRadius = radius * 1.6;

    float ring = smoothstep(ringRadius - ringWidth, ringRadius, dist)
               * smoothstep(ringRadius + ringWidth, ringRadius, dist);

    vec2 wrappedUv = center + delta * (ringRadius / max(dist, radius * 0.3));
    vec3 wrappedStars = starfield(wrappedUv * 4.0, uTime * 0.15);

    return wrappedStars * ring * strength * 3.0;
}

// Relativistic jets perpendicular to the disk
float jet(vec2 uv, vec2 center, float radius, float t) {
    vec2 delta = uv - center;
    float dist = length(delta);
    float angle = atan(delta.y, delta.x);

    // Two jets along y-axis (perpendicular to disk)
    float jetAngle1 = abs(sin(angle));
    float jetAngle2 = abs(cos(angle));

    float jetSpread = smoothstep(0.0, 0.15, 1.0 - jetAngle1);
    float lengthFalloff = exp(-dist / (radius * 6.0));
    float turbulence = fbm3(vec3(delta * 3.0, t * 0.5));

    return jetSpread * lengthFalloff * (0.4 + 0.6 * turbulence);
}

// Accretion disk texture with spiral arms and turbulence
float diskTexture(vec2 p, float t) {
    float angle = atan(p.y, p.x);
    float radius = length(p);
    float spiral1 = sin(angle * 10.0 - radius * 16.0 - t * 2.0) * 0.5 + 0.5;
    float spiral2 = sin(angle * 6.0 + radius * 12.0 - t * 0.9) * 0.5 + 0.5;
    float turb = fbm(vec2(angle * 5.0, radius * 12.0 + t * 0.4));
    return mix(spiral1 * spiral2, turb, 0.45);
}

void main() {
    vec2 fragCoord = FlutterFragCoord().xy;
    vec2 uv = (fragCoord - 0.5 * uResolution.xy) / uResolution.y;
    vec2 center = vec2(0.0);

    // Mouse interaction
    vec2 mouseUv = (uMouse - 0.5 * uResolution.xy) / uResolution.y;
    vec2 mouseOffset = mouseUv * 0.1 * uLensIntensity;
    vec2 lensCenter = center + mouseOffset;

    float screenRadius = uRadius * 0.5;
    float dist = length(uv - lensCenter);

    // Apply strong gravitational lensing
    float lensStrength = uLensIntensity * 1.5;
    vec2 lensUv = gravitationalLens(uv, lensCenter, screenRadius, lensStrength);

    // Chromatic background
    vec3 bg = chromaticStarfield(lensUv * 2.0, lensCenter, screenRadius, uLensIntensity);

    // Einstein ring
    bg += einsteinRing(uv, lensCenter, screenRadius, uLensIntensity);

    // Relativistic jets
    float jetIntensity = jet(uv, lensCenter, screenRadius, uTime * uSpeed);
    vec3 jetColor = mix(vec3(0.6, 0.7, 1.0), uAccentColor, 0.3);
    bg += jetColor * jetIntensity * 0.15 * uDiskBrightness * uLensIntensity;

    // Accretion disk
    vec2 diskUv = uv - lensCenter;
    float diskRadius = length(diskUv);
    float diskAngle = atan(diskUv.y, diskUv.x);

    float innerDisk = screenRadius * 1.35;
    float outerDisk = screenRadius * 5.0;
    float diskMask = smoothstep(outerDisk, innerDisk, diskRadius)
                   * smoothstep(screenRadius * 1.02, innerDisk, diskRadius);

    // Doppler beaming
    float doppler = 1.0 + 0.55 * cos(diskAngle - 1.57);
    float redshift = 1.0 + 0.35 * sin(diskAngle + 1.57);

    // Temperature gradient
    float diskT = clamp((diskRadius - innerDisk) / (outerDisk - innerDisk), 0.0, 1.0);
    vec3 diskColorInner = vec3(1.0, 0.99, 0.9);
    vec3 diskColorMid = vec3(1.0, 0.65, 0.25);
    vec3 diskColorOuter = vec3(0.75, 0.2, 0.35);
    vec3 diskCol = mix(diskColorInner, diskColorMid, diskT);
    diskCol = mix(diskCol, diskColorOuter, smoothstep(0.4, 1.0, diskT));

    // Gravitational redshift near horizon
    float redshiftFactor = smoothstep(outerDisk, innerDisk, diskRadius);
    diskCol = mix(diskCol, vec3(1.0, 0.45, 0.25), redshiftFactor * 0.35);

    // Blend accent color subtly
    diskCol = mix(diskCol, uAccentColor, 0.1);

    float diskTex = diskTexture(diskUv, uTime * uSpeed);
    diskCol *= (0.45 + 0.95 * diskTex) * doppler * redshift * uDiskBrightness;

    // Photon ring - brighter and more detailed
    float photonRing = smoothstep(screenRadius * 1.52, screenRadius * 1.55, diskRadius)
                     * smoothstep(screenRadius * 1.58, screenRadius * 1.55, diskRadius);
    diskCol += photonRing * vec3(1.0, 0.98, 0.85) * 4.0 * uDiskBrightness;

    // Secondary photon ring
    float photonRing2 = smoothstep(screenRadius * 1.72, screenRadius * 1.75, diskRadius)
                      * smoothstep(screenRadius * 1.78, screenRadius * 1.75, diskRadius);
    diskCol += photonRing2 * vec3(1.0, 0.95, 0.75) * 1.5 * uDiskBrightness;

    bg = mix(bg, diskCol, diskMask);

    // Event horizon with soft glowing edge
    float horizonEdge = smoothstep(screenRadius * 1.08, screenRadius * 0.96, dist);
    float horizonGlow = smoothstep(screenRadius * 1.15, screenRadius * 1.0, dist)
                      * smoothstep(screenRadius * 0.9, screenRadius * 1.0, dist);
    bg = mix(bg, vec3(0.0), horizonEdge);
    bg += horizonGlow * uAccentColor * 0.15 * uDiskBrightness;

    // Vignette
    float vig = 1.0 - length(uv) * 0.2;
    bg *= vig;

    fragColor = vec4(bg * uIntensity, 1.0);
}
