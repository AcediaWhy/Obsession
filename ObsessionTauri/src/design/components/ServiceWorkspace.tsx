import { useCallback, useEffect, useLayoutEffect, useRef, useState, type MutableRefObject, type ReactNode, type RefObject } from "react";
import { AnimatePresence, motion, useIsPresent, usePresence } from "framer-motion";
import { useShallow } from "zustand/react/shallow";
import { DpiScreen } from "../../screens/Dpi";
import { TelegramScreen } from "../../screens/Telegram";
import { useDpiStore } from "../../store/dpiStore";
import { useProxyStore } from "../../store/proxyStore";
import { useSettingsStore } from "../../store/settingsStore";
import { useZapret2StrategyStore } from "../../store/zapret2StrategyStore";
import { useThemeStore } from "../../store/themeStore";
import { onRenderActiveChange, renderHidden, renderStill, useMotionOff, useRenderHidden } from "../render";
import { ScreenContinuity, type ServiceTab } from "../screenContinuity";
import { FocusTransition, FOCUS_ENTER, FOCUS_EXIT } from "../focusTransition";
import { GlassPanel } from "./GlassPanel";
import { HeroCore } from "./HeroCore";
import { LogStream } from "./LogStream";
import { Parallax } from "../parallax";
import { StaggerItem } from "./Stagger";
import { StatusBadge } from "./atoms";
import { Uptime } from "./Uptime";
import { Icon } from "./icons";

const glide = [.25, .72, .25, 1] as const;
const release = [.4, 0, .7, 1] as const;

const headingMotion = {
  enter: { opacity: 0, y: 14, filter: "blur(1.5px)", clipPath: "inset(100% 0 0 0)" },
  center: { opacity: 1, y: 0, filter: "blur(0px)", clipPath: "inset(0% 0 0% 0)", transition: { duration: FOCUS_ENTER / 1000, ease: glide } },
  exit: { opacity: 0, y: -10, filter: "blur(1px)", clipPath: "inset(0 0 100% 0)", transition: { duration: FOCUS_EXIT / 1000, ease: release } },
};

function ServiceHeadingCopy({ tab, title, hint, motionOff }: { tab: ServiceTab; title: string; hint: string; motionOff: boolean }) {
  const present = useIsPresent();
  return <motion.div aria-hidden={present ? undefined : true} variants={headingMotion}
    initial={motionOff ? false : "enter"}
    animate={motionOff ? { opacity: 1, y: 0, filter: "blur(0px)", clipPath: "inset(0% 0 0% 0)", transition: { duration: 0 } } : "center"}
    exit={motionOff ? { opacity: 0, transition: { duration: 0 } } : "exit"}>
    <div className="flex items-center gap-2.5">
      <span data-service-anchor="icon" className="service-icon inline-flex h-[25px] w-[25px] shrink-0 text-accent-cyan" aria-hidden="true">
        {tab === "dpi" ? <Icon.Bolt size={25} /> : <Icon.Send size={25} />}
      </span>
      <h1 data-service-anchor="title" tabIndex={-1}
        className="service-title whitespace-nowrap font-display text-3xl font-semibold tracking-tight text-gradient">{title}</h1>
    </div>
    <div className="service-caption-line text-sm text-ink-muted"><p>{hint}</p></div>
  </motion.div>;
}

