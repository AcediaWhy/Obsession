import { useEffect, useState } from "react";
import { motion } from "framer-motion";

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
import { spring } from "../design/tokens";

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
  const dpi = useDpiStore();
  const proxy = useProxyStore();
  const hosts = useHostsStore();
  const reduceMotion = useSettingsStore((s) => s.settings?.reduce_motion);
  const [net, setNet] = useState<NetworkInfo | null>(null);

  useEffect(() => {
    api.getNetworkIdentity().then(setNet).catch(() => {});
  }, []);

  const protectedNow = dpi.active;
  const breathing = protectedNow && !reduceMotion;
  const activeCats = dpi.selectedCategories.map((c) => CATEGORY_LABELS[c] ?? c);
  const canToggle = !dpi.transitioning && dpi.selectedCategories.length > 0;

  return (
    <div className="flex h-full flex-col gap-4 overflow-y-auto pr-1">
      <div>
        <h1 className="font-display text-3xl font-bold text-gradient">Обзор</h1>
        <p className="text-sm text-ink-muted">Состояние защиты одним взглядом</p>
      </div>

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
              "relative flex h-16 w-16 shrink-0 items-center justify-center rounded-full border transition-colors",
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
              <span className={`text-2xl font-bold ${protectedNow ? "text-ink" : "text-ink-soft"}`}>
                {protectedNow ? "Под защитой" : "Защита выключена"}
              </span>
              <Uptime active={protectedNow} />
            </div>
            <div className="mt-1 truncate text-sm text-ink-muted">
              {protectedNow
                ? `Глаз открыт · обход активен${activeCats.length ? ` · ${activeCats.join(", ")}` : ""}`
                : "Один клик — и обход включится с текущими настройками"}
            </div>
          </div>

          <Button
            variant={protectedNow ? "ghost" : "primary"}
            disabled={!canToggle}
            onClick={() => (dpi.active ? dpi.stop() : dpi.start())}
          >
            {dpi.transitioning
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
        <StaggerItem>
          <StatusCard
            icon={<Icon.Bolt size={18} />}
            title="DPI-обход"
            on={dpi.active}
            onLabel="Активен"
            offLabel="Выключен"
            detail={
              dpi.selectedCategories.length
                ? dpi.selectedCategories
                    .map((c) => CATEGORY_LABELS[c] ?? c)
                    .join(", ")
                : "Категории не выбраны"
            }
          />
        </StaggerItem>

        <StaggerItem>
          <StatusCard
            icon={<Icon.Send size={18} />}
            title="Telegram-прокси"
            on={proxy.running}
            onLabel="Работает"
            offLabel={proxy.available ? "Выключен" : "Недоступен"}
            detail={
              proxy.running
                ? `MTProto на 127.0.0.1:${proxy.port}`
                : proxy.available
                  ? "Готов к запуску"
                  : "TgWsProxy.exe не найден"
            }
          />
        </StaggerItem>

        <StaggerItem>
          <StatusCard
            icon={<Icon.Robot size={18} />}
            title="ИИ-разблокировка"
            on={hosts.status === "installed" || hosts.status === "outdated"}
            onLabel={hosts.status === "outdated" ? "Обновить" : "Установлено"}
            offLabel={AI_STATUS_LABEL[hosts.status] ?? "не установлено"}
            warn={hosts.status === "outdated"}
            detail={
              hosts.localVersion
                ? `Провайдер ${hosts.provider} · v${hosts.localVersion}`
                : `Провайдер ${hosts.provider}`
            }
          />
        </StaggerItem>

        <StaggerItem>
          <StatusCard
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
  icon,
  title,
  on,
  onLabel,
  offLabel,
  detail,
  warn = false,
  neutral = false,
}: {
  icon: React.ReactNode;
  title: string;
  on: boolean;
  onLabel: string;
  offLabel: string;
  detail: string;
  warn?: boolean;
  neutral?: boolean;
}) {
  const tone = warn
    ? "text-warn"
    : on
      ? neutral
        ? "text-accent"
        : "text-ok"
      : "text-ink-muted";
  const dot = warn ? "bg-warn" : on ? (neutral ? "bg-accent" : "bg-ok") : "bg-ink-muted";

  return (
    <GlassPanel className="flex h-full flex-col gap-3">
      <div className="flex items-center gap-2.5">
        <span className="flex h-9 w-9 items-center justify-center rounded-xl bg-white/5 text-ink-soft">
          {icon}
        </span>
        <span className="text-sm font-semibold text-ink">{title}</span>
        <span className="ml-auto flex items-center gap-1.5">
          <span className={`h-2 w-2 rounded-full ${dot} ${on ? "animate-pulse" : ""}`} />
          <span className={`text-xs font-semibold ${tone}`}>{on ? onLabel : offLabel}</span>
        </span>
      </div>
      <p className="truncate text-xs text-ink-muted">{detail}</p>
    </GlassPanel>
  );
}
