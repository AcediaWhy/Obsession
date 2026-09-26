import { useEffect, useState, type PointerEvent as ReactPointerEvent } from "react";
import { AnimatePresence, motion } from "framer-motion";

import { useDpiStore } from "../store/dpiStore";
import { useProxyStore } from "../store/proxyStore";
import { useHostsStore } from "../store/hostsStore";
import { useThemeStore, type Theme } from "../store/themeStore";
import { OverviewOrnament, OverviewStatusEffect } from "./OverviewAtmosphere";
import catIcon from "../../src-tauri/icons/128x128.png";
import "../styles/overview.css";
import "../styles/overview-atmospheres.css";
import { api, type NetworkInfo } from "../lib/tauri";
import { GlassPanel } from "../design/components/GlassPanel";
import { Stagger, StaggerItem } from "../design/components/Stagger";
import { Button } from "../design/components/atoms";
import { Uptime } from "../design/components/Uptime";
import { Icon } from "../design/components/icons";
import { cascade, dur, ease } from "../design/tokens";
import { useMotionOff, useRenderActive } from "../design/render";

const CATEGORY_LABELS: Record<string, string> = {
  discord: "Discord",
  youtube_twitch: "YouTube / Twitch",
  gaming: "Gaming",
  universal: "Universal",
};

const AI_STATUS_LABEL: Record<string, string> = {
  installed: "установлено",
  outdated: "есть обновление",
  not_installed: "не установлено",
  offline: "оффлайн",
};

const HIDE_NETWORK_KEY = "obsession.overview.hideNetwork";

