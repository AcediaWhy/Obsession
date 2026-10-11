export const auroraVertex = `
attribute vec2 position;
void main() { gl_Position = vec4(position, 0.0, 1.0); }
`;

// Each veil has a curved hem, a changing direction and advected fine structure.
export const auroraFragment = `
precision highp float;
uniform vec2 resolution;
uniform vec2 displaySize;
uniform float time;
uniform float activity;
uniform float activationAge;
uniform vec3 pointer;
uniform sampler2D noiseMap;

float noise(vec2 p) {
  vec2 cell = floor(p);
  vec2 f = fract(p);
  f = f * f * (3.0 - 2.0 * f);
  return texture2D(noiseMap, (cell + f + 0.5) / 256.0).r;
}
float fbm(vec2 p) {
  return noise(p) * 0.57 + noise(p * 2.03 + 13.2) * 0.28
       + noise(p * 4.09 + 47.1) * 0.15;
}
float hash(vec2 p) {
  vec3 q = fract(vec3(p.xyx) * vec3(0.1031, 0.1030, 0.0973));
  q += dot(q, q.yzx + 33.33);
  return fract((q.x + q.y) * q.z);
}
vec3 stars(vec2 p) {
  vec2 grid = p * 165.0;
  vec2 id = floor(grid);
  vec2 local = fract(grid) - vec2(hash(id + 5.2), hash(id + 13.7));
  float seed = hash(id);
  float radius = mix(0.055, 0.16, pow(seed, 18.0));
  float point = exp(-dot(local, local) / (radius * radius));
  float bright = step(0.994, seed) * (0.22 + 0.45 * hash(id + 47.0));
  float halo = exp(-length(local) * 10.0) * step(0.9995, seed) * 0.09;
  float twinkle = 0.90 + 0.10 * sin(time * 0.32 + seed * 173.0);
  return mix(vec3(0.48, 0.65, 1.0), vec3(0.84, 0.90, 1.0), hash(id + 73.0))
       * (point * bright + halo) * twinkle;
}

vec3 veil(vec2 p, float layer, float phase, vec2 cursor) {
  float x = p.x;
  float drift = sin(phase * 0.61 + layer * 2.3) * 0.028;
  float bend = 0.14 * sin(x * 3.65 - phase * 0.55 + layer * 1.7);
  bend += 0.033 * sin(x * 9.0 + phase * 0.72 + layer * 2.0);
  bend += (fbm(vec2(x * 3.2 - phase * 0.27, layer * 9.0 + phase * 0.14)) - 0.5) * 0.08;
  float hem = -0.13 + x * 0.17 + bend + drift;
  hem += 0.25 * smoothstep(0.30, 0.90, x);
  hem += layer * 0.18;
  hem -= 0.075 * exp(-pow((x - 0.30 - sin(phase * 0.43) * 0.12) / 0.20, 2.0)) * (1.0 - layer * 0.3);
  float reach = exp(-pow((x - cursor.x) / 0.30, 2.0));
  float proximity = exp(-pow((cursor.y - hem - 0.08) / 0.30, 2.0));
  float influence = reach * proximity * pointer.z;
  // Mouse input lifts a nearby fold instead of adding an unrelated spotlight.
  hem += (cursor.y - hem) * influence * 0.38;
  hem += sin((x - cursor.x) * 9.0) * influence * 0.020;
  float height = p.y - hem;
  float lean = 0.16 * sin(x * 2.7 + phase * 0.4 + layer);
  float strandX = x + height * lean
                + height * height * 0.35 * sin(x * 3.0 + layer)
                + (fbm(vec2(x * 4.0, height * 3.0 + phase * 0.16)) - 0.5) * 0.035;
  float folds = fbm(vec2(strandX * 15.0 - phase * 0.48, layer * 21.0 + phase * 0.08));
  float fibres = noise(vec2(strandX * 155.0 - phase * 1.4, height * 3.0 + layer * 31.0));
  float lengthVariation = fbm(vec2(x * 5.0 + phase * 0.12, layer * 13.0 + 7.0));
  float falloff = 0.09 + lengthVariation * 0.20;
  float upper = exp(-pow(max(height, 0.0) / falloff, 1.35));
  float lower = smoothstep(-0.020, 0.032, height);
  float softHem = exp(-pow(height / 0.023, 2.0));
  float haze = exp(-pow(height / 0.14, 2.0));
  float structure = (0.23 + pow(folds, 1.3) * 1.05) * (0.70 + fibres * 0.30);
  float wisp = 0.64 + fbm(vec2(strandX * 8.0 - phase * 0.1, height * 12.0 - phase * 0.24)) * 0.45;
  float envelope = 0.56 + 0.44 * sin(x * 2.6 - phase * 0.45 + layer * 1.1);
  float tail = smoothstep(-1.18, -0.85, x) * (1.0 - smoothstep(0.92, 1.25, x));
  vec3 mint = vec3(0.22, 0.92, 0.69);
  vec3 cyan = vec3(0.07, 0.50, 0.70);
  vec3 violet = vec3(0.31, 0.19, 0.66);
  vec3 colour = mix(mint, cyan, smoothstep(0.015, 0.18, height));
  colour = mix(colour, violet, smoothstep(0.12, 0.38, height) * 0.83);
  colour = mix(colour, vec3(0.35, 0.25, 0.78), layer * 0.78);
  float wave = exp(-pow((x - (-1.4 + activationAge * 0.22)) / 0.26, 2.0))
             * (1.0 - smoothstep(8.0, 12.0, activationAge)) * activity;
  float energy = (upper * lower * wisp + softHem * 0.34) * structure;
  vec3 emission = colour * energy + mix(cyan, violet, layer * 0.45) * haze * 0.035;
  return emission * envelope * tail * mix(1.35, 0.62, layer)
       * (1.0 + influence * 0.55 + activity * 0.14 + wave * 0.28);
}

void main() {
  vec2 uv = gl_FragCoord.xy / resolution;
  float aspect = displaySize.x / displaySize.y;
  vec2 p = (uv - 0.5) * vec2(aspect, 1.0);
  vec2 cursor = (pointer.xy - 0.5) * vec2(aspect, 1.0);
  vec2 skyP = p + cursor * pointer.z * 0.008;
  p += cursor * pointer.z * 0.012;
  float phase = time * 0.080;
  float cloud = fbm(skyP * vec2(2.6, 3.0) + vec2(7.0, 12.0));
  vec3 sky = mix(vec3(0.005, 0.008, 0.028), vec3(0.014, 0.019, 0.056), uv.y);
  vec2 cloudP = (skyP - vec2(-0.58, 0.20)) * vec2(1.3, 2.0);
  sky += vec3(0.031, 0.014, 0.066) * exp(-dot(cloudP, cloudP)) * cloud;
  vec3 light = veil(p, 1.0, phase, cursor) + veil(p, 0.0, phase, cursor);
  light *= 1.0 - 0.20 * exp(-dot(p * vec2(1.2, 2.0), p * vec2(1.2, 2.0)));
  vec3 colour = sky + light + stars(skyP) / (1.0 + dot(light, vec3(1.4)));
  colour = 1.0 - exp(-colour * 1.15);
  colour = pow(max(colour, 0.0), vec3(0.82));
  colour *= 1.0 - 0.25 * smoothstep(0.30, 1.0, length(p * vec2(0.7, 1.0)));
  colour += (hash(gl_FragCoord.xy) - 0.5) / 255.0;
  gl_FragColor = vec4(colour, 1.0);
}
`;
