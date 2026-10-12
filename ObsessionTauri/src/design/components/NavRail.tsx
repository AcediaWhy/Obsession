import { motion } from "framer-motion";
import { Icon } from "./icons";
import { EyeLogo } from "./EyeLogo";
import { GoldenMeadowCore } from "./GoldenMeadowCore";
import { HalloweenIcon } from "./HalloweenIcon";
import { ThemeNavIcon } from "./ThemeNavIcon";
import { useMotionOff, useRenderActive } from "../render";
import { useNavIconMotion } from "../navIconMotion";
import { useThemeStore } from "../../store/themeStore";
import { useDpiStore } from "../../store/dpiStore";
import { useProxyStore } from "../../store/proxyStore";
import { useUpdateStore } from "../../store/updateStore";

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

// Shared navigation order for keyboard movement and service navigation.
export const TAB_ORDER: Tab[] = items.map((it) => it.id);

export function NavRail({
  active,
  onSelect,
}: {
  active: Tab;
  onSelect: (t: Tab) => void;
}) {
  const motionOff = useMotionOff();
  const renderOn = useRenderActive();
  // Оформление меню зависит от [data-theme] в globals.css.
  const theme = useThemeStore((s) => s.theme);
  const iconMotion = useNavIconMotion(renderOn && !motionOff, theme);
  // Оба хука вызываются без короткого замыкания, чтобы сохранять их порядок.
  const dpiActive = useDpiStore((s) => s.active);
  const proxyRunning = useProxyStore((s) => s.running);
  const updateAvailable = useUpdateStore((s) => s.result?.available ?? false);
  const burning = dpiActive || proxyRunning;


  return (
    <nav
      {...iconMotion}
      className={`app-nav-rail flex w-[220px] flex-col px-4 pb-4 pt-2 ${
        theme === "yanineko" ? "yani-cabbage-rail" : ""
      }`}
      data-burning={burning || undefined}
      data-halloween-motion={theme === "goldenmeadow" && renderOn && !motionOff}
      data-theme-icon-motion={renderOn && !motionOff}
    >
      {/* Лого следует выбранной теме, а имя продукта остаётся неизменным. */}
      <div className="app-nav-brand mb-8 flex items-center gap-3 px-2">
        <div data-startup-brand-anchor className="h-10 w-10 shrink-0">
        {theme === "goldenmeadow" ? (
          <GoldenMeadowCore size={40} active={burning} interactive={false} paused={motionOff} />
        ) : (
          <EyeLogo size={40} />
        )}
        </div>
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
              type="button"
              key={it.id}
              onClick={() => onSelect(it.id)}
              data-active={isActive || undefined}
              aria-current={isActive ? "page" : undefined}
              className="nav-item no-drag theme-morph group relative flex items-center gap-3 rounded-xl px-3 py-2.5 text-sm transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent/70"
            >
              {isActive && (
                <motion.div
                  layoutId="nav-active"
                  aria-hidden
                  className="absolute inset-0 rounded-xl"
                  transition={motionOff ? { duration: 0 } : { type: "tween", duration: .36, ease: [.25, .72, .25, 1] }}
                >
                  <span
                    className="nav-active-plate absolute inset-0 rounded-xl border border-accent/40 bg-accent/15 shadow-glow"
                  />
                </motion.div>
              )}
              {isActive && theme === "yanineko" && (
                <span aria-hidden className="nav-cig-fire" />
              )}
              <span className="yani-cabbage-number" aria-hidden="true">
                {String(index + 1).padStart(2, "0")}
              </span>
              <span
                className={[
                  "relative z-10 flex items-center gap-3 transition-[color,transform] duration-[var(--motion-fast)] group-hover:translate-x-0.5",
                  isActive ? "text-accent-cyan" : "text-ink-muted group-hover:text-ink-soft",
                ].join(" ")}
              >
                {theme === "goldenmeadow" ? <HalloweenIcon item={it.id} />
                  : <ThemeNavIcon theme={theme} item={it.id} />}
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
              {it.id === "settings" && updateAvailable && (
                <span className="relative z-10 ml-auto h-2 w-2 rounded-full bg-accent shadow-glow" aria-label="Доступно обновление" />
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
