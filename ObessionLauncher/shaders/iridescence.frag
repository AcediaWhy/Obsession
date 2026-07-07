#version 460 core

#include <flutter/runtime_effect.glsl>

// Iridescence / thin-film interference shader for Seraphim Desktop.
// Имитирует перламутровый перелив (nacre): интерференционные цвета тонкой
// плёнки, смещающиеся в зависимости от угла (через uMouse) и времени.
// Низкая насыщенность — пастельный перламутр, а не «бензиновое пятно».

uniform vec2 uResolution;
uniform float uTime;
uniform float uIntensity;      // 0..1 — сила перелива
uniform float uSpeed;          // скорость дрейфа
uniform vec3 uTint1;           // cloud white
uniform vec3 uTint2;           // blush pink
uniform vec3 uTint3;           // mist blue
uniform vec3 uTint4;           // dream lavender
uniform vec3 uTint5;           // mint glow
uniform vec2 uMouse;           // нормализованная позиция курсора 0..1
uniform float uHighlight;      // 0..1 — подсветка hover-эффекта

out vec4 fragColor;

const float PI = 3.14159265359;
const float TAU = 6.28318530718;

float hash(vec2 p) {
    return fract(sin(dot(p, vec2(127.1, 311.7))) * 43758.5453);
}

float noise(vec2 p) {
    vec2 i = floor(p);
    vec2 f = fract(p);
    f = f * f * (3.0 - 2.0 * f);
    float a = hash(i);
    float b = hash(i + vec2(1.0, 0.0));
    float c = hash(i + vec2(0.0, 1.0));
    float d = hash(i + vec2(1.0, 1.0));
    return mix(mix(a, b, f.x), mix(c, d, f.x), f.y);
}

float fbm(vec2 p) {
    float v = 0.0;
    float amp = 0.5;
    for (int i = 0; i < 4; i++) {
        v += amp * noise(p);
        p *= 2.0;
        amp *= 0.5;
    }
    return v;
}

void main() {
    vec2 uv = FlutterFragCoord().xy / uResolution.xy;

    // Направление «угла падения» — смещается курсором + медленным дрейфом.
    vec2 dir = uMouse - vec2(0.5);
    float angle = atan(dir.y, dir.x) + uTime * 0.05 * uSpeed;

    // Толщина плёнки варьируется по поверхности + лёгкий шум.
    float thickness = fbm(uv * 3.0 + vec2(uTime * 0.04 * uSpeed, 0.0));
    thickness = 0.5 + 0.5 * thickness;

    // Фаза интерференции — главный перелив.
    float phase = thickness * TAU * 2.0 + angle * 1.5 + uTime * 0.2 * uSpeed;

    // Пять пастельных тинт-остановок, смешанных по фазе (sweep).
    float s = fract(phase / TAU);
    vec3 col;
    if (s < 0.2) {
        col = mix(uTint1, uTint2, s / 0.2);
    } else if (s < 0.4) {
        col = mix(uTint2, uTint3, (s - 0.2) / 0.2);
    } else if (s < 0.6) {
        col = mix(uTint3, uTint4, (s - 0.4) / 0.2);
    } else if (s < 0.8) {
        col = mix(uTint4, uTint5, (s - 0.6) / 0.2);
    } else {
        col = mix(uTint5, uTint1, (s - 0.8) / 0.2);
    }

    // Мягкая полосатость перламутра (nacre striations) — тонкие сияющие линии.
    float striation = sin(uv.x * 18.0 + angle * 2.0 + uTime * 0.3 * uSpeed);
    striation = smoothstep(0.85, 1.0, striation);
    col += striation * 0.06 * uIntensity;

    // Бликовое свечение, следующее за курсором (soft sheen).
    float sheen = exp(-length(uv - uMouse) * 4.0);
    col += sheen * 0.12 * uIntensity;

    // Hover-подсветка усиливает перелив.
    col = mix(col, col * 1.08, uHighlight);

    // Понижаем насыщенность, чтобы остаться в пастели.
    float lum = dot(col, vec3(0.299, 0.587, 0.114));
    col = mix(col, vec3(lum), 0.35);

    // Базовая непрозрачность слоя — поверх белой панели.
    float alpha = 0.55 * uIntensity + sheen * 0.25 * uIntensity + uHighlight * 0.15;

    fragColor = vec4(col, clamp(alpha, 0.0, 1.0));
}
