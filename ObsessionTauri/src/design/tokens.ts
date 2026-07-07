// Aurora Glass — дизайн-токены. Единый источник цветов/движений для JS-логики
// (Tailwind-конфиг зеркалит эти же значения для классов).

export const colors = {
  base: "#0A0B10",
  accent: "#6366F1",
  accentCyan: "#22D3EE",
  accentViolet: "#8B5CF6",
  ok: "#34D399",
  warn: "#FBBF24",
  danger: "#F87171",
  ink: "#F4F5FB",
  inkSoft: "#B7BCD0",
  inkMuted: "#6B7189",
} as const;

// Пружинные пресеты Framer Motion для консистентного «премиум» ощущения.
export const spring = {
  soft: { type: "spring", stiffness: 260, damping: 26 },
  snappy: { type: "spring", stiffness: 400, damping: 30 },
  gentle: { type: "spring", stiffness: 140, damping: 20 },
} as const;

export type LogLevel = "info" | "success" | "warn" | "error";

export const levelColor: Record<LogLevel, string> = {
  info: colors.inkSoft,
  success: colors.ok,
  warn: colors.warn,
  error: colors.danger,
};
