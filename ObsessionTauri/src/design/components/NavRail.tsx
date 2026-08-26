import { useLayoutEffect, useRef } from "react";
import { motion } from "framer-motion";
import { Icon } from "./icons";
import { EyeLogo } from "./EyeLogo";
import { useMotionOff } from "../render";
import { dur, ease, spring } from "../tokens";
import { useThemeStore } from "../../store/themeStore";
import { useDpiStore } from "../../store/dpiStore";
import { useProxyStore } from "../../store/proxyStore";

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
  // Сигаретная рестилизация меню живёт только в теме Yani Neko: здесь лишь
  // класс-хук nav-item, флаг «горит» (включён обход/прокси) и декоративный
  // огонёк в активном пункте. Весь вид — в globals.css под [data-theme].
  const theme = useThemeStore((s) => s.theme);
  const burning = useDpiStore((s) => s.active) || useProxyStore((s) => s.running);

  useLayoutEffect(() => {
    previousActiveRef.current = active;
  }, [active]);

  return (
    <nav
      className={`app-nav-rail flex w-[220px] flex-col px-4 pb-4 pt-2 ${
        theme === "yanineko" ? "yani-cabbage-rail" : ""
      }`}
      data-burning={burning || undefined}
    >
      {/* Лого — живой глаз (идентичность Obsession). */}
      <div className="app-nav-brand mb-8 flex items-center gap-3 px-2">
        <EyeLogo size={40} />
        <div className="leading-tight">
          <div className="wordmark font-display text-lg font-semibold tracking-tight text-ink">Obsession</div>
          <div className="font-mono text-2xs tabular-nums tracking-widest text-ink-muted">V{__APP_VERSION__}</div>
        </div>
      </div>

      <div className="app-nav-section-label mb-2 px-2 text-3xs font-semibold uppercase tracking-[0.14em] text-ink-muted">
        Меню
      </div>

      <div className="nav-cig-pack yani-cabbage-shell relative">
        {theme === "yanineko" && (
          <>
            <div className="yani-cabbage-leaves" aria-hidden="true">
              <i />
              <i />
              <i />
              <i />
              <i />
              <i />
              <i />
            </div>
            <div className="yani-cabbage-cap" aria-hidden="true">
              <span>YANI</span>
              <i />
              <span>03:17</span>
            </div>
            <div className="yani-cabbage-foil" aria-hidden="true" />
          </>
        )}

        <div className="yani-cabbage-list relative flex flex-col gap-1">
        {items.map((it, index) => {
          const isActive = active === it.id;
          return (
            <button
              key={it.id}
              onClick={() => onSelect(it.id)}
              data-active={isActive || undefined}
              // group — для hover-сдвига связки иконка+текст ниже.
              className="nav-item no-drag theme-morph group relative flex items-center gap-3 rounded-xl px-3 py-2.5 text-sm transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent/70"
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
              {/* Yani Neko: у активного пункта — тлеющий кончик сигареты со
                  струйкой дыма. Декоративный элемент, весь вид — в globals.css. */}
              {isActive && theme === "yanineko" && (
                <span aria-hidden className="nav-cig-fire" />
              )}
              <span className="yani-cabbage-number" aria-hidden="true">
                {String(index + 1).padStart(2, "0")}
              </span>
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

        {theme === "yanineko" && (
          <>
            <div className="yani-cabbage-budget" aria-label="170 иен, 6 дней, 1 сигарета">
              <span>YANI SURVIVAL BUDGET</span>
              <strong>170円</strong>
              <small>6 DAYS / 1 CIG</small>
              <i aria-hidden="true" />
            </div>
            <div className="yani-cabbage-foot" aria-hidden="true">
              <span>STILL AWAKE</span>
              <span>NO. 07</span>
            </div>
          </>
        )}
      </div>

      <div className="app-nav-footer mt-auto px-2 text-3xs text-ink-muted opacity-70">made by AcediaWhy</div>
    </nav>
  );
}