// Only the settings body is replaced. Glass, the log's scroll node and the
// character's canvas belong to this persistent workspace across both services.
export function ServiceWorkspace({ tab, onOverview, restoreScrollTop = 0 }: { tab: ServiceTab; onOverview: () => void; restoreScrollTop?: number }) {
  const motionOff = useMotionOff();
  const hidden = useRenderHidden();
  const [displayTab, setDisplayTab] = useState(tab);
  const hero = useRef<HTMLDivElement>(null);
  const heroTransition = useRef(new FocusTransition());
  const bodyReady = useCallback((next: ServiceTab) => setDisplayTab(next), []);
  const entry = useRef({ tab, animated: false });
  if (entry.current.tab !== tab) entry.current = {
    tab, animated: true,
  };
  const { animated } = entry.current;
  useEffect(() => () => heroTransition.current.stop(), []);
  const dpi = useDpiStore(useShallow((s) => ({ active: s.active, transitioning: s.transitioning,
    testing: s.testing, results: s.testResults, startedAt: s.startedAt, processes: s.processes,
    start: s.start, stop: s.stop })));
  const proxy = useProxyStore(useShallow((s) => ({ running: s.running, transitioning: s.transitioning,
    available: s.available, start: s.start, stop: s.stop })));
  const protectedDpi = useSettingsStore((s) => s.protectedDpiAvailable);
  const strategyBusy = useZapret2StrategyStore((s) => s.busy);
  const isDpi = tab === "dpi";
  const shownDpi = displayTab === "dpi";
  const busy = shownDpi ? dpi.transitioning || dpi.testing || strategyBusy || !protectedDpi
    : proxy.transitioning || (!proxy.running && !proxy.available);
  const active = shownDpi ? dpi.active : proxy.running;
  const title = isDpi ? "DPI-обход" : "Telegram-прокси";
  const hint = isDpi ? "Обход блокировок через Zapret (winws)" : "Собственный MTProto-прокси Obsession в один клик";
  const caption = shownDpi ? !protectedDpi ? "Защищённая служба DPI недоступна"
    : dpi.active ? dpi.processes.length ? `Обход активен · ${dpi.processes.length} процесс(ов)` : "Обход активен · системная служба"
    : "Активируйте ядро" : proxy.running ? "Прокси работает" : proxy.available ? "Нажмите, чтобы запустить"
    : "Компонент Telegram-прокси отсутствует или повреждён";
  return <div className="service-workspace flex h-full flex-col gap-4" data-service-workspace={tab}>
    <StaggerItem standalone className="screen-heading service-heading flex flex-wrap items-center justify-between gap-3">
      <div className="service-heading-copy min-w-0 basis-[280px] flex-1 overflow-hidden">
        {motionOff || hidden ? <ServiceHeadingCopy tab={tab} title={title} hint={hint} motionOff /> : <AnimatePresence initial={false} mode="wait">
          <ServiceHeadingCopy key={tab} tab={tab} title={title} hint={hint} motionOff={motionOff} />
        </AnimatePresence>}
      </div>
      <div className="flex shrink-0 items-center gap-2">
        <button type="button" onClick={onOverview} aria-label="Вернуться в Обзор" className="no-drag inline-flex items-center gap-1.5 rounded-lg border border-glass-border bg-white/5 px-2 py-1.5 text-xs text-ink transition-colors hover:bg-white/10 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent/70">
          <svg width="12" height="12" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true"><path d="M13 8H3m4-4L3 8l4 4" /></svg>
          Обзор
        </button>
        <Uptime active={active} startedAt={shownDpi ? dpi.startedAt : undefined} />
        <StatusBadge active={active} labelOn={shownDpi ? "Активно" : "Запущен"} labelOff={shownDpi ? "Выключено" : "Остановлен"} />
      </div>
    </StaggerItem>
    <div className="screen-split screen-split--end-360">
      <GlassPanel scroll data-service-anchor="surface" contentClassName="flex flex-col gap-6" className="service-primary">
        <div ref={hero} className="service-hero mt-2 flex flex-col items-center gap-4">
          <div className="service-hero-artwork">
            <Parallax depth={18}>
              <div className="relative h-[240px] w-[240px]">
                <HeroCore size={240} active={active} busy={busy} scanning={shownDpi && dpi.testing}
                  alarm={shownDpi && dpi.testing && Object.values(dpi.results).some((result) => !result)}
                  onClick={() => {
                    if (busy) return;
                    if (shownDpi) { if (dpi.active) void dpi.stop(); else if (protectedDpi) void dpi.start(); }
                    else { if (proxy.running) void proxy.stop(); else if (proxy.available) void proxy.start(); }
                  }} />
              </div>
            </Parallax>
          </div>
          <div className="service-caption-line w-full text-center text-sm text-ink-soft">
            <p>{caption}</p>
          </div>
        </div>
        <div className="service-controls relative w-full">
          <AnimatePresence initial={false} mode="wait">
            <ServiceControls key={tab} tab={tab} animated={animated} restoreScrollTop={restoreScrollTop}
              hero={hero} heroTransition={heroTransition} onReady={bodyReady}>
              {isDpi ? <DpiScreen embedded /> : <TelegramScreen embedded />}
            </ServiceControls>
          </AnimatePresence>
        </div>
      </GlassPanel>
      <GlassPanel className="service-journal flex flex-col overflow-hidden" data-service-journal><LogStream /></GlassPanel>
    </div>
  </div>;
}

