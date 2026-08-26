export const YANI_FULLSCREEN_VERTEX_SHADER = `#version 300 es
layout(location = 0) in vec2 a_position;
out vec2 v_uv;
void main() {
  v_uv = a_position * 0.5 + 0.5;
  gl_Position = vec4(a_position, 0.0, 1.0);
}`;

export const YANI_SMOKE_FRAGMENT_SHADER = `#version 300 es
precision highp float;
in vec2 v_uv;
out vec4 out_color;

uniform sampler2D u_previous;
uniform vec2 u_resolution;
uniform vec4 u_pointer;
uniform float u_time;
uniform float u_dt;
uniform float u_mood;
uniform float u_reset;
uniform int u_octaves;

float hash21(vec2 p) {
  p = fract(p * vec2(123.34, 456.21));
  p += dot(p, p + 45.32);
  return fract(p.x * p.y);
}

float noise21(vec2 p) {
  vec2 i = floor(p);
  vec2 f = fract(p);
  f = f * f * (3.0 - 2.0 * f);
  return mix(mix(hash21(i), hash21(i + vec2(1, 0)), f.x),
             mix(hash21(i + vec2(0, 1)), hash21(i + vec2(1)), f.x), f.y);
}

float fbm(vec2 p) {
  float value = 0.0;
  float amplitude = 0.5;
  for (int i = 0; i < 5; i++) {
    if (i >= u_octaves) break;
    value += noise21(p) * amplitude;
    p = mat2(1.55, 1.12, -1.12, 1.55) * p + 7.3;
    amplitude *= 0.5;
  }
  return value;
}

vec2 curlVelocity(vec2 uv) {
  float eps = 0.009;
  vec2 p = uv * vec2(4.2, 3.1) + vec2(u_time * 0.025, -u_time * 0.018);
  float dx = fbm(p + vec2(eps, 0.0)) - fbm(p - vec2(eps, 0.0));
  float dy = fbm(p + vec2(0.0, eps)) - fbm(p - vec2(0.0, eps));
  float restless = u_mood == 2.0 ? 1.8 : u_mood == 4.0 ? 2.2 : 1.0;
  return vec2(dy, -dx) * 0.17 * restless + vec2(0.0, 0.026 + uv.y * 0.014);
}

void main() {
  vec2 uv = v_uv;
  float dt = min(u_dt, 0.05);
  vec2 velocity = curlVelocity(uv);
  vec2 pointerDelta = uv - u_pointer.xy;
  float wake = exp(-dot(pointerDelta, pointerDelta) * 85.0);
  velocity += u_pointer.zw * wake * 0.035;
  vec2 previousUv = clamp(uv - velocity * dt, vec2(0.002), vec2(0.998));
  vec4 previous = texture(u_previous, previousUv);

  float decay = exp(-dt * (u_mood == 4.0 ? 0.72 : 0.18));
  float density = previous.r * decay;
  float heat = previous.g * exp(-dt * 0.42);
  float age = min(1.0, previous.b + dt * 0.08);

  vec2 source = vec2(0.315, 0.145);
  float sway = sin(u_time * 0.7) * 0.018 + sin(u_time * 0.19) * 0.012;
  source.x += sway;
  vec2 sourceDelta = (uv - source) * vec2(1.0, 1.7);
  float injection = exp(-dot(sourceDelta, sourceDelta) * 950.0);
  float drag = u_mood == 1.0 ? 2.0 : u_mood == 3.0 ? 1.45 : u_mood == 2.0 ? 1.2 : 0.72;
  float alarmGate = u_mood == 4.0 ? 0.12 : 1.0;
  density += injection * dt * 2.8 * drag * alarmGate;
  heat += injection * dt * 2.2 * (0.5 + drag * 0.5) * alarmGate;
  age = mix(age, 0.0, clamp(injection * dt * 4.0, 0.0, 1.0));

  float ashSource = exp(-dot((uv - vec2(0.35, 0.13)) * vec2(1.0, 2.2),
                            (uv - vec2(0.35, 0.13)) * vec2(1.0, 2.2)) * 520.0);
  density += ashSource * dt * (u_mood == 2.0 ? 0.9 : 0.35);

  if (u_reset > 0.5) {
    density = injection * 0.17 * alarmGate;
    heat = injection * 0.3 * alarmGate;
    age = 0.0;
  }
  out_color = vec4(clamp(density, 0.0, 1.0), clamp(heat, 0.0, 1.0), age, 1.0);
}`;