export function OverviewScreen() {
  // Точечные селекторы вместо подписки на весь стор: экран не должен
  // ре-рендериться на изменения testingLabel/testResults/netStats и пр., которые
  // он не показывает (актуально во время DPI-теста — там частые set в цикле).
  const dpiActive = useDpiStore((s) => s.active);
  const dpiStartedAt = useDpiStore((s) => s.startedAt);
  const dpiSelectedCategories = useDpiStore((s) => s.selectedCategories);
  const dpiTransitioning = useDpiStore((s) => s.transitioning);
  const dpiStart = useDpiStore((s) => s.start);
  const dpiStop = useDpiStore((s) => s.stop);
  const proxyRunning = useProxyStore((s) => s.running);
  const proxyAvailable = useProxyStore((s) => s.available);
  const proxyPort = useProxyStore((s) => s.port);
  const hostsStatus = useHostsStore((s) => s.status);
  const hostsLocalVersion = useHostsStore((s) => s.localVersion);
  const hostsProvider = useHostsStore((s) => s.provider);
  const theme = useThemeStore((s) => s.theme);
  const motionOff = useMotionOff();
  const [net, setNet] = useState<NetworkInfo | null>(null);
  const [networkHidden, setNetworkHidden] = useState(() => {
    try {
      return localStorage.getItem(HIDE_NETWORK_KEY) === "true";
    } catch {
      return false;
    }
  });

  useEffect(() => {
    try {
      localStorage.setItem(HIDE_NETWORK_KEY, String(networkHidden));
    } catch {
      // При недоступном localStorage переключатель действует до закрытия экрана.
    }
  }, [networkHidden]);

  useEffect(() => {
    // cancelled-гвард: getNetworkIdentity() может быть медленным/оффлайн; уход с
    // экрана до резолва не должен звать setState на размонтированном компоненте.
    let cancelled = false;
    api.getNetworkIdentity().then((n) => {
      if (!cancelled) setNet(n);
    }).catch(() => {});
    return () => {
      cancelled = true;
    };
  }, []);

  const protectedNow = dpiActive;
  const renderOn = useRenderActive();
  const still = motionOff || !renderOn;
  const activeCats = dpiSelectedCategories.map((c) => CATEGORY_LABELS[c] ?? c);
  const canToggle = !dpiTransitioning && dpiSelectedCategories.length > 0;

  return (
    <div className="overview-board flex h-full flex-col gap-4 overflow-y-auto pr-1" data-note-motion={still ? "still" : "on"}>
      <StaggerItem standalone>
        <h1 className="font-display text-3xl font-semibold tracking-tight text-gradient">Обзор</h1>
        <p className="text-sm text-ink-muted">Состояние защиты одним взглядом</p>
      </StaggerItem>

      {/* Командный центр: котик с короткой реакцией на включение защиты. */}
      <GlassPanel spotlight={false} className="overview-note overview-command" data-active={protectedNow}>
        <OverviewStatusEffect theme={theme} event={String(protectedNow)} disabled={still} />
        <div className="overview-command-content flex items-center gap-5">
          <motion.div className="overview-mascot" aria-hidden="true" initial={false}
            animate={dpiActive && !still ? { rotate: [0, -7, 3, 0], y: [0, -3, 0, 0] } : { rotate: 0, y: 0 }}
            transition={{ duration: still ? 0 : 0.42, ease: ease.enter }}>
            <img src={catIcon} alt="" width={64} height={64} draggable={false} />
            <svg className="overview-season-hat" viewBox="0 0 52 42" fill="none">
              <path d="M12 31 26 4l4 13 9 13Z" fill="#352938" stroke="#e9c789" strokeWidth="1.5" strokeLinejoin="round" />
              <path d="m16 25 20-1 3 6-27 1Z" fill="#bb8650" />
              <path d="M5 33q20-7 41-3-17 10-41 3Z" fill="#352938" stroke="#e9c789" strokeWidth="1.5" strokeLinejoin="round" />
              <path d="m26 24 5-1 1 5-5 1Z" stroke="#f5dfac" strokeWidth="1.5" />
            </svg>
          </motion.div>

          <div className="min-w-0 flex-1">
            <div className="flex items-center gap-3">
              {/* Статус и подпись свапаются кроссфейдом (mode="wait", только
                  opacity): без движения, просто одно состояние растворяется в
                  другое. grid-стопка держит место — строка не схлопывается на
                  кадры, пока уходящий текст ещё жив. */}
              <div className="grid">
                <AnimatePresence mode="wait" initial={false}>
                  <motion.span
                    key={protectedNow ? "on" : "off"}
                    initial={{ opacity: 0 }}
                    animate={{ opacity: 1 }}
                    exit={{ opacity: 0, transition: { duration: dur.fast, ease: ease.exit } }}
                    transition={{ duration: dur.base, ease: ease.enter }}
                    className={`col-start-1 row-start-1 whitespace-nowrap text-2xl font-bold ${protectedNow ? "text-ink" : "text-ink-soft"}`}
                  >
                    {protectedNow ? "Под защитой" : "Защита выключена"}
                  </motion.span>
                </AnimatePresence>
              </div>
              <Uptime active={protectedNow} startedAt={dpiStartedAt} />
            </div>
            <div className="mt-1 grid">
              <AnimatePresence mode="wait" initial={false}>
                <motion.div
                  key={protectedNow ? "on" : "off"}
                  initial={{ opacity: 0 }}
                  animate={{ opacity: 1 }}
                  exit={{ opacity: 0, transition: { duration: dur.fast, ease: ease.exit } }}
                  transition={{ duration: dur.base, ease: ease.enter }}
                  className="col-start-1 row-start-1 truncate text-sm text-ink-muted"
                >
                  {protectedNow
                    ? `Обход активен${activeCats.length ? ` · ${activeCats.join(", ")}` : ""}`
                    : "Один клик — и обход включится с текущими настройками"}
                </motion.div>
              </AnimatePresence>
            </div>
          </div>

          <Button
            variant={protectedNow ? "ghost" : "primary"}
            disabled={!canToggle}
            onClick={() => (dpiActive ? dpiStop() : dpiStart())}
          >
            {dpiTransitioning
              ? protectedNow
                ? "Выключаю…"
                : "Включаю…"
              : protectedNow
                ? "Выключить"
                : "Включить защиту"}
          </Button>
        </div>
      </GlassPanel>

      {/* Карточки сервисов. */}
      <Stagger className="overview-notes grid grid-cols-2 gap-4">
        <StaggerItem glass>
          <StatusCard
            entryOrder={0}
            theme={theme}
            still={still}
            icon={<Icon.Bolt size={18} />}
            title="DPI-обход"
            on={dpiActive}
            onLabel="Активен"
            offLabel="Выключен"
            detail={
              dpiSelectedCategories.length
                ? dpiSelectedCategories
                    .map((c) => CATEGORY_LABELS[c] ?? c)
                    .join(", ")
                : "Категории не выбраны"
            }
          />
        </StaggerItem>

        <StaggerItem glass>
          <StatusCard
            entryOrder={1}
            theme={theme}
            still={still}
            icon={<Icon.Send size={18} />}
            title="Telegram-прокси"
            on={proxyRunning}
            onLabel="Работает"
            offLabel={proxyAvailable ? "Выключен" : "Недоступен"}
            detail={
              proxyRunning
                ? `MTProto на 127.0.0.1:${proxyPort}`
                : proxyAvailable
                  ? "Готов к запуску"
                  : "Компонент Telegram-прокси не найден"
            }
          />
        </StaggerItem>

        <StaggerItem glass>
          <StatusCard
            entryOrder={2}
            theme={theme}
            still={still}
            icon={<Icon.Robot size={18} />}
            title="ИИ-разблокировка"
            on={hostsStatus === "installed" || hostsStatus === "outdated"}
            onLabel={hostsStatus === "outdated" ? "Обновить" : "Установлено"}
            offLabel={AI_STATUS_LABEL[hostsStatus] ?? "не установлено"}
            warn={hostsStatus === "outdated"}
            detail={
              hostsLocalVersion
                ? `Провайдер ${hostsProvider} · v${hostsLocalVersion}`
                : `Провайдер ${hostsProvider}`
            }
          />
        </StaggerItem>

        <StaggerItem glass>
          <StatusCard
            entryOrder={3}
            theme={theme}
            still={still}
            icon={<Icon.Globe size={18} />}
            title="Сеть"
            detailHidden={networkHidden}
            headingAction={
              <button
                type="button"
                className="overview-network-visibility"
                aria-label={networkHidden ? "Показать сведения о сети" : "Скрыть сведения о сети"}
                title={networkHidden ? "Показать сведения о сети" : "Скрыть сведения о сети"}
                aria-pressed={networkHidden}
                onClick={() => setNetworkHidden((hidden) => !hidden)}
              >
                <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
                  <path d="M2 12s3.5-7 10-7 10 7 10 7-3.5 7-10 7-10-7-10-7Z" />
                  <circle cx="12" cy="12" r="3" />
                  {networkHidden && <path d="m3 3 18 18" />}
                </svg>
              </button>
            }
            on={!!net?.online}
            onLabel="Определена"
            offLabel={net ? "Не определена" : "…"}
            neutral
            detail={
              net?.org ||
              net?.asn_region ||
              (net?.gateway_mac_masked
                ? `Шлюз ${net.gateway_mac_masked}`
                : "Идентичность недоступна")
            }
          />
        </StaggerItem>
      </Stagger>
    </div>
  );
}

