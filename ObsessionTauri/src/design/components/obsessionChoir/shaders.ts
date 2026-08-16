export const CHOIR_FULLSCREEN_VERTEX_SHADER = `#version 300 es
layout(location = 0) in vec2 a_position;
out vec2 v_uv;
void main() {
  v_uv = a_position * 0.5 + 0.5;
  gl_Position = vec4(a_position, 0.0, 1.0);
}`;

export const CHOIR_WORLD_FRAGMENT_SHADER = `#version 300 es
precision highp float;
in vec2 v_uv;
out vec4 outColor;

uniform vec2 u_resolution;
uniform vec2 u_focus;
uniform vec2 u_gaze;
uniform vec4 u_body;
uniform float u_time;
uniform float u_phase;
uniform float u_lid_open;
uniform float u_pupil_scale;
uniform float u_line_flow;
uniform float u_chorus_reveal;
uniform float u_chorus_alignment;
uniform float u_carmine_depth;
uniform float u_fault_shear;
uniform float u_ritual;

const float TAU = 6.28318530718;

float hash21(vec2 p) {
  p = fract(p * vec2(123.34, 345.45));
  p += dot(p, p + 34.345);
  return fract(p.x * p.y);
}

float eyeSdf(vec2 p, vec2 radius, float openAmount) {
  float nx = clamp(p.x / radius.x, -1.0, 1.0);
  float arch = pow(max(0.0, 1.0 - nx * nx), 0.62);
  float centerLine = radius.y * nx * 0.09;
  float upper = centerLine + radius.y * openAmount * (0.78 * arch + 0.22 * arch * arch);
  float lower = centerLine - radius.y * openAmount * (0.58 * arch + 0.42 * arch * arch);
  float vertical = max(p.y - upper, lower - p.y);
  return max(abs(p.x) - radius.x, vertical);
}

float ring(float value, float center, float width) {
  return 1.0 - smoothstep(width, width * 2.2, abs(value - center));
}

vec4 choirEye(int index) {
  if (index == 0) return vec4(0.16, 0.22, 0.082, 0.026);
  if (index == 1) return vec4(0.36, 0.16, 0.064, 0.022);
  if (index == 2) return vec4(0.58, 0.25, 0.072, 0.024);
  if (index == 3) return vec4(0.25, 0.43, 0.070, 0.023);
  if (index == 4) return vec4(0.48, 0.48, 0.058, 0.019);
  if (index == 5) return vec4(0.70, 0.51, 0.068, 0.021);
  if (index == 6) return vec4(0.13, 0.72, 0.074, 0.023);
  if (index == 7) return vec4(0.40, 0.76, 0.062, 0.020);
  return vec4(0.67, 0.79, 0.078, 0.024);
}

void main() {
  vec2 aspect = vec2(u_resolution.x / max(1.0, u_resolution.y), 1.0);
  vec2 centered = (v_uv - vec2(0.5)) * aspect;
  float grain = hash21(floor(v_uv * u_resolution * 0.42));
  float smoke = sin(centered.x * 5.1 + sin(centered.y * 4.2 + u_time * 0.025))
    + sin(centered.y * 7.3 - centered.x * 2.1 - u_time * 0.018);
  vec3 color = vec3(0.0045, 0.0048, 0.0065);
  color += vec3(0.010, 0.009, 0.012) * (0.5 + 0.5 * smoke);
  color += vec3(0.006, 0.001, 0.003) * grain;

  // Nine eyes remain embedded in the engraving until the choir aligns.
  float hiddenRims = 0.0;
  float hiddenCores = 0.0;
  for (int index = 0; index < 9; index++) {
    vec4 spec = choirEye(index);
    vec2 local = (v_uv - spec.xy) * aspect;
    float tilt = (float(index) - 4.0) * 0.035;
    mat2 rotation = mat2(cos(tilt), -sin(tilt), sin(tilt), cos(tilt));
    local = rotation * local;
    float openness = mix(0.46 + 0.1 * sin(u_time * (0.13 + u_line_flow * 0.08) + float(index)), 0.88, u_chorus_alignment);
    float sdf = eyeSdf(local, spec.zw, openness);
    float rim = 1.0 - smoothstep(0.0007, 0.0025, abs(sdf));
    vec2 choirGaze = mix(
      vec2(sin(float(index) * 2.1 + u_time * (0.07 + u_line_flow * 0.05)), cos(float(index) * 1.7) * 0.5),
      vec2(0.0),
      u_chorus_alignment
    );
    float pupil = exp(-length(local - choirGaze * spec.zw * 0.35) * 115.0);
    float cadence = 0.5 + 0.5 * sin(float(index) * 2.43 + u_time * 0.11);
    hiddenRims += rim * mix(0.3 + cadence * 0.15, 1.0, u_chorus_alignment);
    hiddenCores += pupil * (1.0 - smoothstep(0.0, 0.003, sdf));
  }
  color += vec3(0.52, 0.51, 0.52) * hiddenRims * u_chorus_reveal * 0.34;
  color += vec3(0.35, 0.008, 0.055) * hiddenCores * u_chorus_reveal * (0.35 + u_chorus_alignment * 0.65);

  // The master eye is a cut in the scene: no sclera, only absence and refraction.
  vec2 bodyFocus = u_focus + (u_body.xy - vec2(0.5)) * vec2(0.076, 0.054);
  vec2 master = (v_uv - bodyFocus) * aspect;
  float bodyRoll = (u_body.z - 0.5) * 0.2;
  mat2 bodyRotation = mat2(cos(bodyRoll), -sin(bodyRoll), sin(bodyRoll), cos(bodyRoll));
  master = bodyRotation * master;
  master.x += u_fault_shear * sin(master.y * 61.0 + u_time * 4.3) * 0.008;
  vec2 masterRadius = vec2(
    0.225 * mix(0.965, 1.035, u_body.w),
    0.125 * mix(0.9, 1.1, u_body.w)
  );
  float masterSdf = eyeSdf(master, masterRadius, u_lid_open);
  float masterMask = 1.0 - smoothstep(-0.003, 0.003, masterSdf);
  float outerRim = 1.0 - smoothstep(0.0007, 0.0034, abs(masterSdf));
  float innerRim = (1.0 - smoothstep(0.0012, 0.005, abs(masterSdf + 0.0065))) * masterMask;
  color = mix(color, vec3(0.0006, 0.0007, 0.001), masterMask * 0.985);
  float rimSweep = 0.55 + 0.45 * sin(
    atan(master.y, master.x) * 2.0 - u_time * (0.28 + u_line_flow * 0.27)
  );
  float masterNx = clamp(master.x / masterRadius.x, -1.0, 1.0);
  float masterArch = pow(max(0.0, 1.0 - masterNx * masterNx), 0.62);
  float masterCenterLine = masterRadius.y * masterNx * 0.09;
  float masterUpper = masterCenterLine
    + masterRadius.y * u_lid_open * (0.78 * masterArch + 0.22 * masterArch * masterArch);
  float masterLower = masterCenterLine
    - masterRadius.y * u_lid_open * (0.58 * masterArch + 0.42 * masterArch * masterArch);
  float upperLid = smoothstep(-0.016, 0.032, master.y - masterCenterLine);
  vec3 rimTone = mix(vec3(0.37, 0.011, 0.068), vec3(0.82, 0.8, 0.78), upperLid);
  color += rimTone * outerRim * (0.24 + rimSweep * 0.2 + upperLid * 0.13);
  color += vec3(0.42, 0.009, 0.071) * innerRim * (0.12 + u_carmine_depth * 0.2);

  // A separate upper fold and lower wet line make the void read as an eye,
  // rather than as one symmetrical leaf/blade contour.
  float foldY = masterUpper + 0.015 * (0.35 + masterArch * 0.65);
  float upperFold = (1.0 - smoothstep(0.0012, 0.0054, abs(master.y - foldY)))
    * smoothstep(0.05, 0.42, masterArch);
  float wetY = masterLower + 0.005 * (0.25 + masterArch * 0.75);
  float lowerWet = (1.0 - smoothstep(0.0008, 0.0038, abs(master.y - wetY)))
    * masterMask * smoothstep(0.04, 0.36, masterArch);
  color += vec3(0.58, 0.56, 0.56) * upperFold * (0.09 + rimSweep * 0.09);
  color += vec3(0.75, 0.68, 0.66) * lowerWet * 0.13;
  color += vec3(0.34, 0.006, 0.052) * lowerWet * u_carmine_depth * 0.18;

  vec2 irisCenter = u_gaze * vec2(0.064, 0.043) - vec2(0.0, 0.004);
  vec2 irisP = master - irisCenter;
  float irisDistance = length(irisP);
  float irisMask = (1.0 - smoothstep(0.059, 0.066, irisDistance)) * masterMask;
  float irisAngle = atan(irisP.y, irisP.x);
  float irisClock = u_time * (0.31 + u_line_flow * 0.37);
  float radial = clamp(irisDistance / 0.064, 0.0, 1.0);
  float fiber = pow(abs(sin(irisAngle * 19.0 + radial * 7.0 - irisClock)), 7.0);
  float counterFiber = pow(abs(cos(irisAngle * 11.0 - radial * 10.0 + irisClock * 0.47)), 9.0);
  float deepFlow = 0.5 + 0.5 * sin(irisAngle * 5.0 - radial * 16.0 + irisClock * 0.34);
  vec3 iris = mix(vec3(0.005, 0.0005, 0.002), vec3(0.34, 0.008, 0.062), (1.0 - radial) * u_carmine_depth);
  iris += vec3(0.34, 0.012, 0.07) * fiber * (1.0 - radial) * u_carmine_depth * 0.42;
  iris += vec3(0.16, 0.004, 0.032) * counterFiber * deepFlow * u_carmine_depth * 0.34;
  iris += vec3(0.7, 0.62, 0.59) * ring(irisDistance, 0.061, 0.0011) * 0.22;
  color = mix(color, iris, irisMask * 0.98);
  float pupilBreath = mix(0.76, 1.24, u_pupil_scale) * mix(1.0, 1.12, u_ritual);
  vec2 pupilRadius = vec2(0.0095, 0.029) * pupilBreath;
  float pupilDistance = length(irisP / pupilRadius);
  float pupil = (1.0 - smoothstep(0.86, 1.08, pupilDistance)) * masterMask;
  color = mix(color, vec3(0.0), pupil);
  float pupilRim = 1.0 - smoothstep(0.94, 1.12, abs(pupilDistance - 1.0) + 0.94);
  color += vec3(0.82, 0.79, 0.76) * pupilRim * masterMask * 0.22;

  vec2 glintOrbit = vec2(sin(u_time * 0.31), cos(u_time * 0.23)) * vec2(0.012, 0.007);
  float wetGlint = exp(-length((master - irisCenter - vec2(-0.031, 0.037) - glintOrbit) * vec2(0.82, 2.2)) * 39.0);
  float wetGlintFine = exp(-length((master - irisCenter - vec2(0.019, 0.023) + glintOrbit * 0.55) * vec2(1.2, 2.7)) * 75.0);
  color += vec3(0.94, 0.91, 0.87) * wetGlint * masterMask * 0.45;
  color += vec3(0.82, 0.74, 0.73) * wetGlintFine * masterMask * 0.34;
  float cornerGlint = exp(-length((abs(master) - vec2(masterRadius.x * 0.94, 0.0)) * vec2(1.0, 4.5)) * 38.0);
  color += vec3(0.43, 0.025, 0.08) * cornerGlint * (0.34 + u_carmine_depth * 0.48);
  float halo = exp(-length(master * vec2(0.72, 1.55)) * 7.2);
  color += vec3(0.13, 0.003, 0.025) * halo * (1.0 - masterMask) * u_carmine_depth;

  float vignette = smoothstep(0.92, 0.18, length(centered * vec2(0.72, 1.0)));
  color *= 0.43 + vignette * 0.57;
  color = color / (color + vec3(1.0));
  color = pow(color, vec3(0.86));
  outColor = vec4(color, 1.0);
}`;

