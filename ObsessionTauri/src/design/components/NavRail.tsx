import { motion } from "framer-motion";
import { Icon } from "./icons";

export type Tab = "dpi" | "ai" | "telegram" | "lists" | "profiles" | "settings";

const items: { id: Tab; label: string; icon: (p: { size?: number }) => JSX.Element; soon?: boolean }[] = [
  { id: "dpi", label: "DPI-обход", icon: Icon.Bolt },
  { id: "ai", label: "ИИ-разблокировка", icon: Icon.Robot },
  { id: "telegram", label: "Telegram", icon: Icon.Send },
  { id: "lists", label: "Списки", icon: Icon.List },
  { id: "profiles", label: "Профили", icon: Icon.Layers },
  { id: "settings", label: "Настройки", icon: Icon.Settings },
];

export function NavRail({
  active,
  onSelect,
}: {
  active: Tab;
  onSelect: (t: Tab) => void;
}) {
  return (
    <nav className="flex w-[220px] flex-col px-4 pb-4 pt-2">
      {/* Лого. */}
      <div className="mb-8 flex items-center gap-3 px-2">
        <div className="flex h-10 w-10 items-center justify-center rounded-xl bg-gradient-to-br from-accent to-accent-violet shadow-glow">
          <Icon.Bolt size={20} />
        </div>
        <div className="leading-tight">
          <div className="font-display text-lg font-bold text-ink">Obsession</div>
          <div className="text-[11px] tracking-widest text-ink-muted">V1.0.0</div>
        </div>
      </div>

      <div className="mb-2 px-2 text-[11px] font-semibold uppercase tracking-[0.14em] text-ink-muted">
        Меню
      </div>

      <div className="flex flex-col gap-1">
        {items.map((it) => {
          const isActive = active === it.id;
          return (
            <button
              key={it.id}
              onClick={() => onSelect(it.id)}
              className="no-drag relative flex items-center gap-3 rounded-xl px-3 py-2.5 text-sm transition-colors"
            >
              {isActive && (
                <motion.div
                  layoutId="nav-active"
                  className="absolute inset-0 rounded-xl border border-accent/40 bg-accent/15 shadow-glow"
                  transition={{ type: "spring", stiffness: 380, damping: 32 }}
                />
              )}
              <it.icon size={18} />
              <span
                className={[
                  "relative z-10 whitespace-nowrap font-medium",
                  isActive ? "text-ink" : "text-ink-soft",
                ].join(" ")}
              >
                {it.label}
              </span>
              {it.soon && (
                <span className="relative z-10 ml-auto rounded-md bg-white/8 px-1.5 py-0.5 text-[9px] font-semibold uppercase text-ink-muted">
                  soon
                </span>
              )}
            </button>
          );
        })}
      </div>

      <div className="mt-auto px-2 text-[11px] text-ink-muted">made by VlarpSu</div>
    </nav>
  );
}