export const YANI_WORLD_FRAGMENT_SHADER = `#version 300 es
precision highp float;
in vec2 v_uv;
out vec4 out_color;

uniform vec2 u_resolution;
uniform vec2 u_scene_shift;
uniform float u_time;
uniform float u_mood;
uniform int u_dust_count;

float hash21(vec2 p) {
  p = fract(p * vec2(123.34, 345.45));
  p += dot(p, p + 34.345);
  return fract(p.x * p.y);
}

float boxMask(vec2 p, vec2 center, vec2 halfSize, float feather) {
  vec2 d = abs(p - center) - halfSize;
  return 1.0 - smoothstep(0.0, feather, max(d.x, d.y));
}

float ellipseMask(vec2 p, vec2 center, vec2 radius, float feather) {
  float d = length((p - center) / radius) - 1.0;
  return 1.0 - smoothstep(0.0, feather, d);
}

void main() {
  vec2 uv = v_uv;
  vec2 p = uv + u_scene_shift;
  float warm = u_mood == 3.0 ? 1.0 : u_mood == 1.0 ? 0.7 : 0.0;
  float alarm = u_mood == 4.0 ? 1.0 : 0.0;
  float scan = u_mood == 2.0 ? 1.0 : 0.0;

  vec3 tobacco = mix(vec3(0.025, 0.022, 0.019), vec3(0.075, 0.054, 0.037), pow(1.0 - uv.y, 1.4));
  float wallStain = hash21(floor(p * vec2(18.0, 12.0))) * 0.018;
  vec3 color = tobacco + vec3(wallStain * 0.6, wallStain * 0.42, wallStain * 0.25);

  // Window and blinds: the room stays empty; Yaniko only exists in the core.
  float window = boxMask(p, vec2(0.835, 0.72), vec2(0.145, 0.27), 0.006);
  vec3 moon = mix(vec3(0.12, 0.18, 0.17), vec3(0.43, 0.62, 0.54), uv.y);
  color = mix(color, moon, window * (0.48 + warm * 0.08));
  float frameV = boxMask(p, vec2(0.835, 0.72), vec2(0.012, 0.28), 0.003);
  float frameH = boxMask(p, vec2(0.835, 0.72), vec2(0.15, 0.012), 0.003);
  color = mix(color, vec3(0.045, 0.052, 0.045), max(frameV, frameH));
  float blindLine = 1.0 - smoothstep(0.0, 0.022, abs(fract((p.y - 0.45) * 18.0) - 0.5));
  color = mix(color, vec3(0.12, 0.16, 0.145), window * blindLine * 0.44);

  // Volumetric moon shaft from the window toward the ashtray.
  vec2 beamA = vec2(0.92, 0.98);
  vec2 beamB = vec2(0.42, 0.08);
  vec2 ba = beamB - beamA;
  float beamT = clamp(dot(p - beamA, ba) / dot(ba, ba), 0.0, 1.0);
  float beamD = length((p - beamA) - ba * beamT);
  float beam = (1.0 - smoothstep(0.0, 0.19 * beamT + 0.018, beamD))
             * (1.0 - smoothstep(0.03, 1.0, beamT));
  color += vec3(0.22, 0.38, 0.32) * beam * (0.12 + warm * 0.035);

  // Desk and its worn front edge.
  float desk = 1.0 - smoothstep(0.235, 0.255, p.y);
  vec3 deskColor = mix(vec3(0.06, 0.042, 0.028), vec3(0.135, 0.085, 0.045), 1.0 - p.y / 0.26);
  float grain = sin(p.x * 92.0 + sin(p.x * 19.0) * 2.0) * 0.009;
  color = mix(color, deskColor + grain, desk);
  color += vec3(0.18, 0.105, 0.04) * exp(-abs(p.y - 0.238) * 260.0) * 0.22;

  // Ashtray, cigarette pack, can and a crumpled cloth: controlled clutter.
  float trayOuter = ellipseMask(p, vec2(0.315, 0.145), vec2(0.105, 0.038), 0.08);
  float trayInner = ellipseMask(p, vec2(0.315, 0.151), vec2(0.076, 0.022), 0.1);
  color = mix(color, vec3(0.19, 0.19, 0.17), trayOuter * 0.88);
  color = mix(color, vec3(0.04, 0.036, 0.031), trayInner * 0.9);
  float pack = boxMask(p, vec2(0.585, 0.151), vec2(0.058, 0.075), 0.006);
  color = mix(color, vec3(0.64, 0.72, 0.65), pack * 0.72);
  float packBand = boxMask(p, vec2(0.585, 0.18), vec2(0.06, 0.009), 0.002);
  color = mix(color, vec3(0.12, 0.21, 0.37), packBand * 0.9);
  float can = boxMask(p, vec2(0.76, 0.15), vec2(0.033, 0.105), 0.01);
  float canLight = 1.0 - smoothstep(0.0, 0.034, abs(p.x - 0.75));
  color = mix(color, vec3(0.13, 0.16, 0.14) + canLight * vec3(0.12, 0.16, 0.14), can * 0.72);
  float cloth = ellipseMask(p, vec2(0.46, 0.085), vec2(0.15, 0.055), 0.25);
  color = mix(color, vec3(0.11, 0.13, 0.115) + sin(p.x * 58.0) * 0.012, cloth * 0.45);

  // Ember and a few deterministic ash points in the tray.
  float heartbeat = exp(-pow(fract(u_time * 0.22) - 0.12, 2.0) / 0.004)
                  + 0.55 * exp(-pow(fract(u_time * 0.22) - 0.3, 2.0) / 0.006);
  float ember = exp(-dot((p - vec2(0.315, 0.158)) * vec2(1.0, 1.8),
                         (p - vec2(0.315, 0.158)) * vec2(1.0, 1.8)) * 2400.0);
  float emberGain = (0.52 + warm * 0.8 + heartbeat * 0.4) * (1.0 - alarm * 0.85);
  color += ember * emberGain * vec3(1.0, 0.31, 0.055);

  vec2 dustGrid = floor(p * vec2(42.0, 28.0));
  float dustSeed = hash21(dustGrid);
  float dustLimit = float(u_dust_count) / 110.0;
  vec2 dustUv = fract(p * vec2(42.0, 28.0)) - 0.5;
  dustUv.x += sin(u_time * (0.1 + dustSeed * 0.18) + dustSeed * 9.0) * 0.22;
  float dust = (1.0 - smoothstep(0.0, 0.09, length(dustUv)))
             * step(1.0 - dustLimit, dustSeed) * beam;
  color += dust * vec3(0.56, 0.72, 0.64) * (0.18 + scan * 0.22);

  float fluorescent = alarm * (0.55 + 0.45 * step(0.4, hash21(vec2(floor(u_time * 12.0), 7.0))));
  color = mix(color, color * vec3(0.56, 0.82, 0.67), fluorescent * 0.18);
  out_color = vec4(max(color, vec3(0.0)), 1.0);
}`;