export const CHOIR_RIBBON_VERTEX_SHADER = `#version 300 es
precision highp float;
layout(location = 0) in vec2 a_position;
layout(location = 1) in vec2 a_normal;
layout(location = 2) in vec4 a_meta;

uniform vec2 u_resolution;
uniform float u_time;
uniform float u_tension;
uniform float u_line_flow;
uniform float u_fault_shear;

out float v_side;
out float v_along;
out float v_depth;
out float v_seed;
out vec2 v_position;

void main() {
  float side = a_meta.x;
  float along = a_meta.y;
  float depth = a_meta.z;
  float seed = a_meta.w;
  float flowClock = u_time * (0.08 + u_line_flow * 0.16);
  float breath = sin(flowClock * (0.72 + seed * 0.65) + along * 7.0 + seed * 12.0);
  float tensionWarp = breath * (0.0026 + depth * 0.0017) * (0.38 + u_tension * 0.72);
  tensionWarp += sin(along * 13.0 - flowClock * 1.7 + seed * 19.0)
    * 0.0011 * (0.35 + u_line_flow * 0.65);
  vec2 position = a_position + a_normal * tensionWarp;
  position.x += u_fault_shear * sin(position.y * 43.0 + seed * 19.0 + u_time * 5.0) * 0.005;
  // Keep enough physical coverage for stable sub-pixel engraving. The scene
  // texture is supersampled on high quality, so this factor preserves the
  // apparent width after the linear downsample into the canvas framebuffer.
  float widthPx = mix(0.66, 1.48, depth / 2.0);
  widthPx *= mix(0.84, 1.06, u_tension);
  vec2 widthUv = a_normal * side * widthPx / u_resolution;
  vec2 clip = (position + widthUv) * 2.0 - 1.0;
  gl_Position = vec4(clip, depth * 0.0001, 1.0);
  v_side = side;
  v_along = along;
  v_depth = depth;
  v_seed = seed;
  v_position = position;
}`;

