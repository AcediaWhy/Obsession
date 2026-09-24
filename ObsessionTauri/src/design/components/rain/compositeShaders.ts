// Композитный проход GLSL 300 es объединяет мир, конденсат и преломление
// в каплях. Размытие вне капель берётся из mip-уровней буфера мира.
// Модель воды основана на codrops/RainEffect; дополнительные эффекты
// используют возможности WebGL2, включая HDR-буфер и дизеринг.

import { rainPassVert } from "./worldShaders";

export const compositeVert = rainPassVert;

export const compositeFrag = /* glsl */ `#version 300 es
precision highp float;

uniform sampler2D u_world;
uniform sampler2D u_water;
uniform sampler2D u_mist;
uniform sampler2D u_shine;
uniform vec2 u_resolution;
uniform float u_lightning;
uniform float u_time;
uniform float u_dropLod;
uniform float u_glassLod;
uniform float u_mistLod;
uniform float u_scatterLod;

in vec2 v_uv;
out vec4 outColor;

// Константы референса: alphaMultiply/alphaSubtract дают жёсткую кромку капли,
// min/maxRefraction — смещение в пикселях, brightness — капля светлее стекла.
const float ALPHA_MULTIPLY = 6.0;
const float ALPHA_SUBTRACT = 3.0;
const float MIN_REFRACTION = 150.0;
const float REFRACTION_DELTA = 362.0;
const float DROP_BRIGHTNESS = 1.10;
const float MAX_SHINE = 490.0;
const float WHITE_POINT = 3.2;

vec3 toSrgb(vec3 color) {
  return pow(max(color, vec3(0.0)), vec3(1.0 / 2.2));
}

// Дизер против полос на тёмных градиентах. Зависит только от координаты
// пикселя, поэтому не мерцает между кадрами.
float ditherNoise(vec2 fragCoord) {
  return fract(sin(dot(fragCoord, vec2(12.9898, 78.233))) * 43758.5453);
}

void main() {
  // Экранные координаты (y=0 сверху) — водная карта рисуется Canvas2D в том
  // же порядке; мир из FBO сэмплится по v_uv (GL y-вверх).
  vec2 sUv = vec2(v_uv.x, 1.0 - v_uv.y);
  vec2 pixel = 1.0 / u_resolution;

  vec4 water = texture(u_water, sUv);
  float thickness = water.b;
  vec2 refraction = (vec2(water.g, water.r) - 0.5) * 2.0;
  float dropAlpha = clamp(water.a * ALPHA_MULTIPLY - ALPHA_SUBTRACT, 0.0, 1.0);

  // Конденсат: тела капель и мокрые дорожки протирают его.
  float mist = texture(u_mist, sUv).r;
  mist *= 1.0 - dropAlpha * 0.92;

  // Стекло вне капель не в фокусе; конденсат добавляет размытия и вуали.
  // Вуаль держим слабой: мир стал HDR, и широкий scatter несёт энергию фонарей,
  // из-за которой кадр легко уходит в молочную дымку.
  vec3 glass = textureLod(u_world, v_uv, u_glassLod + mist * u_mistLod).rgb;
  vec3 scatter = textureLod(u_world, v_uv, u_scatterLod).rgb;
  glass = mix(glass, scatter * 0.90 + vec3(0.0025, 0.0032, 0.0045), mist * 0.10);

  // Содержимое капли: смещение в пикселях, как в референсе. Капля показывает
  // сильно сдвинутый и чуть более резкий, чем стекло, фрагмент мира.
  vec2 refractionOffset = pixel * refraction * (MIN_REFRACTION + thickness * REFRACTION_DELTA);
  vec2 dropUv = v_uv + vec2(refractionOffset.x, -refractionOffset.y);
  vec3 drop = textureLod(u_world, dropUv, u_dropLod).rgb * DROP_BRIGHTNESS;

  // Блик: matcap индексируется вектором рефракции, поэтому пятно света стоит
  // на всех каплях согласованно, как от одного источника.
  float minShine = MAX_SHINE * 0.18;
  vec2 shineUv = vec2(0.5) + (refraction / 512.0) * -(minShine + (MAX_SHINE - minShine) * thickness);
  float shine = texture(u_shine, shineUv).a;
  drop += vec3(0.86, 0.91, 1.0) * shine * (0.045 + thickness * 0.20);

  // Тень под каплей: та же альфа, сдвинутая вверх на толщину — капля отбирает
  // свет у стекла под собой и получает опору.
  float shadowAlpha = texture(u_water, sUv - vec2(0.0, thickness * 6.0) * pixel).a;
  shadowAlpha = clamp(shadowAlpha * ALPHA_MULTIPLY - (ALPHA_SUBTRACT + 0.5), 0.0, 1.0) * 0.26;

  vec3 color = glass * (1.0 - shadowAlpha);
  color = mix(color, drop, dropAlpha);

  // Зарница мягко заливает всё стекло (рассеяние в воде и конденсате).
  color *= 1.0 + u_lightning * 0.25;

  // ── Финальный грейд ────────────────────────────────────────────────
  vec2 vignettePos = (sUv - vec2(0.5, 0.5)) * vec2(1.06, 1.0);
  color *= 1.0 - smoothstep(0.52, 1.02, length(vignettePos)) * 0.20;
  float luma = dot(color, vec3(0.299, 0.587, 0.114));
  color = mix(color, luma * vec3(0.88, 0.98, 1.12), 0.06);

  // Тонмап: мир HDR — ядра фонарей ярче единицы, и без сжатия боке в каплях
  // просто клиппится в белое пятно. Расширенный Рейнхард с точкой белого.
  color = color * (1.0 + color / (WHITE_POINT * WHITE_POINT)) / (1.0 + color);

  vec3 srgb = toSrgb(color);
  srgb += (ditherNoise(gl_FragCoord.xy) - 0.5) / 255.0;
  outColor = vec4(srgb, 1.0);
}
`;
