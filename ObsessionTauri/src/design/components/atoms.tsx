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
      type="button"
      // На hover смещаем кнопку по Y, чтобы не масштабировать её ширину.
      whileHover={disabled ? undefined : { y: -1 }}
      whileTap={disabled ? undefined : { y: 0, scale: 0.97 }}
      transition={spring.flick}
      onClick={onClick}
      disabled={disabled}
      className={[
        // Transform управляется framer-motion; CSS-анимация отвечает за цвет и свечение.
        "no-drag theme-morph btn-anim rounded-xl px-4 py-2.5 text-sm font-semibold focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent/70 disabled:cursor-not-allowed disabled:opacity-50",
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
  // за base-такт — смена состояния «протекает», но не отстаёт от контрола.
  return (
    <motion.div
      layout
      transition={spring.expand}
      className="theme-morph flex items-center gap-2 rounded-full bg-white/5 px-3 py-1.5 text-xs font-medium"
    >
      <span
        className={[
          "h-2 w-2 rounded-full transition-[background-color,box-shadow] duration-[var(--motion-base)]",
          active ? "bg-ok shadow-[0_0_10px_2px_rgba(52,211,153,0.7)]" : "bg-ink-muted",
        ].join(" ")}
      />
      <span className={`transition-colors duration-[var(--motion-base)] ${active ? "text-ok" : "text-ink-muted"}`}>
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
  className = "",
  role,
  ariaChecked,
}: {
  label: string;
  active: boolean;
  disabled?: boolean;
  onClick: () => void;
  className?: string;
  // role/ariaChecked — чтобы chip мог служить радиокнопкой внутри radiogroup
  // (выбор режима), не отращивая второй визуальный язык выбора.
  role?: string;
  ariaChecked?: boolean;
}) {
  return (
    <motion.button
      type="button"
      role={role}
      aria-checked={ariaChecked}
      whileHover={disabled ? undefined : { y: -1 }}
      whileTap={disabled ? undefined : { y: 0, scale: 0.97 }}
      transition={spring.flick}
      onClick={onClick}
      disabled={disabled}
      className={[
        "no-drag theme-morph btn-anim rounded-xl px-3.5 py-2 text-sm font-medium focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent/70 disabled:opacity-40",
        active
          ? "bg-accent/20 text-ink border border-accent/50 shadow-glow"
          : "bg-white/5 text-ink-soft border border-glass-border hover:bg-white/10",
        className,
      ].join(" ")}
    >
      {label}
    </motion.button>
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
      className="no-drag theme-morph control-anim w-full rounded-xl border border-glass-border bg-base-800/80 px-3 py-2.5 text-sm text-ink outline-none focus:border-accent/60 disabled:opacity-50"
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
      className="no-drag theme-morph control-anim w-full rounded-xl border border-glass-border bg-base-800/80 px-3 py-2.5 text-sm text-ink outline-none placeholder:text-ink-muted focus:border-accent/60"
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
  ariaLabel,
}: {
  checked: boolean;
  onChange: (v: boolean) => void;
  disabled?: boolean;
  ariaLabel?: string;
}) {
  return (
    <motion.button
      type="button"
      role="switch"
      aria-checked={checked}
      aria-label={ariaLabel}
      whileHover={disabled ? undefined : { y: -1 }}
      whileTap={disabled ? undefined : { y: 0, scale: 0.94 }}
      transition={spring.flick}
      disabled={disabled}
      onClick={() => onChange(!checked)}
      className={[
        "no-drag theme-morph btn-anim relative h-6 w-11 shrink-0 rounded-full border focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent/70 disabled:opacity-40",
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
    </motion.button>
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
