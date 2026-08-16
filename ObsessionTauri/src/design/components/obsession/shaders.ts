export const OBSESSION_VERTEX_SHADER = `#version 300 es
layout(location = 0) in vec2 a_position;
out vec2 v_uv;
void main() {
  v_uv = a_position * 0.5 + 0.5;
  gl_Position = vec4(a_position, 0.0, 1.0);
}`;

export const OBSESSION_FRAGMENT_SHADER = `#version 300 es
precision highp float;
in vec2 v_uv;
out vec4 outColor;

uniform sampler2D u_plate;
uniform sampler2D u_normal;
uniform vec2 u_resolution;
uniform vec2 u_focus;
uniform vec2 u_pointer;
uniform vec2 u_gaze;
uniform float u_time;
uniform float u_phase;
uniform float u_phase_age;
uniform float u_threads;
uniform float u_caustics;
uniform float u_aberration;
uniform float u_capture;
uniform float u_lid_open;
uniform float u_pupil_scale;
uniform float u_body_tension;
uniform float u_iris_rotation;
uniform float u_highlight_phase;
uniform float u_fixation;
uniform float u_fault_split;

#define PI 3.14159265359
#define TAU 6.28318530718

float sdLine(vec2 p, vec2 a, vec2 b) {
  vec2 pa = p - a;
  vec2 ba = b - a;
  float h = clamp(dot(pa, ba) / dot(ba, ba), 0.0, 1.0);
  return length(pa - ba * h);
}

float ring(float radius, float center, float width) {
  return 1.0 - smoothstep(width, width * 2.2, abs(radius - center));
}

float invertedSmoothstep(float low, float high, float value) {
  return 1.0 - smoothstep(low, high, value);
}

float almondSdf(vec2 p, float rx, float ry) {
  float nx = abs(p.x) / rx;
  float lid = ry * pow(max(0.0, 1.0 - nx), 0.62);
  return max(abs(p.x) - rx, abs(p.y) - lid);
}

float hash21(vec2 p) {
  p = fract(p * vec2(123.34, 345.45));
  p += dot(p, p + 34.345);
  return fract(p.x * p.y);
}

void main() {
  vec2 aspect = vec2(u_resolution.x / max(1.0, u_resolution.y), 1.0);
  float scanDrive = u_phase == 2.0 ? 1.0 : 0.0;
  float focusedDrive = u_phase == 3.0 ? 1.0 : 0.0;
  float opticalTime = u_time * mix(0.72, 1.35, scanDrive);
  float breath = 0.5 + 0.5 * sin(opticalTime * 0.78 + sin(opticalTime * 0.21) * 0.7);
  vec2 bodyDrift = vec2(u_gaze.x * 0.013, u_gaze.y * 0.008)
    + vec2(sin(opticalTime * 0.29), cos(opticalTime * 0.25)) * 0.0018 * (1.0 - focusedDrive);
  vec2 centered = (v_uv - u_focus) * aspect - bodyDrift;
  float radius = length(centered);
  float angle = atan(centered.y, centered.x);

  vec2 materialUv = v_uv * vec2(1.0, 0.82)
    + vec2(0.0, u_time * 0.00012)
    + u_pointer * vec2(0.0013, 0.0009);
  vec2 normal = texture(u_normal, materialUv).rg * 2.0 - 1.0;
  float lensField = invertedSmoothstep(0.02, 0.38, radius);
  vec2 refractedUv = v_uv + normal * (0.002 + lensField * 0.007)
    - normalize(centered + 0.0001) * lensField * 0.003;
  vec3 plate = texture(u_plate, refractedUv).rgb;

  // Глубокий темный фон
  vec3 color = plate * 0.65 + vec3(0.003, 0.0035, 0.006);

  // ─── 1. МНОГОСЛОЙНАЯ ВЕКТОРНАЯ СЕТКА И АСТРОЛЯБИЯ ────────────────────────────
  vec3 vectorTitanium = vec3(0.84, 0.87, 0.92);
  vec3 vectorCarmine = vec3(0.78, 0.11, 0.26);
  vec3 vectorGold = vec3(0.88, 0.75, 0.52);

  float rotSlow = opticalTime * 0.04;
  float rotMid = -opticalTime * 0.07;
  float rotFast = opticalTime * 0.12 + u_iris_rotation * 0.3;

  // Масштабные орбитальные кольца
  float r1 = ring(radius, 0.14, 0.0012);
  float r2 = ring(radius, 0.24, 0.0015);
  float r3 = ring(radius, 0.38, 0.0018);
  float r4 = ring(radius, 0.56, 0.0016);
  float r5 = ring(radius, 0.78, 0.002);

  // Засечки и деления шкал
  float ticks12 = pow(max(0.0, cos((angle + rotSlow) * 12.0)), 24.0) * ring(radius, 0.38, 0.008);
  float ticks72 = pow(max(0.0, cos((angle + rotMid) * 72.0)), 32.0) * ring(radius, 0.56, 0.006);
  float ticks144 = pow(max(0.0, cos((angle + rotFast) * 144.0)), 28.0) * ring(radius, 0.24, 0.005);
  float ticksInner = pow(max(0.0, cos((angle - rotFast * 1.5) * 36.0)), 20.0) * ring(radius, 0.14, 0.004);

  // Дуговые секторы астролябии (gimbal arcs)
  float arc1 = pow(max(0.0, cos(angle * 3.0 + rotMid)), 16.0) * ring(radius, 0.32, 0.003);
  float arc2 = pow(max(0.0, cos(angle * 4.0 - rotSlow * 2.0)), 22.0) * ring(radius, 0.46, 0.003);
  float arc3 = pow(max(0.0, cos(angle * 6.0 + rotFast)), 18.0) * ring(radius, 0.68, 0.0025);

  // Тончайшая радиальная сетка (лучи полярных координат)
  float radialRays = pow(max(0.0, cos((angle + rotSlow * 0.5) * 8.0)), 42.0) * smoothstep(0.08, 0.85, radius);

  // Параметрическая спираль / гильош
  float guil = pow(max(0.0, cos(angle * 6.0 + radius * 42.0 - opticalTime * 0.2)), 12.0)
    * smoothstep(0.12, 0.32, radius) * (1.0 - smoothstep(0.62, 0.78, radius));

  color += vectorTitanium * (r1 * 0.22 + r2 * 0.18 + r3 * 0.25 + r4 * 0.2 + r5 * 0.15);
  color += vectorTitanium * (ticks12 * 0.35 + ticks72 * 0.25 + ticks144 * 0.3 + ticksInner * 0.25);
  color += vectorCarmine * (arc1 * 0.45 + arc2 * 0.35 + arc3 * 0.3);
  color += vectorTitanium * (radialRays * 0.09 + guil * 0.12);

  // ─── 2. УТОНЧЕННОЕ ВЕКТОРНОЕ ОКО (MASTER OCULUS) ─────────────────────────────
  // Уменьшенный, гармоничный размер Ока
  float eyeScale = 0.62;
  float openHeight = mix(0.008, (0.095 + (breath - 0.5) * 0.008), u_lid_open) * eyeScale;
  openHeight *= mix(0.98, 0.91, u_body_tension);
  float bodyWidth = (0.185 + u_body_tension * 0.015) * eyeScale;

  float eyeSdf = almondSdf(centered, bodyWidth, openHeight);
  float eyeMask = 1.0 - smoothstep(-0.003, 0.003, eyeSdf);
  float wetRim = 1.0 - smoothstep(0.0015, 0.006, abs(eyeSdf));
  float innerRim = (1.0 - smoothstep(0.001, 0.008, abs(eyeSdf + 0.004))) * eyeMask;

  // Оптические скобы по краям
  float cornerLeft = ring(length(centered - vec2(-bodyWidth * 0.98, 0.0)), 0.012, 0.001);
  float cornerRight = ring(length(centered - vec2(bodyWidth * 0.98, 0.0)), 0.012, 0.001);
  color += vectorTitanium * (cornerLeft + cornerRight) * 0.35;

  // Склера
  float pearl = invertedSmoothstep(0.02, 0.22, length(centered * vec2(0.8, 1.8)));
  float upperLight = invertedSmoothstep(0.01, 0.12, length(centered - vec2(-0.045, 0.035)));
  float sideShade = smoothstep(0.07, 0.18, abs(centered.x));
  vec3 sclera = mix(vec3(0.035, 0.036, 0.045), vec3(0.42, 0.44, 0.48), pearl);
  sclera += vec3(0.35, 0.38, 0.42) * upperLight * 0.4;
  sclera *= 1.0 - sideShade * 0.75;
  sclera += vec3(0.18, 0.02, 0.05) * (0.12 + u_body_tension * 0.15);

  // Радужка
  vec2 irisCenter = u_gaze * vec2(0.032, 0.02);
  vec2 irisP = centered - irisCenter;
  float irisRadius = (0.056 - u_body_tension * 0.003) * eyeScale;
  float irisD = length(irisP);
  float irisMask = (1.0 - smoothstep(irisRadius - 0.003, irisRadius + 0.003, irisD)) * eyeMask;
  float irisAngle = atan(irisP.y, irisP.x) + u_iris_rotation;

  float irisRays = 0.5 + 0.5 * sin(irisAngle * 48.0 + sin(irisAngle * 10.0) * 1.5 + irisD * 420.0);
  irisRays = pow(irisRays, 2.6) * smoothstep(0.01, irisRadius, irisD);
  float irisRingRays = ring(irisD, irisRadius * 0.65, 0.0015) * pow(max(0.0, cos(irisAngle * 24.0)), 6.0);

  vec3 irisColor = mix(vec3(0.035, 0.01, 0.02), vec3(0.55, 0.05, 0.14), irisRays);
  irisColor += vec3(0.78, 0.72, 0.68) * irisRays * 0.25;
  irisColor += vec3(0.9, 0.92, 0.98) * irisRingRays * 0.4;
  irisColor += vec3(0.28, 0.02, 0.06) * invertedSmoothstep(0.0, irisRadius, irisD);

  // Зрачок
  float pupilRadius = 0.016 * eyeScale * u_pupil_scale;
  float pupil = (1.0 - smoothstep(pupilRadius - 0.0018, pupilRadius + 0.0018, irisD)) * eyeMask;
  float pupilRim = ring(irisD, pupilRadius + 0.002, 0.0012) * eyeMask;
  float pupilCore = ring(irisD, pupilRadius * 0.45, 0.0009) * eyeMask;

  color = mix(color, sclera, eyeMask * 0.94);
  color = mix(color, irisColor, irisMask * 0.98);
  color = mix(color, vec3(0.001, 0.001, 0.002), pupil);
  color += vec3(0.96, 0.94, 0.91) * (pupilRim * 0.35 + pupilCore * 0.22);

  // Влажные блики роговицы
  float highlightAngle = u_highlight_phase * TAU;
  vec2 highlightCenter = irisCenter
    + vec2(cos(highlightAngle), sin(highlightAngle) * 0.55) * 0.012
    + vec2(-0.02, 0.025);
  float primaryGlint = exp(-length(centered - highlightCenter) * 120.0);
  float softGlint = exp(-length((centered - vec2(-0.05, 0.035)) * vec2(1.0, 1.8)) * 24.0);
  color += vec3(0.98, 0.97, 0.95) * (primaryGlint * 0.85 + softGlint * 0.2) * eyeMask;

  // Окантовка века
  color += vec3(0.94, 0.91, 0.88) * wetRim * (0.3 + (1.0 - u_body_tension) * 0.15);
  color += vec3(0.42, 0.02, 0.08) * innerRim * (0.5 + u_body_tension * 0.25);
  float upperWet = wetRim * smoothstep(-0.01, 0.04, centered.y);
  color += vec3(0.98, 0.96, 0.94) * upperWet * 0.2;

  // ─── 3. ДИНАМИЧЕСКИЕ ЛУЧИ ВЗГЛЯДА (SIGHT VECTORS & PULSES) ───────────────────
  float threads = 0.0;
  for (int i = 0; i < 16; i++) {
    if (float(i) >= u_threads) break;
    float fi = float(i);
    float side = mod(fi, 4.0);
    float lane = fract(fi * 0.61803398875);
    vec2 origin = side < 1.0 ? vec2(0.0, lane)
      : side < 2.0 ? vec2(1.0, lane)
      : side < 3.0 ? vec2(lane, 0.0)
      : vec2(lane, 1.0);
    float movement = scanDrive > 0.5 ? sin(u_time * 1.4 + fi * 2.1) * 0.016 : sin(u_time * 0.15 + fi) * 0.003;
    origin += vec2(movement, -movement * 0.45);
    float d = sdLine(v_uv, origin, u_focus + irisCenter / aspect);

    float pulsePos = fract(u_time * (scanDrive > 0.5 ? 0.9 : 0.25) + fi * 0.23);
    float pulseGlow = exp(-abs(length(v_uv - origin) - pulsePos * 1.4) * 8.0);

    threads += exp(-d * (880.0 - fi * 10.0)) * (0.24 + fract(fi * 0.37) * 0.35 + pulseGlow * 0.45);
  }
  threads *= smoothstep(0.02, 0.24, radius) * (1.0 - eyeMask * 0.72);
  color += vectorTitanium * threads * 0.08;
  color += vectorCarmine * threads * 0.22;

  // Хроматическая аберрация
  float chroma = (r2 + r3 + wetRim * 0.55) * u_aberration;
  color.r += chroma * 0.035 * sin(angle * 2.0 + opticalTime * 0.11);
  color.b += chroma * 0.026 * cos(angle * 2.0 - opticalTime * 0.09);

  // ─── 4. СОСТОЯНИЕ СБОЯ (FAULT DIPLOPIA) ───────────────────────────────────────
  if (u_fault_split > 0.0) {
    vec2 splitCenter = irisCenter + vec2(0.012, -0.003) * u_fault_split;
    float splitIris = ring(length(centered - splitCenter), irisRadius, 0.004) * eyeMask;
    color += vec3(0.48, 0.02, 0.09) * splitIris * 0.7 * u_fault_split;
    color.gb *= 1.0 - ring(length(centered - irisCenter + vec2(0.008, 0.002)), irisRadius, 0.005) * 0.16 * u_fault_split;
  }

  color += vec3(0.28, 0.02, 0.06) * u_fixation * irisMask * 0.25;
  float captureRing = ring(radius, mix(0.62, 0.04, u_capture), 0.025);
  color += vec3(0.88, 0.85, 0.82) * captureRing * (1.0 - u_capture) * 0.08;

  // Мягкая виньетка по краям экрана
  float vignette = invertedSmoothstep(0.25, 1.05, length((v_uv - vec2(0.5)) * vec2(0.72, 1.0)));
  color *= 0.6 + vignette * 0.4;
  outColor = vec4(color, 1.0);
}`;
