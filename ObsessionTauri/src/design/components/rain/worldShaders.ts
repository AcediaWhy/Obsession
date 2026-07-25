// Проход A (видео-мир). Кадр петли vibe (тёмный туманный хвойный лес)
// кладётся в FBO cover-fit'ом: запас ширины кадра уходит на мягкий параллакс
// от указателя. Цвет переводится в линейный свет (обратно в sRGB конвертирует
// композит) и почти не тонируется — читается родной teal-green клипа; зарницы
// добавляются здесь, чтобы мипы разнесли вспышку и по размытым зонам. Поверх
// FBO строятся мипы — композит берёт из них переменный блюр (конденсат/фокус).

export const rainPassVert = /* glsl */ `#version 300 es
layout(location = 0) in vec2 a_position;
out vec2 v_uv;
void main() {
  v_uv = a_position * 0.5 + 0.5;
  gl_Position = vec4(a_position, 0.0, 1.0);
}
`;

export const worldVideoFrag = /* glsl */ `#version 300 es
precision highp float;

uniform sampler2D u_video;
uniform vec2 u_resolution;
uniform float u_videoAspect;
uniform vec2 u_parallax;
uniform float u_lightning;

in vec2 v_uv;
out vec4 outColor;

void main() {
  // Экранные координаты (y=0 сверху): пишем в FBO перевёрнуто, композит
  // сэмплит по сырому v_uv — двойной флип взаимно сокращается. Кадр видео
  // загружен «канвасным» порядком (v=0 — верх), поэтому сэмплится по sUv.
  vec2 sUv = vec2(v_uv.x, 1.0 - v_uv.y);

  // Мягкий параллакс от указателя. Ходим по запасу ширины кадра.
  vec2 parallaxUv = u_parallax / max(u_resolution, vec2(1.0)) * 3.0;

  float fboAspect = u_resolution.x / max(1.0, u_resolution.y);
  float spanX = min(1.0, fboAspect / max(0.01, u_videoAspect));
  float spanY = min(1.0, u_videoAspect / max(0.01, fboAspect));
  vec2 vUv = vec2(
    0.5 + (sUv.x - 0.5 + parallaxUv.x * 0.6) * spanX,
    0.5 + (sUv.y - 0.5 + parallaxUv.y * 0.4) * spanY
  );
  vUv = clamp(vUv, vec2(0.001), vec2(0.999));

  vec3 col = texture(u_video, vUv).rgb;
  // sRGB видео → линейный свет. Грейд уже вшит при кодировании, рантайм почти
  // нейтрален — лёгкий подъём держит линзы капель читаемыми на тёмном клипе.
  col = pow(max(col, vec3(0.0)), vec3(2.2)) * 1.1;
  // Едва заметный холодный тон — не глушим родной teal-green клипа.
  col *= vec3(0.97, 0.99, 1.04);
  // Зарница за облаками: сильнее у верха кадра.
  col += vec3(0.12, 0.15, 0.20) * u_lightning * (1.0 - sUv.y * 0.55);

  outColor = vec4(col, 1.0);
}
`;