export const YANI_COMPOSITE_FRAGMENT_SHADER = `#version 300 es
precision highp float;
in vec2 v_uv;
out vec4 out_color;

uniform sampler2D u_world;
uniform sampler2D u_smoke;
uniform vec2 u_resolution;
uniform vec2 u_smoke_resolution;
uniform float u_time;
uniform float u_mood;
uniform int u_panel_count;
uniform vec4 u_panels[12];
uniform float u_panel_radii[12];

float hash21(vec2 p) {
  p = fract(p * vec2(123.34, 345.45));
  p += dot(p, p + 34.345);
  return fract(p.x * p.y);
}

float roundedPanel(vec2 uv, vec4 panel, float radius) {
  vec2 halfSize = panel.zw * 0.5;
  vec2 center = panel.xy + halfSize;
  vec2 q = abs(uv - center) - halfSize + radius;
  float distance = min(max(q.x, q.y), 0.0) + length(max(q, 0.0)) - radius;
  return 1.0 - smoothstep(0.0, 1.5 / min(u_resolution.x, u_resolution.y), distance);
}

void main() {
  vec2 uv = v_uv;
  vec2 lensWarp = vec2(0.0);
  float lensMask = 0.0;
  float lensEdge = 0.0;
  for (int index = 0; index < 12; index++) {
    if (index >= u_panel_count) break;
    vec4 panel = u_panels[index];
    float mask = roundedPanel(uv, panel, u_panel_radii[index]);
    vec2 center = panel.xy + panel.zw * 0.5;
    vec2 local = (uv - center) / max(panel.zw, vec2(0.001));
    float edge = smoothstep(0.42, 0.5, max(abs(local.x), abs(local.y))) * mask;
    lensWarp += normalize(local + vec2(0.0001)) * (0.0012 + 0.0018 * edge) * mask;
    lensMask = max(lensMask, mask);
    lensEdge = max(lensEdge, edge);
  }

  vec2 worldUv = clamp(uv + lensWarp, vec2(0.001), vec2(0.999));
  vec3 world = texture(u_world, worldUv).rgb;
  vec2 px = 1.0 / u_resolution;
  vec3 bloom = texture(u_world, worldUv + vec2(px.x * 3.0, 0)).rgb
             + texture(u_world, worldUv - vec2(px.x * 3.0, 0)).rgb
             + texture(u_world, worldUv + vec2(0, px.y * 3.0)).rgb
             + texture(u_world, worldUv - vec2(0, px.y * 3.0)).rgb;
  bloom *= 0.25;
  world += max(bloom - vec3(0.12), vec3(0.0)) * 0.22;

  vec2 smokeUv = clamp(uv + lensWarp * 1.8, vec2(0.002), vec2(0.998));
  vec4 smokeData = texture(u_smoke, smokeUv);
  float density = smoothstep(0.015, 0.72, smokeData.r);
  float heat = smokeData.g;
  vec3 coolSmoke = vec3(0.49, 0.57, 0.53);
  vec3 warmSmoke = vec3(0.68, 0.48, 0.31);
  vec3 smokeColor = mix(coolSmoke, warmSmoke, clamp(heat * 0.8, 0.0, 1.0));
  world = mix(world, smokeColor, density * (0.18 + lensMask * 0.09));
  world += density * heat * vec3(0.24, 0.085, 0.018) * 0.15;
  world += lensEdge * vec3(0.38, 0.54, 0.47) * 0.045;

  float vignette = 1.0 - smoothstep(0.22, 0.78, length((uv - 0.5) * vec2(0.82, 1.0)));
  world *= 0.62 + vignette * 0.42;
  float grain = hash21(gl_FragCoord.xy + fract(u_time) * 91.7) - 0.5;
  world += grain * (1.0 / 255.0) * 2.4;
  world = pow(max(world, vec3(0.0)), vec3(0.94));
  out_color = vec4(world, 1.0);
}`;
