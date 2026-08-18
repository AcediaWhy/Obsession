import { useEffect, useState } from "react";
import { AnimatePresence, motion } from "framer-motion";

import { useDpiStore } from "../store/dpiStore";
import { useProxyStore } from "../store/proxyStore";
import { useHostsStore } from "../store/hostsStore";
import { useSettingsStore } from "../store/settingsStore";
import { api, type NetworkInfo } from "../lib/tauri";
import { GlassPanel } from "../design/components/GlassPanel";
import { Stagger, StaggerItem } from "../design/components/Stagger";
import { Button } from "../design/components/atoms";
import { Uptime } from "../design/components/Uptime";
import { Icon } from "../design/components/icons";
import { cascade, dur, ease, spring } from "../design/tokens";
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
  const reduceMotion = useSettingsStore((s) => s.settings?.reduce_motion);
  const [net, setNet] = useState<NetworkInfo | null>(null);

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
  // Гасим «дыхание» глаза, когда окно скрыто: это тоже бесконечная
  // framer-motion-анимация (WAAPI), иначе жгла бы CPU в свёрнутом виде.
  const renderOn = useRenderActive();
  const breathing = protectedNow && !reduceMotion && renderOn;
  const activeCats = dpiSelectedCategories.map((c) => CATEGORY_LABELS[c] ?? c);
  const canToggle = !dpiTransitioning && dpiSelectedCategories.length > 0;

  return (
    <div className="flex h-full flex-col gap-4 overflow-y-auto pr-1">
      <StaggerItem standalone>
        <h1 className="font-display text-3xl font-semibold tracking-tight text-gradient">Обзор</h1>
        <p className="text-sm text-ink-muted">Состояние защиты одним взглядом</p>
      </StaggerItem>

      {/* Командный центр: живой глаз-герой + главное действие. */}
      <GlassPanel glow={protectedNow}>
        <div className="flex items-center gap-5">
          {/* Глаз — идентичность Obsession. Открыт и «дышит», пока под защитой. */}
          <motion.div
            initial={{ scale: 0.9, opacity: 0 }}
            animate={
              breathing
                ? { scale: [1, 1.045, 1], opacity: 1 }
                : { scale: 1, opacity: 1 }
            }
            transition={
              breathing
                ? { scale: { duration: 3.4, repeat: Infinity, ease: "easeInOut" } }
                : spring.soft
            }
            className={[
              // Доводка цвета/свечения за base-такт — состояние «протекает» в новое,
              // в одном темпе с glow панели и бейджами.
              "relative flex h-16 w-16 shrink-0 items-center justify-center rounded-full border transition-[color,background-color,border-color,box-shadow] duration-[var(--motion-base)]",
              protectedNow
                ? "border-ok/40 bg-ok/15 text-ok shadow-glow"
                : "border-glass-border bg-white/5 text-ink-muted",
            ].join(" ")}
          >
            {breathing && (
              <motion.span
                aria-hidden
                className="absolute inset-0 rounded-full border border-ok/40"
                animate={{ scale: [1, 1.4], opacity: [0.5, 0] }}
                transition={{ duration: 2.6, repeat: Infinity, ease: "easeOut" }}
              />
            )}
            <Icon.Eye size={30} className={protectedNow ? "" : "opacity-60"} />
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
                    ? `Глаз открыт · обход активен${activeCats.length ? ` · ${activeCats.join(", ")}` : ""}`
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
      <Stagger className="grid grid-cols-2 gap-4">
        <StaggerItem glass>
          <StatusCard
            entryOrder={0}
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
            icon={<Icon.Globe size={18} />}
            title="Сеть"
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
  icon,
  title,
  on,
  onLabel,
  offLabel,
  detail,
  warn = false,
  neutral = false,
}: {
  entryOrder?: number;
  icon: React.ReactNode;
  title: string;
  on: boolean;
  onLabel: string;
  offLabel: string;
  detail: string;
  warn?: boolean;
  neutral?: boolean;
}) {
  const motionOff = useMotionOff();
  const tone = warn
    ? "text-warn"
    : on
      ? neutral
        ? "text-accent"
        : "text-ok"
      : "text-ink-muted";
  const dot = warn ? "bg-warn" : on ? (neutral ? "bg-accent" : "bg-ok") : "bg-ink-muted";
  // Учитываем не только on: outdated -> installed остаётся on=true, но это всё
  // равно полноценная смена состояния, которую должны заметить label/dot/wash.
  const stateKey = `${on}:${warn}:${neutral}:${on ? onLabel : offLabel}`;

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
        transition={
          motionOff
            ? { duration: dur.fast, ease: ease.enter }
            : {
                duration: dur.slow,
                ease: ease.enter,
                delay: entryOrder * cascade.step,
              }
        }
        className="relative flex h-full flex-col gap-3 overflow-hidden"
      >
        {/* Одноразовый wash при настоящей смене состояния. initial={false}
            оставляет холодный mount спокойным; постоянного pulse у карточки нет. */}
        <AnimatePresence initial={false}>
          <motion.span
            key={stateKey}
            aria-hidden
            initial={
              motionOff
                ? { opacity: 0 }
                : { x: "-130%", opacity: 0 }
            }
            animate={
              motionOff
                ? { opacity: 0 }
                : { x: "300%", opacity: [0, 0.08, 0] }
            }
            exit={{ opacity: 0, transition: { duration: 0 } }}
            transition={{ duration: dur.slow + dur.base, ease: ease.xfade }}
            className={`pointer-events-none absolute inset-y-0 left-0 w-1/2 -skew-x-12 bg-gradient-to-r from-transparent via-current to-transparent ${tone}`}
          />
        </AnimatePresence>

        <div className="relative flex items-center gap-2.5">
          <span className="flex h-9 w-9 items-center justify-center rounded-xl bg-white/5 text-ink-soft">
            {icon}
          </span>
          <span className="text-sm font-semibold text-ink">{title}</span>
          <span className="ml-auto flex items-center gap-1.5" aria-live="polite">
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
            <span className="grid min-w-[6.75rem] justify-items-end">
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

        <div className="relative grid min-w-0">
          <AnimatePresence initial={false} mode="sync">
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
          </AnimatePresence>
        </div>
      </GlassPanel>
    </motion.div>
  );
}
