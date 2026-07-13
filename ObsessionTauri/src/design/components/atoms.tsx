import { motion } from "framer-motion";
import type { ReactNode } from "react";

import { spring } from "../tokens";

// ─── Button ───────────────────────────────────────────────────────────────

type BtnProps = {
  children: ReactNode;
  onClick?: () => void;
  variant?: "primary" | "ghost" | "danger";
  disabled?: boolean;
  className?: string;
};

export function Button({
  children,
  onClick,
  variant = "primary",
  disabled = false,
  className = "",
}: BtnProps) {
  const styles: Record<string, string> = {
    primary:
      "bg-accent/90 hover:bg-accent text-white shadow-glow hover:shadow-glow-lg",
    ghost:
      "bg-white/5 hover:bg-white/10 text-ink-soft border border-glass-border",
    danger: "bg-danger/85 hover:bg-danger text-white shadow-glow-danger",
  };
  return (
    <motion.button
      // Подъём на hover + прижатие на tap — пружиной flick (чётко, без желе).
      // y вместо scale: кнопки бывают широкими, масштаб на них заметно «дышит»
      // по краям, а вертикальный сдвиг читается как честный физический подъём.
      whileHover={disabled ? undefined : { y: -1 }}
      whileTap={disabled ? undefined : { y: 0, scale: 0.97 }}
      transition={spring.flick}
      onClick={onClick}
      disabled={disabled}
      className={[
        // btn-anim: цвета быстрые, glow расцветает 0.35s; transform не трогаем —
        // его ведёт framer (whileHover/Tap), CSS-транзишен поверх дал бы «резину».
        "no-drag btn-anim rounded-xl px-4 py-2.5 text-sm font-semibold focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent/70 disabled:cursor-not-allowed disabled:opacity-50",
        styles[variant],
        className,
      ].join(" ")}
    >
      {children}
    </motion.button>
  );
}

// ─── StatusBadge ────────────────────────────────────────────────────────────

export function StatusBadge({
  active,
  labelOn = "Активно",
  labelOff = "Выключено",
}: {
  active: boolean;
  labelOn?: string;
  labelOff?: string;
}) {
  // layout: в шапках Dpi/Telegram слева появляется Uptime-пилюля — бейдж
  // отъезжает пружиной, а не рывком. Цвет точки/текста и свечение доводятся
  // медленно (0.5s) — смена состояния «протекает», как glow у кнопок.
  return (
    <motion.div
      layout
      transition={spring.expand}
      className="flex items-center gap-2 rounded-full bg-white/5 px-3 py-1.5 text-xs font-medium"
    >
      <span
        className={[
          "h-2 w-2 rounded-full transition-[background-color,box-shadow] duration-500",
          active ? "bg-ok shadow-[0_0_10px_2px_rgba(52,211,153,0.7)]" : "bg-ink-muted",
        ].join(" ")}
      />
      <span className={`transition-colors duration-500 ${active ? "text-ok" : "text-ink-muted"}`}>
        {active ? labelOn : labelOff}
      </span>
    </motion.div>
  );
}

// ─── Chip (toggle) ──────────────────────────────────────────────────────────

export function Chip({
  label,
  active,
  disabled = false,
  onClick,
}: {
  label: string;
  active: boolean;
  disabled?: boolean;
  onClick: () => void;
}) {
  return (
    <button
      onClick={onClick}
      disabled={disabled}
      className={[
        "no-drag btn-anim rounded-xl px-3.5 py-2 text-sm font-medium focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent/70 disabled:opacity-40",
        active
          ? "bg-accent/20 text-ink border border-accent/50 shadow-glow"
          : "bg-white/5 text-ink-soft border border-glass-border hover:bg-white/10",
      ].join(" ")}
    >
      {label}
    </button>
  );
}

// ─── Select ─────────────────────────────────────────────────────────────────

export function Select({
  value,
  options,
  placeholder,
  onChange,
  disabled = false,
}: {
  value: string;
  options: string[];
  placeholder?: string;
  onChange: (v: string) => void;
  disabled?: boolean;
}) {
  return (
    <select
      value={value}
      disabled={disabled}
      onChange={(e) => onChange(e.target.value)}
      className="no-drag w-full rounded-xl border border-glass-border bg-base-800/80 px-3 py-2.5 text-sm text-ink outline-none transition-colors focus:border-accent/60 disabled:opacity-50"
    >
      {options.map((o) => (
        <option key={o} value={o} className="bg-base-800">
          {o || placeholder || "(по умолчанию)"}
        </option>
      ))}
    </select>
  );
}

// ─── TextField ──────────────────────────────────────────────────────────────

export function TextField({
  value,
  onChange,
  placeholder,
  type = "text",
}: {
  value: string;
  onChange: (v: string) => void;
  placeholder?: string;
  type?: string;
}) {
  return (
    <input
      type={type}
      value={value}
      placeholder={placeholder}
      onChange={(e) => onChange(e.target.value)}
      className="no-drag w-full rounded-xl border border-glass-border bg-base-800/80 px-3 py-2.5 text-sm text-ink outline-none transition-colors placeholder:text-ink-muted focus:border-accent/60"
    />
  );
}

// ─── SectionLabel ─────────────────────────────────────────────────────────────

export function SectionLabel({ children }: { children: ReactNode }) {
  return (
    <div className="mb-2 text-2xs font-semibold uppercase tracking-[0.14em] text-ink-muted">
      {children}
    </div>
  );
}

// ─── Switch (toggle) ──────────────────────────────────────────────────────────

export function Switch({
  checked,
  onChange,
  disabled = false,
}: {
  checked: boolean;
  onChange: (v: boolean) => void;
  disabled?: boolean;
}) {
  return (
    <button
      role="switch"
      aria-checked={checked}
      disabled={disabled}
      onClick={() => onChange(!checked)}
      className={[
        "no-drag relative h-6 w-11 shrink-0 rounded-full border transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent/70 disabled:opacity-40",
        checked
          ? "border-accent/50 bg-accent/30 shadow-glow"
          : "border-glass-border bg-white/5",
      ].join(" ")}
    >
      <motion.span
        layout
        transition={spring.flick}
        className={[
          "absolute rounded-full",
          checked
            ? "bg-gradient-to-br from-accent-cyan to-accent shadow-[0_0_10px_2px_rgba(99,102,241,0.7)]"
            : "bg-ink-muted",
        ].join(" ")}
        style={{ height: 18, width: 18, top: 2, left: checked ? 22 : 3 }}
      />
    </button>
  );
}

// ─── Row (лейбл + управляющий элемент) ────────────────────────────────────────

export function Row({
  label,
  hint,
  children,
}: {
  label: string;
  hint?: string;
  children: ReactNode;
}) {
  return (
    <div className="flex items-center justify-between gap-4 py-2.5">
      <div className="min-w-0">
        <div className="text-sm font-medium text-ink">{label}</div>
        {hint && <div className="mt-0.5 text-xs text-ink-muted">{hint}</div>}
      </div>
      <div className="shrink-0">{children}</div>
    </div>
  );
}
