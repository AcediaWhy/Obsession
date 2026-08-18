import { useLayoutEffect, useRef } from "react";
import { motion } from "framer-motion";
import { Icon } from "./icons";
import { EyeLogo } from "./EyeLogo";
import { useMotionOff } from "../render";
import { dur, ease, spring } from "../tokens";

export type Tab = "overview" | "dpi" | "ai" | "telegram" | "lists" | "profiles" | "settings";

const items: { id: Tab; label: string; icon: (p: { size?: number }) => JSX.Element; soon?: boolean }[] = [
  { id: "overview", label: "Обзор", icon: Icon.Shield },
  { id: "dpi", label: "DPI-обход", icon: Icon.Bolt },
  { id: "ai", label: "ИИ-разблокировка", icon: Icon.Robot },
  { id: "telegram", label: "Telegram", icon: Icon.Send },
  { id: "lists", label: "Списки", icon: Icon.List },
  { id: "profiles", label: "Профили", icon: Icon.Layers },
  { id: "settings", label: "Настройки", icon: Icon.Settings },
];

// Порядок вкладок в меню — источник направления для слайда экранов (App):
// переход вниз по списку двигает контент вверх, и наоборот.
export const TAB_ORDER: Tab[] = items.map((it) => it.id);

export function NavRail({
  active,
  onSelect,
}: {
  active: Tab;
  onSelect: (t: Tab) => void;
}) {
  const previousActiveRef = useRef(active);
  const previousIndex = TAB_ORDER.indexOf(previousActiveRef.current);
  const activeIndex = TAB_ORDER.indexOf(active);
  const farJump = Math.abs(activeIndex - previousIndex) > 1;
  const motionOff = useMotionOff();

  useLayoutEffect(() => {
    previousActiveRef.current = active;
  }, [active]);

  return (
    <nav className="flex w-[220px] flex-col px-4 pb-4 pt-2">
      {/* Лого — живой глаз (идентичность Obsession). */}
      <div className="mb-8 flex items-center gap-3 px-2">
        <EyeLogo size={40} />
        <div className="leading-tight">
          <div className="wordmark font-display text-lg font-semibold tracking-tight text-ink">Obsession</div>
          <div className="font-mono text-2xs tabular-nums tracking-widest text-ink-muted">V{__APP_VERSION__}</div>
        </div>
      </div>

      <div className="mb-2 px-2 text-3xs font-semibold uppercase tracking-[0.14em] text-ink-muted">
        Меню
      </div>

      <div className="flex flex-col gap-1">
        {items.map((it) => {
          const isActive = active === it.id;
          return (
            <button
              key={it.id}
              onClick={() => onSelect(it.id)}
              // group — для hover-сдвига связки иконка+текст ниже.
              className="no-drag theme-morph group relative flex items-center gap-3 rounded-xl px-3 py-2.5 text-sm transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent/70"
            >
              {isActive && (
                <motion.div
                  layoutId="nav-active"
                  aria-hidden
                  className="absolute inset-0 rounded-xl"
                  // В вертикальном меню дальний spring-перелёт на мгновение
                  // подсвечивал промежуточные пункты. Соседние пункты сохраняют
                  // физичный slide; дальний выбор телепортирует геометрию и
                  // проявляет новую plate коротким fade.
                  transition={farJump || motionOff ? { duration: 0 } : spring.snappy}
                >
                  <motion.span
                    key={active}
                    initial={farJump && !motionOff ? { opacity: 0, scale: 0.98 } : false}
                    animate={{ opacity: 1, scale: 1 }}
                    transition={
                      farJump && !motionOff
                        ? { duration: dur.fast, ease: ease.enter }
                        : { duration: 0 }
                    }
                    className="nav-active-plate absolute inset-0 rounded-xl border border-accent/40 bg-accent/15 shadow-glow"
                  />
                </motion.div>
              )}
              {/* Иконка: у активного пункта — акцент темы (цвет приезжает вместе
                  с пилюлей), у прочих — гаснет до soft и оживает на hover.
                  Связка иконка+текст на hover сдвигается на 2px вправо — жест
                  «пункт подаётся навстречу»; transform дёшев и не трогает пилюлю. */}
              <span
                className={[
                  "relative z-10 flex items-center gap-3 transition-[color,transform] duration-[var(--motion-fast)] group-hover:translate-x-0.5",
                  isActive ? "text-accent-cyan" : "text-ink-muted group-hover:text-ink-soft",
                ].join(" ")}
              >
                <it.icon size={18} />
                <span
                  className={[
                    "whitespace-nowrap font-medium transition-colors",
                    isActive ? "text-ink" : "text-ink-soft",
                  ].join(" ")}
                >
                  {it.label}
                </span>
              </span>
              {it.soon && (
                <span className="relative z-10 ml-auto rounded-md bg-white/8 px-1.5 py-0.5 text-3xs font-semibold uppercase text-ink-muted">
                  soon
                </span>
              )}
            </button>
          );
        })}
      </div>

      <div className="mt-auto px-2 text-3xs text-ink-muted opacity-70">made by AcediaWhy</div>
    </nav>
  );
}
