import type { Config } from "tailwindcss";

// Aurora Glass — тёмный glassmorphism. База #0A0B10, индиго→циан акценты.
export default {
  content: ["./index.html", "./src/**/*.{ts,tsx}"],
  theme: {
    extend: {
      colors: {
        base: {
          DEFAULT: "#05060B", // deep space
          800: "#0B0D16",
          700: "#121526",
        },
        // Акцентная палитра завязана на CSS-переменные — переключается вместе
        // с темой (Aurora: холодный индиго/циан, Ophanim: тёплое золото/магента).
        // Значения задаются в globals.css (:root и [data-theme=...]).
        accent: {
          DEFAULT: "rgb(var(--accent) / <alpha-value>)",
          cyan: "rgb(var(--accent-cyan) / <alpha-value>)",
          violet: "rgb(var(--accent-violet) / <alpha-value>)",
          magenta: "#F0ABFC", // разогрев горизонта (тёплый, общий)
          hot: "#FDE68A", // горячий ободок (тёплый, общий)
        },
        glass: {
          DEFAULT: "rgba(255,255,255,0.05)",
          strong: "rgba(255,255,255,0.08)",
          border: "rgba(255,255,255,0.10)",
        },
        ink: {
          DEFAULT: "#F4F5FB",
          soft: "#B7BCD0",
          muted: "#6B7189",
        },
        ok: "#34D399",
        warn: "#FBBF24",
        danger: "#F87171",
      },
      // Локальные шрифты с кириллицей (см. src/styles/fonts.css). Единая семья
      // IBM Plex: Plex Sans — заголовки/логотип (display) + тело (sans);
      // Plex Mono — метрики/логи/код, со slashed-zero (0↔O) через feature-settings.
      fontFamily: {
        sans: ['"IBM Plex Sans"', "system-ui", "sans-serif"],
        display: ['"IBM Plex Sans"', "sans-serif"],
        mono: ['"IBM Plex Mono", ui-monospace, monospace', { fontFeatureSettings: '"zero"' }],
      },
      // Типо-шкала: добавляем микроразмеры (убираем россыпь text-[9..11px]) и
      // подтягиваем h1 (плотнее интерлиньяж + отрицательный трекинг). Дефолтные
      // xs/sm/base НЕ трогаем — иначе поедет вертикальный ритм всего UI.
      fontSize: {
        "3xs": ["0.625rem", { lineHeight: "0.875rem", letterSpacing: "0.02em" }],
        "2xs": ["0.6875rem", { lineHeight: "1rem", letterSpacing: "0.01em" }],
        "3xl": ["1.875rem", { lineHeight: "2.15rem", letterSpacing: "-0.02em" }],
      },
      borderRadius: {
        xl2: "1.25rem",
        xl3: "1.75rem",
      },
      boxShadow: {
        glow: "0 0 24px -4px rgb(var(--accent) / 0.55)",
        "glow-lg": "0 0 48px -6px rgb(var(--accent) / 0.65)",
        "glow-cyan": "0 0 32px -4px rgb(var(--accent-cyan) / 0.55)",
        // Красное свечение danger-кнопок: язык глубины (glow у primary/active)
        // не должен рваться на единственном «плоском» варианте.
        "glow-danger": "0 0 24px -4px rgba(248,113,113,0.45)",
        glass: "0 8px 32px -8px rgba(0,0,0,0.6), inset 0 1px 0 0 rgba(255,255,255,0.06)",
      },
      backdropBlur: {
        xs: "2px",
      },
      keyframes: {
        "pulse-glow": {
          "0%, 100%": { opacity: "0.55" },
          "50%": { opacity: "1" },
        },
        drift: {
          "0%": { transform: "translate(0,0) scale(1)" },
          "50%": { transform: "translate(4%,-3%) scale(1.08)" },
          "100%": { transform: "translate(0,0) scale(1)" },
        },
        spin360: {
          from: { transform: "rotate(0deg)" },
          to: { transform: "rotate(360deg)" },
        },
        "spin360-rev": {
          from: { transform: "rotate(360deg)" },
          to: { transform: "rotate(0deg)" },
        },
      },
      animation: {
        "pulse-glow": "pulse-glow 2.6s ease-in-out infinite",
        "spin-slow": "spin360 24s linear infinite",
        "spin-med": "spin360 12s linear infinite",
        "spin-rev": "spin360-rev 18s linear infinite",
      },
    },
  },
  plugins: [],
} satisfies Config;
