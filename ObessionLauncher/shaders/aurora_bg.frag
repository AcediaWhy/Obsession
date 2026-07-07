#version 460 core

#include <flutter/runtime_effect.glsl>

uniform vec2 uResolution;
uniform float uTime;
uniform float uIntensity;
uniform vec3 uColor1;
uniform vec3 uColor2;
uniform vec3 uColor3;
uniform float uSpeed;

vec3 mod289(vec3 x) { return x - floor(x * (1.0 / 289.0)) * 289.0; }
vec2 mod289(vec2 x) { return x - floor(x * (1.0 / 289.0)) * 289.0; }
vec3 permute(vec3 x) { return mod289(((x*34.0)+1.0)*x); }

float snoise(vec2 v) {
  const vec4 C = vec4(0.211324865405187, 0.366025403784439, -0.577350269189626, 0.024390243902439);
  vec2 i = floor(v + dot(v, C.yy));
  vec2 x0 = v - i + dot(i, C.xx);
  vec2 i1 = (x0.x > x0.y) ? vec2(1.0, 0.0) : vec2(0.0, 1.0);
  vec4 x12 = x0.xyxy + C.xxzz;
  x12.xy -= i1;
  i = mod289(i);
  vec3 p = permute(permute(i.y + vec3(0.0, i1.y, 1.0)) + i.x + vec3(0.0, i1.x, 1.0));
  vec3 m = max(0.5 - vec3(dot(x0, x0), dot(x12.xy, x12.xy), dot(x12.zw, x12.zw)), 0.0);
  m = m*m; m = m*m;
  vec3 x = 2.0 * fract(p * C.www) - 1.0;
  vec3 h = abs(x) - 0.5;
  vec3 ox = floor(x + 0.5);
  vec3 a0 = x - ox;
  m *= 1.79284291400159 - 0.85373472095314 * (a0*a0 + h*h);
  vec3 g;
  g.x = a0.x * x0.x + h.x * x12.x;
  g.yz = a0.yz * x12.xw + h.yz * x12.zw;
  return 130.0 * dot(m, g);
}

out vec4 fragColor;

void main() {
  vec2 uv = FlutterFragCoord().xy / uResolution.xy;
  vec2 p = uv * 3.0;
  p.x += uTime * 0.1 * uSpeed;

  float n1 = snoise(p + vec2(uTime * 0.3 * uSpeed, 0.0));
  float n2 = snoise(p * 2.0 + vec2(uTime * 0.5 * uSpeed, uTime * 0.2 * uSpeed));
  float n3 = snoise(p * 4.0 + vec2(0.0, uTime * 0.4 * uSpeed));

  float aurora = n1 * 0.5 + n2 * 0.3 + n3 * 0.2;
  aurora = smoothstep(-0.2, 0.8, aurora);

  vec3 col = mix(uColor1, uColor2, uv.y);
  col = mix(col, uColor3, aurora);

  float vig = 1.0 - length(uv - 0.5) * 0.8;
  col *= vig;
  col *= uIntensity;

  fragColor = vec4(col, 1.0);
}