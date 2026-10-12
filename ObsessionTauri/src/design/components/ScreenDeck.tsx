import { forwardRef, useEffect, useImperativeHandle, useLayoutEffect, useRef, useState } from "react";
import { AnimatePresence, motion, useIsPresent } from "framer-motion";
import { OverviewScreen } from "../../screens/Overview";
import { AiScreen } from "../../screens/Ai";
import { ListsScreen } from "../../screens/Lists";
import { ProfilesScreen } from "../../screens/Profiles";
import { SettingsScreen } from "../../screens/Settings";
import { useThemeStore } from "../../store/themeStore";
import type { Tab } from "./NavRail";
import { ScreenContinuity, isServicePair, isServiceTab, type ServiceTab } from "../screenContinuity";
import { ServiceWorkspace } from "./ServiceWorkspace";
import { onRenderActiveChange, renderHidden, renderStill, useMotionOff } from "../render";
import { FocusTransition } from "../focusTransition";
import { screenVariants } from "../screenTransition";
import "../../styles/screen-navigation.css";

export type ScreenDeckHandle = { prepare: (next: Tab) => void };

export function useScreenNavigation() {
  const [tab, setTab] = useState<Tab>("overview");
  const current = useRef(tab);
  const deckRef = useRef<ScreenDeckHandle>(null);
  const selectTab = (next: Tab) => {
    if (next === current.current) return;
    deckRef.current?.prepare(next);
    current.current = next;
    setTab(next);
  };
  return { tab, selectTab, deckRef };
}

function RouteFrame({ tab, children }: { tab: Tab; children: React.ReactNode }) {
  const present = useIsPresent();
  const ref = useRef<HTMLDivElement>(null);
  useLayoutEffect(() => { if (ref.current) ref.current.inert = !present; }, [present]);
  return <div ref={ref} data-screen-route={tab} data-screen-current={present} aria-hidden={present ? undefined : true} className="h-full" tabIndex={-1}>
    <ScreenContinuity.Provider value>{children}</ScreenContinuity.Provider>
  </div>;
}

type Pending = { to: Tab; departing: HTMLElement; focus: boolean; service: boolean };

export const ScreenDeck = forwardRef<ScreenDeckHandle, { tab: Tab; onSelect: (tab: Tab) => void }>(function ScreenDeck({ tab, onSelect }, forwardedRef) {
  const stage = useRef<HTMLDivElement>(null);
  const transfer = useRef(new FocusTransition());
  const pending = useRef<Pending | null>(null);
  const overviewScroll = useRef(0);
  const serviceScroll = useRef<Partial<Record<ServiceTab, number>>>({});
  const entry = useRef({ tab, id: 0 });
  const motionOff = useMotionOff();
  const theme = useThemeStore((state) => state.theme);
  if (entry.current.tab !== tab) entry.current = {
    tab, id: entry.current.id + (isServicePair(entry.current.tab, tab) ? 0 : 1),
  };

  useImperativeHandle(forwardedRef, () => ({ prepare(next) {
    const host = stage.current?.querySelector<HTMLElement>('[data-screen-current="true"]');
    if (!host) return;
    // A service swap keeps this route and its glass. Let a still-running route
    // entrance finish; freezing it here would leave paint with no next owner.
    if (!isServicePair(tab, next)) transfer.current.interrupt(host);
    const board = host.querySelector<HTMLElement>(".overview-board");
    if (board) overviewScroll.current = board.scrollTop;
    const controls = host.querySelector<HTMLElement>(".service-primary > .scroll-fade");
    const displayedService = host.querySelector<HTMLElement>('[data-service-current="true"]')?.dataset.serviceBody;
    // A quick return can happen before the queued service body has appeared.
    if (controls && isServiceTab(tab) && displayedService === tab) serviceScroll.current[tab] = controls.scrollTop;
    pending.current = { to: next, departing: host, focus: host.contains(document.activeElement), service: isServicePair(tab, next) };
  } }), [tab]);

  useLayoutEffect(() => {
    const host = stage.current?.querySelector<HTMLElement>('[data-screen-current="true"]');
    if (!host) return;
    const board = host.querySelector<HTMLElement>(".overview-board");
    if (board) board.scrollTop = overviewScroll.current;
    const request = pending.current;
    pending.current = null;
    if (request?.to !== tab) return;
    // Same route key preserves glass, character canvas and the journal.
    if (!request.service) transfer.current.start(request.departing, host, !motionOff && !renderHidden());
    if (request.focus) {
      const target = host.querySelector<HTMLElement>("h1");
      (target ?? host).focus({ preventScroll: true });
    }
  }, [tab, motionOff]);

  useLayoutEffect(() => { transfer.current.stop(); }, [theme, motionOff]);
  useEffect(() => {
    const stop = () => transfer.current.stop();
    const unsubscribe = onRenderActiveChange(() => { if (renderHidden() || renderStill()) stop(); });
    window.addEventListener("resize", stop);
    const observer = typeof ResizeObserver === "undefined" ? null : new ResizeObserver(stop);
    if (stage.current) observer?.observe(stage.current);
    return () => { unsubscribe(); window.removeEventListener("resize", stop); observer?.disconnect(); stop(); };
  }, []);

  const content = tab === "overview" ? <OverviewScreen />
    : isServiceTab(tab) ? <ServiceWorkspace tab={tab} restoreScrollTop={serviceScroll.current[tab] ?? 0} onOverview={() => onSelect("overview")} />
    : tab === "ai" ? <AiScreen /> : tab === "lists" ? <ListsScreen />
    : tab === "profiles" ? <ProfilesScreen /> : <SettingsScreen />;
  const route = <RouteFrame tab={tab}>{content}</RouteFrame>;
  return <div ref={stage} data-screen-deck data-motion-language="focus" className="relative h-full min-h-0 flex-1">
    <AnimatePresence mode="sync">
      <motion.div key={entry.current.id} variants={screenVariants} initial="enter" animate="center"
        exit={motionOff ? { "--route-presence": 0, pointerEvents: "none", transition: { duration: 0 } } : "exit"}
        className="absolute inset-0 h-full">{route}</motion.div>
    </AnimatePresence>
  </div>;
});
