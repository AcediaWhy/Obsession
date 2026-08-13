// Проход A: мир за стеклом. Реальная подложка (ночная улица с фонарями) плюс
// процедурная атмосфера. Никаких аналитических силуэтов: структуру кадра даёт
// плита, а код добавляет то, чего в статичном кадре нет — параллакс, дождевую
// вуаль в конусе света, дыхание тумана и зарницы.
//
// Ассеты печёт scripts/bake-rain-plate.ps1:
//   u_plate    — кадр 2048x1152 (sRGB);
//   u_emission — только ядра источников света в их родном цвете. Здесь они
//                поднимаются до HDR-яркости, поэтому FBO обязан быть RGBA16F:
//                именно из этого запаса капли собирают боке.
//
// Результат — линейный свет; композит делает тонмап и гамму после рефракции.

export const rainPassVert = /* glsl */ `#version 300 es
layout(location = 0) in vec2 a_position;
out vec2 v_uv;
void main() {
  v_uv = a_position * 0.5 + 0.5;
  gl_Position = vec4(a_position, 0.0, 1.0);
}
`;

export const worldPlateFrag = /* glsl */ `#version 300 es
precision highp float;

uniform sampler2D u_plate;
uniform sampler2D u_emission;
uniform vec2 u_resolution;
uniform vec2 u_parallax;
uniform float u_plateAspect;
uniform float u_lightning;
uniform float u_time;
uniform float u_activity;
uniform float u_wind;

in vec2 v_uv;
out vec4 outColor;

// Запас на параллакс: плита показывается чуть крупнее кадра, поэтому сдвиг
// открывает реальные пиксели, а не растянутый край.
const float PARALLAX_ZOOM = 1.05;
const float EMISSION_GAIN = 4.0;
const float HALO_GAIN = 0.45;

float saturate(float value) {
  return clamp(value, 0.0, 1.0);
}

float hash21(vec2 value) {
  vec3 p3 = fract(vec3(value.xyx) * 0.1031);
  p3 += dot(p3, p3.yzx + 33.33);
  return fract((p3.x + p3.y) * p3.z);
}

float valueNoise(vec2 point) {
  vec2 cell = floor(point);
  vec2 local = fract(point);
  local = local * local * (3.0 - 2.0 * local);
  float a = hash21(cell);
  float b = hash21(cell + vec2(1.0, 0.0));
  float c = hash21(cell + vec2(0.0, 1.0));
  float d = hash21(cell + vec2(1.0, 1.0));
  return mix(mix(a, b, local.x), mix(c, d, local.x), local.y);
}

vec3 toLinear(vec3 color) {
  return pow(max(color, vec3(0.0)), vec3(2.2));
}

/** Cover-fit: плита заполняет кадр без искажения пропорций. */
vec2 plateUv(vec2 uv, vec2 shift) {
  float viewAspect = u_resolution.x / max(u_resolution.y, 1.0);
  vec2 scale = vec2(1.0);
  if (viewAspect > u_plateAspect) {
    scale.y = u_plateAspect / viewAspect;
  } else {
    scale.x = viewAspect / u_plateAspect;
  }
  return (uv - 0.5) * scale / PARALLAX_ZOOM + 0.5 + shift;
}

/** Длинные вертикальные нити дождя: шум, растянутый по вертикали и уведённый
 *  ветром. Видны в основном там, где есть свет. */
float rainVeil(vec2 uv, float time, float wind) {
  vec2 q = vec2(uv.x + uv.y * (0.10 + wind * 0.05), uv.y - time * 0.55);
  float filaments = valueNoise(vec2(q.x * 460.0, q.y * 24.0));
  float streaks = pow(saturate(filaments), 6.0);
  float fine = pow(saturate(valueNoise(vec2(q.x * 900.0, q.y * 52.0 + 11.0))), 8.0);
  return streaks * 0.7 + fine * 0.45;
}

void main() {
  // Картинки лежат первой строкой вверх, а v_uv — GL-овский (y вверх), поэтому
  // плита сэмплится по экранной координате: иначе кадр встаёт вверх ногами.
  vec2 screenUv = vec2(v_uv.x, 1.0 - v_uv.y);
  vec2 shift = u_parallax * vec2(0.0075, 0.0042);
  vec2 uv = plateUv(screenUv, shift);
  // Эмиссия двигается вместе с плитой, но чуть слабее: источники «дальше».
  vec2 emissionUv = plateUv(screenUv, shift * 0.72);

  vec3 plate = toLinear(texture(u_plate, uv).rgb);
  vec3 core = toLinear(texture(u_emission, emissionUv).rgb);
  // Широкий ореол берётся из мипов карты эмиссии: рассеяние света в воздухе.
  vec3 halo = toLinear(textureLod(u_emission, emissionUv, 4.0).rgb);

  // Живой свет: лампы едва заметно дышат, разряд усиливает атмосферу.
  float flicker = 0.97 + 0.03 * valueNoise(vec2(u_time * 0.7, 3.1));
  vec3 color = plate;
  color += core * EMISSION_GAIN * flicker;
  color += halo * HALO_GAIN * flicker;

  // Дождь в конусе света: без освещения капли в воздухе не видны.
  float haloLuma = dot(halo, vec3(0.299, 0.587, 0.114));
  float veil = rainVeil(screenUv, u_time, u_wind);
  color += vec3(0.86, 0.92, 1.0) * veil * haloLuma * (2.2 + 3.4 * u_activity);

  // Дыхание тумана поверх дальнего плана, привязано к ореолу.
  float breath = valueNoise(vec2(screenUv.x * 2.4 - u_time * 0.03, screenUv.y * 3.1 + u_time * 0.02));
  color += halo * (0.06 + 0.10 * u_activity) * breath;

  // Зарница: сильнее по верху кадра, где небо между деревьями.
  float lightningShape = 0.32 + (1.0 - screenUv.y) * 0.68;
  color += vec3(0.030, 0.038, 0.048) * u_lightning * lightningShape;

  outColor = vec4(color, 1.0);
}
`;