function ServiceControls({ children, tab, animated, restoreScrollTop, hero, heroTransition, onReady }: {
  children: ReactNode; tab: ServiceTab; animated: boolean; restoreScrollTop: number;
  hero: RefObject<HTMLDivElement>; heroTransition: MutableRefObject<FocusTransition>; onReady: (tab: ServiceTab) => void;
}) {
  const root = useRef<HTMLDivElement>(null);
  const [present, safeToRemove] = usePresence();
  const motionOff = useMotionOff();
  const theme = useThemeStore((state) => state.theme);
  const transition = useRef(new FocusTransition());
  const phase = useRef<boolean | undefined>(undefined);
  const savedScroll = useRef(restoreScrollTop);
  const remove = useRef(safeToRemove);
  remove.current = safeToRemove;
  const ready = useRef(onReady);
  ready.current = onReady;
  const stop = () => transition.current.stop();
  useEffect(() => {
    const settle = () => {
      stop(); heroTransition.current.stop();
      if (root.current) root.current.style.visibility = present ? "" : "hidden";
      if (hero.current) { hero.current.style.visibility = present ? "" : "hidden"; hero.current.inert = !present; }
      if (!present) remove.current?.();
    };
    const unsubscribe = onRenderActiveChange(() => { if (renderHidden() || renderStill()) settle(); });
    window.addEventListener("resize", settle);
    return () => { unsubscribe(); window.removeEventListener("resize", settle); };
  }, [present, hero, heroTransition]);
  useEffect(() => () => transition.current.stop(), []);
  useLayoutEffect(() => {
    const host = root.current;
    if (!host) return;
    const first = phase.current === undefined;
    const changed = phase.current !== present;
    phase.current = present;
    const artwork = hero.current;
    const footprint = host.parentElement;
    const enabled = !motionOff && !renderHidden();
    host.inert = !present;
    if (artwork) artwork.inert = !present;
    if (present) {
      if (first) {
        // The glass never moves. Both foreground groups are hidden while its
        // scroll range and the service's saved position are transferred.
        stop(); heroTransition.current.stop();
        host.style.visibility = "hidden";
        if (artwork) artwork.style.visibility = "hidden";
        if (footprint) footprint.style.minHeight = "";
        const viewport = host.closest(".service-primary")?.querySelector<HTMLElement>(".scroll-fade");
        if (viewport) viewport.scrollTop = savedScroll.current;
        ready.current(tab);
      } else if (changed && enabled) {
        transition.current.interrupt(host);
        if (artwork) heroTransition.current.interrupt(artwork);
      } else {
        stop(); heroTransition.current.stop();
      }
      host.style.visibility = "";
      if (artwork) artwork.style.visibility = "";
      if (footprint) footprint.style.minHeight = "";
      if (enabled && animated && changed) {
        transition.current.reveal(host, FOCUS_ENTER, !first);
        if (artwork) heroTransition.current.reveal(artwork, FOCUS_ENTER, !first, false);
      }
      return;
    }
    if (!enabled || !changed) {
      stop(); heroTransition.current.stop();
      host.style.visibility = "hidden";
      if (artwork) artwork.style.visibility = "hidden";
      remove.current?.();
      return;
    }
    if (footprint) footprint.style.minHeight = `${host.offsetHeight}px`;
    transition.current.interrupt(host);
    if (artwork) { heroTransition.current.interrupt(artwork); heroTransition.current.exit(artwork); }
    transition.current.exit(host);
    // A single phase boundary swaps the body and its hero caption. This clock
    // also guarantees removal when WAAPI is unavailable or the view is hidden.
    const timer = window.setTimeout(() => {
      host.style.visibility = "hidden";
      if (artwork) artwork.style.visibility = "hidden";
      remove.current?.();
    }, FOCUS_EXIT);
    // Cancelling a timer must not release the current paint before a reversal
    // captures it. Unmount/render-state cleanup owns final animation removal.
    return () => window.clearTimeout(timer);
  }, [present, motionOff, theme, animated, tab, hero, heroTransition]);
  return <div ref={root} data-service-body={tab} data-service-current={present} aria-hidden={present ? undefined : true} className="w-full">
    <ScreenContinuity.Provider value>{children}</ScreenContinuity.Provider>
  </div>;
}