export const CHOIR_RIBBON_FRAGMENT_SHADER = `#version 300 es
precision highp float;
in float v_side;
in float v_along;
in float v_depth;
in float v_seed;
in vec2 v_position;
out vec4 outColor;

uniform float u_time;
uniform float u_carmine_depth;
uniform float u_ritual;
uniform float u_line_flow;
uniform vec2 u_resolution;
uniform vec2 u_focus;
uniform vec4 u_body;
uniform float u_lid_open;

float eyeSdf(vec2 p, vec2 radius, float openAmount) {
  float nx = clamp(p.x / radius.x, -1.0, 1.0);
  float arch = pow(max(0.0, 1.0 - nx * nx), 0.62);
  float centerLine = radius.y * nx * 0.09;
  float upper = centerLine + radius.y * openAmount * (0.78 * arch + 0.22 * arch * arch);
  float lower = centerLine - radius.y * openAmount * (0.58 * arch + 0.42 * arch * arch);
  float vertical = max(p.y - upper, lower - p.y);
  return max(abs(p.x) - radius.x, vertical);
}

void main() {
  // Derivative-aware coverage removes the staircase that a fixed smoothstep
  // produces on long diagonal ribbons. A separate inner filament makes each
  // curve read like layered optical engraving rather than a flat pixel line.
  float sideDistance = abs(v_side);
  float edgeWidth = clamp(fwidth(v_side) * 0.72, 0.035, 0.42);
  float edge = 1.0 - smoothstep(1.0 - edgeWidth, 1.0, sideDistance);
  float filament = 1.0 - smoothstep(0.24, 0.62 + edgeWidth * 0.16, sideDistance);
  float flowClock = u_time * (0.14 + u_line_flow * 0.3);
  float travelling = pow(max(0.0, sin(v_along * 9.0 - flowClock + v_seed * 17.0)), 14.0);
  float travellingFine = pow(max(0.0, cos(v_along * 17.0 - flowClock * 1.7 + v_seed * 29.0)), 32.0);
  float depthAlpha = mix(0.052, 0.165, v_depth / 2.0);
  vec3 smoke = vec3(0.38, 0.37, 0.4);
  vec3 pearl = vec3(0.75, 0.73, 0.72);
  vec3 carmine = vec3(0.42, 0.012, 0.073);
  vec3 color = mix(smoke, carmine, smoothstep(0.25, 1.0, v_depth));
  color = mix(color, pearl, smoothstep(1.25, 2.0, v_depth));
  float carmineBand = smoothstep(0.58, 0.94, sin(v_along * 4.0 + v_seed * 13.0) * 0.5 + 0.5);
  color = mix(color, carmine, carmineBand * u_carmine_depth * (0.22 + (2.0 - v_depth) * 0.14));
  color += pearl * travelling * (0.15 + u_line_flow * 0.22 + u_ritual * 0.2);
  color += vec3(0.62, 0.05, 0.14) * travellingFine * u_carmine_depth * 0.2;
  color += mix(pearl, carmine, 0.24 + u_carmine_depth * 0.18)
    * filament * (0.045 + v_depth * 0.018);
  vec2 aspect = vec2(u_resolution.x / max(1.0, u_resolution.y), 1.0);
  vec2 bodyFocus = u_focus + (u_body.xy - vec2(0.5)) * vec2(0.076, 0.054);
  vec2 master = (v_position - bodyFocus) * aspect;
  float bodyRoll = (u_body.z - 0.5) * 0.2;
  mat2 bodyRotation = mat2(cos(bodyRoll), -sin(bodyRoll), sin(bodyRoll), cos(bodyRoll));
  master = bodyRotation * master;
  vec2 masterRadius = vec2(
    0.225 * mix(0.965, 1.035, u_body.w),
    0.125 * mix(0.9, 1.1, u_body.w)
  );
  float masterSdf = eyeSdf(master, masterRadius, u_lid_open);
  float masterCut = smoothstep(-0.022, 0.018, masterSdf);
  float focusGlow = exp(-length(master * vec2(0.8, 1.45)) * 5.8);
  float alpha = depthAlpha * (0.78 + travelling * 0.42 + travellingFine * 0.18);
  alpha += focusGlow * (0.012 + v_depth * 0.008) * u_carmine_depth;
  // Compensate for the double vignette so the lower-left half still carries
  // visible structure instead of surrendering all visual weight to the eye.
  float fieldBalance = 0.94
    + (1.0 - clamp(v_position.y, 0.0, 1.0)) * 0.22
    + (1.0 - clamp(v_position.x, 0.0, 1.0)) * 0.08;
  alpha *= fieldBalance;
  float layeredCoverage = edge * mix(0.82, 1.12, filament);
  outColor = vec4(color, layeredCoverage * alpha * masterCut);
}`;

