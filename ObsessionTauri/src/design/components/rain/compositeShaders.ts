// Проход B (композит) — GLSL 300 es. Весь вьюпорт — запотевшее стекло ночью,
// за ним видео-мир (FBO с мипами). Формула воды — дословно из codrops
// RainEffect (water.frag): спрайт капли несёт фото-рефракционную карту
// (G→x, R→y), B — толщину, A — маску-слезу; альфа раскручивается ×6−3,
// нутро капли — сильно смещённый (256..512 px) сэмпл мягкого мипа мира с
// лёгким подъёмом яркости. Роли крошечных статических текстур демо (fg 96px,
// bg 384px) играют мипы видео-FBO: u_fgLod/u_bgLod. Поверх — конденсат
// (mistSim): доп. блюр + молочная вуаль, протираемая каплями.

import { rainPassVert } from "./worldShaders";

export const compositeVert = rainPassVert;

export const compositeFrag = /* glsl */ `#version 300 es
precision highp float;

uniform sampler2D u_world;
uniform sampler2D u_water;
uniform sampler2D u_mist;
uniform vec2 u_resolution;
uniform float u_lightning;
uniform float u_time;
uniform float u_fgLod;
uniform float u_bgLod;

in vec2 v_uv;
out vec4 outColor;

vec3 toSrgb(vec3 color) {
  return pow(max(color, vec3(0.0)), vec3(1.0 / 2.2));
}

float hash21(vec2 value) {
  value = fract(value * vec2(123.34, 456.21));
  value += dot(value, value + 45.32);
  return fract(value.x * value.y);
}

void main() {
  // Экранные координаты (y=0 сверху) — водная карта рисуется Canvas2D в том
  // же порядке; мир из FBO сэмплится по v_uv (GL y-вверх).
  vec2 sUv = vec2(v_uv.x, 1.0 - v_uv.y);

  // Вода по codrops: G→x, R→y (фото-карта рефракции), B — толщина, A — маска.
  vec4 water = texture(u_water, sUv);
  float thickness = water.b;
  float alpha = clamp(water.a * 6.0 - 3.0, 0.0, 1.0);
  vec2 refraction = (vec2(water.g, water.r) - 0.5) * 2.0;

  // Конденсат: сетка симуляции, равномерно по стеклу; тела капель и мокрые
  // дорожки протирают его (в симе — wipes, тут — маска воды добивает).
  float mist = texture(u_mist, sUv).r;
  mist *= 1.0 - smoothstep(0.10, 0.50, water.a) * 0.9;

  // Смещение рефракции — в «пикселях демо» (эталон 1080p), инвариантно к
  // backing-разрешению и анизотропии кадра. Y канвасный (вниз) → в мир с минусом.
  vec2 pixelN = vec2(u_resolution.y / u_resolution.x, 1.0) / 1080.0;
  vec2 refrOff = refraction * (256.0 + thickness * 256.0) * pixelN;
  vec2 dropUv = v_uv + vec2(refrOff.x, -refrOff.y);

  // Базовое стекло почти резкое (видеофон сам мягкий — лишний блюр давал
  // кашу); конденсат добавляет умеренную муть и лёгкую вуаль рассеяния.
  vec3 world = textureLod(u_world, v_uv, u_bgLod + mist * 1.2).rgb;
  vec3 scatter = textureLod(u_world, v_uv, 5.2).rgb;
  world = mix(world, scatter * 1.5 + vec3(0.0050, 0.0060, 0.0090), mist * 0.24);

  // Нутро капли: очень мягкий мип (эталон — fg 96px демо), сильный сдвиг,
  // лёгкий подъём яркости — капля читается светящейся линзой на тёмном стекле.
  vec3 dropWorld = textureLod(u_world, dropUv, u_fgLod).rgb * 1.06;
  world = mix(world, dropWorld, alpha);

  // Зарница мягко заливает всё стекло (рассеяние в самой воде/конденсате).
  world *= 1.0 + u_lightning * 0.25;

  // ── Финальный грейд ────────────────────────────────────────────────
  // Лёгкая симметричная кино-виньетка: держит читаемость стеклянных панелей.
  vec2 vigPos = (sUv - vec2(0.5, 0.5)) * vec2(1.06, 1.0);
  world *= 1.0 - smoothstep(0.52, 1.02, length(vigPos)) * 0.20;
  // Ночной тонинг: едва заметный холодный сдвиг — тёплый фонарь клипа
  // должен остаться янтарным, не глушим его синевой.
  float luma = dot(world, vec3(0.299, 0.587, 0.114));
  world = mix(world, luma * vec3(0.88, 0.98, 1.12), 0.06);
  vec3 srgb = toSrgb(world);
  // Живое плёночное зерно: рвёт бандинг тёмных градиентов, склеивает слои.
  float grain = hash21(gl_FragCoord.xy + fract(u_time * 7.31) * 191.0) - 0.5;
  srgb += grain * 0.009;
  outColor = vec4(srgb, 1.0);
}
`;