function StatusCard({
  entryOrder = 0,
  theme,
  still,
  icon,
  title,
  on,
  onLabel,
  offLabel,
  detail,
  warn = false,
  neutral = false,
  headingAction,
  detailHidden = false,
}: {
  entryOrder?: number;
  theme: Theme;
  still: boolean;
  icon: React.ReactNode;
  title: string;
  on: boolean;
  onLabel: string;
  offLabel: string;
  detail: string;
  warn?: boolean;
  neutral?: boolean;
  headingAction?: React.ReactNode;
  detailHidden?: boolean;
}) {
  const motionOff = still;
  const tone = warn
    ? "text-warn"
    : on
      ? neutral
        ? "text-accent"
        : "text-ok"
      : "text-ink-muted";
  const dot = warn ? "bg-warn" : on ? (neutral ? "bg-accent" : "bg-ok") : "bg-ink-muted";
  // Учитываем не только on: outdated -> installed остаётся on=true, но это
  // полноценная смена состояния для подписи, точки и мягкой волны.
  const stateKey = `${on}:${warn}:${neutral}:${on ? onLabel : offLabel}`;
  const materialEnabled = !still;
  const moveMaterial = (event: ReactPointerEvent<HTMLDivElement>) => {
    if (!materialEnabled || event.pointerType === "touch") return;
    const card = event.currentTarget;
    const bounds = card.getBoundingClientRect();
    const x = Math.max(0, Math.min(100, ((event.clientX - bounds.left) / bounds.width) * 100));
    const y = Math.max(0, Math.min(100, ((event.clientY - bounds.top) / bounds.height) * 100));
    card.style.setProperty("--overview-pointer-x", `${x}%`);
    card.style.setProperty("--overview-pointer-y", `${y}%`);
    if (theme === "ophanim" || theme === "fallendown") {
      // Готовые слои с узором перемещаются через transform без перерисовки градиента.
      card.style.setProperty("--overview-pointer-px", `${(x / 100) * bounds.width}px`);
      card.style.setProperty("--overview-pointer-py", `${(y / 100) * bounds.height}px`);
    }
    card.style.setProperty("--overview-surface-energy", "1");
  };
  const settleMaterial = (event: ReactPointerEvent<HTMLDivElement>) => {
    const card = event.currentTarget;
    card.style.setProperty("--overview-surface-energy", "0");
    card.style.setProperty("--overview-pointer-x", "50%");
    card.style.setProperty("--overview-pointer-y", "50%");
  };

  return (
    <motion.div
      // Opacity анимирует сама стеклянная поверхность (это сохраняет корректный
      // backdrop sampling), а внешний transform даёт ей лёгкий settle без
      // ложного hover-lift: карточки здесь информационные, не кнопки.
      initial={motionOff ? false : { y: 4, scale: 0.995 }}
      animate={{ y: 0, scale: 1 }}
      transition={
        motionOff
          ? { duration: 0 }
          : {
              duration: dur.slow,
              ease: ease.enter,
              delay: entryOrder * cascade.step,
            }
      }
      className="h-full"
    >
      <GlassPanel
        spotlight={false}
        data-active={on}
        data-tone={warn ? "warn" : on ? neutral ? "neutral" : "ok" : "off"}
        data-material={materialEnabled}
        onPointerEnter={moveMaterial}
        onPointerMove={moveMaterial}
        onPointerLeave={settleMaterial}
        transition={
          motionOff
            ? { duration: dur.fast, ease: ease.enter }
            : {
                duration: dur.slow,
                ease: ease.enter,
                delay: entryOrder * cascade.step,
              }
        }
        className="overview-note overview-service relative h-full"
      >
        <span className="overview-fx-material" aria-hidden="true" />
        <span className="overview-fx-hover" aria-hidden="true"><OverviewOrnament theme={theme} /></span>
        <OverviewStatusEffect theme={theme} event={stateKey} disabled={still} />

        <div className="overview-note-heading">
          <span className="overview-note-icon text-ink-soft">
            {icon}
          </span>
          <span className="overview-note-title text-sm font-semibold text-ink">{title}</span>
          <span className="overview-note-tools">
            <span className="overview-note-number">0{entryOrder + 1}</span>
            {headingAction}
          </span>
          <span className="overview-note-status flex items-center gap-1.5" aria-live="polite">
            <span className="relative h-2 w-2 shrink-0">
              <AnimatePresence initial={false} mode="sync">
                <motion.span
                  key={stateKey}
                  initial={motionOff ? { opacity: 0 } : { opacity: 0.5, scale: 0.82 }}
                  animate={{ opacity: 1, scale: 1 }}
                  exit={{ opacity: 0, scale: motionOff ? 1 : 1.22 }}
                  transition={
                    motionOff
                      ? { duration: dur.fast, ease: ease.enter }
                      : { duration: dur.base + dur.fast, ease: ease.xfade }
                  }
                  className={`absolute inset-0 rounded-full ${dot}`}
                />
              </AnimatePresence>
            </span>
            <span className="grid min-w-0">
              <AnimatePresence initial={false} mode="sync">
                <motion.span
                  key={stateKey}
                  initial={{ opacity: 0 }}
                  animate={{ opacity: 1 }}
                  exit={{ opacity: 0 }}
                  transition={{
                    duration: motionOff ? dur.fast : dur.base + dur.fast,
                    ease: ease.xfade,
                  }}
                  className={`col-start-1 row-start-1 whitespace-nowrap text-xs font-semibold ${tone}`}
                >
                  {on ? onLabel : offLabel}
                </motion.span>
              </AnimatePresence>
            </span>
          </span>
        </div>

        <div className="overview-note-detail relative grid min-w-0">
          {detailHidden ? (
            // Приватная строка удаляется сразу, без сохранения в уходящем кроссфейде.
            <p className="col-start-1 row-start-1 text-xs text-ink-muted">Сведения о сети скрыты</p>
          ) : <AnimatePresence initial={false} mode="sync">
            <motion.p
              key={detail}
              initial={{ opacity: 0 }}
              animate={{ opacity: 1 }}
              exit={{ opacity: 0 }}
              transition={{
                duration: motionOff ? dur.fast : dur.base + dur.fast,
                ease: ease.xfade,
              }}
              className="col-start-1 row-start-1 truncate text-xs text-ink-muted"
            >
              {detail}
            </motion.p>
          </AnimatePresence>}
        </div>
      </GlassPanel>
    </motion.div>
  );
}