export const CHOIR_COMPOSITE_FRAGMENT_SHADER = `#version 300 es
precision highp float;
in vec2 v_uv;
out vec4 outColor;

uniform sampler2D u_scene;
uniform vec2 u_resolution;
uniform vec2 u_pointer;
uniform float u_time;
uniform float u_refraction;
uniform float u_aberration;
uniform float u_caustics;
uniform int u_panel_count;
uniform vec4 u_panels[12];
uniform float u_panel_radii[12];

float roundedBox(vec2 p, vec2 halfSize, float radius) {
  vec2 q = abs(p) - halfSize + radius;
  return min(max(q.x, q.y), 0.0) + length(max(q, 0.0)) - radius;
}

void main() {
  vec2 uv = v_uv;
  float panelMask = 0.0;
  vec2 refractVector = vec2(0.0);
  for (int index = 0; index < 12; index++) {
    if (index >= u_panel_count) break;
    vec4 panel = u_panels[index];
    vec2 center = panel.xy + panel.zw * 0.5;
    vec2 local = uv - center;
    float sdf = roundedBox(local, panel.zw * 0.5, u_panel_radii[index]);
    float inside = 1.0 - smoothstep(-0.016, 0.006, sdf);
    vec2 normal = normalize(local / max(panel.zw, vec2(0.001)) + vec2(0.0001));
    float glassFlow = sin((local.x - local.y) * 78.0 + u_time * 0.17 + float(index)) * 0.5 + 0.5;
    refractVector += inside * (normal * 0.0024 + vec2(glassFlow - 0.5, 0.5 - glassFlow) * 0.0014);
    panelMask = max(panelMask, inside);
  }
  refractVector *= u_refraction;
  float pointerLight = max(0.0, 1.0 - length(v_uv - (u_pointer * 0.5 + 0.5)) * 2.4);
  vec2 sampleUv = clamp(uv + refractVector, vec2(0.001), vec2(0.999));
  vec3 base = texture(u_scene, sampleUv).rgb;
  if (panelMask > 0.001 && u_aberration > 0.0) {
    float split = 0.0013 * u_aberration * panelMask;
    base.r = texture(u_scene, clamp(sampleUv + vec2(split, 0.0), vec2(0.001), vec2(0.999))).r;
    base.b = texture(u_scene, clamp(sampleUv - vec2(split, 0.0), vec2(0.001), vec2(0.999))).b;
  }
  float softGlass = panelMask * 0.022;
  base += vec3(0.016, 0.015, 0.017) * softGlass;
  base += vec3(0.22, 0.017, 0.044) * panelMask * pointerLight * u_caustics * 0.008;
  float vignette = smoothstep(0.94, 0.24, length((uv - 0.5) * vec2(0.76, 1.0)));
  base *= 0.72 + vignette * 0.28;
  outColor = vec4(base, 1.0);
}`;
