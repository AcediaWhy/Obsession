// Фрагментный шейдер depth-parallax («живые обои» из фото + карты глубины).
// Смещаем выборку фото по карте глубины в зависимости от параллакса (курсор +
// лёгкий idle-дрейф): близкие пиксели (depth→1) едут сильнее дальних → из одной
// плоской картинки рождается объём. Вершинный шейдер берём готовый из rain
// (simpleVert) — здесь только фрагмент.
export const depthFrag = /* glsl */ `
precision mediump float;

uniform sampler2D u_photo;
uniform sampler2D u_depth;
uniform vec2 u_resolution;
uniform vec2 u_parallax;      // -1..1 позиция курсора (сглаженная)
uniform float u_scale;        // сила параллакса в пикселях
uniform float u_focus;        // фокальная глубина 0..1 (не двигается)
uniform float u_invert;       // 1.0 — инвертировать карту (near/far перепутаны)
uniform float u_textureRatio; // соотношение сторон фото (w/h)
uniform float u_time;

// Глубина в нашей конвенции: ближнее = 1 (светлое). u_invert исправляет карты,
// где ближнее = тёмное.
float sampleDepth(vec2 c) {
  float d = texture2D(u_depth, c).r;
  return mix(d, 1.0 - d, u_invert);
}

// UV с cover-fit (фото заполняет экран без искажения, как scaledTexCoord в rain).
vec2 coverUV() {
  vec2 uv = vec2(gl_FragCoord.x, u_resolution.y - gl_FragCoord.y) / u_resolution;
  float ratio = u_resolution.x / u_resolution.y;
  vec2 scale = vec2(1.0);
  vec2 offset = vec2(0.0);
  float d = ratio - u_textureRatio;
  if (d >= 0.0) {
    scale.y = 1.0 + d;
    offset.y = d / 2.0;
  } else {
    scale.x = 1.0 - d;
    offset.x = -d / 2.0;
  }
  return (uv + offset) / scale;
}

void main() {
  vec2 uv = coverUV();
  vec2 px = 1.0 / u_resolution;

  // Курсор + мягкий автономный дрейф (сцена «дышит» даже без мыши).
  vec2 par = u_parallax + vec2(sin(u_time * 0.3), cos(u_time * 0.23)) * 0.12;

  // Один уточняющий проход: сэмплим глубину, смещаемся, пересэмплим — заметно
  // меньше «размазывания» на резких перепадах глубины, чем одиночный тап.
  float depth = sampleDepth(uv);
  vec2 off = par * px * u_scale * (depth - u_focus);
  depth = sampleDepth(uv + off);
  off = par * px * u_scale * (depth - u_focus);

  vec3 col = texture2D(u_photo, uv + off).rgb;
  gl_FragColor = vec4(col, 1.0);
}
`;
