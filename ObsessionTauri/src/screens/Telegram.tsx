import { useEffect, useState } from "react";
import QRCode from "qrcode";
import { useShallow } from "zustand/react/shallow";

import { useProxyStore } from "../store/proxyStore";
import { useSettingsStore } from "../store/settingsStore";
import { GlassPanel } from "../design/components/GlassPanel";
import { StaggerItem } from "../design/components/Stagger";
import { LogStream } from "../design/components/LogStream";
import { HeroCore } from "../design/components/HeroCore";
import { Parallax } from "../design/parallax";
import {
  Button,
  SectionLabel,
  Select,
  StatusBadge,
  TextField,
} from "../design/components/atoms";
import { Uptime } from "../design/components/Uptime";
import { Icon } from "../design/components/icons";
import { resolveTelegramQrLink } from "./telegramQrLink";

const FAKE_TLS_PRESETS = ["", "www.google.com", "www.bing.com", "www.cloudflare.com"];

// Пресеты таймаута LAN-публикации: подпись → секунды (0 = без авто-закрытия).
const LAN_TIMEOUT_OPTIONS: { label: string; secs: number }[] = [
  { label: "Без авто-закрытия", secs: 0 },
  { label: "5 минут", secs: 300 },
  { label: "15 минут", secs: 900 },
  { label: "1 час", secs: 3600 },
];

/// Оставшиеся секунды до авто-закрытия LAN-публикации (тикает раз в секунду).
/// null = публикация без таймаута или неактивна.
function useLanCountdown(expiryUnix: number | null): number | null {
  const [remaining, setRemaining] = useState<number | null>(null);
  useEffect(() => {
    if (expiryUnix == null) {
      setRemaining(null);
      return;
    }
    const tick = () =>
      setRemaining(Math.max(0, expiryUnix - Math.floor(Date.now() / 1000)));
    tick();
    const id = setInterval(tick, 1000);
    return () => clearInterval(id);
  }, [expiryUnix]);
  return remaining;
}

function formatMMSS(secs: number): string {
  const m = Math.floor(secs / 60);
  const s = secs % 60;
  return `${m}:${String(s).padStart(2, "0")}`;
}

export function TelegramScreen() {
  const s = useProxyStore(useShallow((state) => ({
    available: state.available,
    running: state.running,
    transitioning: state.transitioning,
    link: state.link,
    lanLink: state.lanLink,
    lanPublished: state.lanPublished,
    lanExpiryUnix: state.lanExpiryUnix,
    port: state.port,
    fakeTlsDomain: state.fakeTlsDomain,
    error: state.error,
    copied: state.copied,
    setPort: state.setPort,
    setFakeTlsDomain: state.setFakeTlsDomain,
    start: state.start,
    stop: state.stop,
    closeLan: state.closeLan,
    copy: state.copy,
    open: state.open,
  })));
  const settings = useSettingsStore((st) => st.settings);
  const proxyLanFirewallAvailable = useSettingsStore(
    (st) => st.protectedRuntime.proxyLanFirewall,
  );
  const patchSettings = useSettingsStore((st) => st.patch);
  const [qr, setQr] = useState<string | null>(null);
  const lanRemaining = useLanCountdown(s.lanExpiryUnix);
  const lanTimeoutSecs = settings?.lan_publish_secs ?? 0;
  const canStart = s.available;

  // QR-код для телефона: LAN-ссылка (с LAN IP), не 127.0.0.1. Сохраняем
  // нативную tg://proxy-схему, чтобы совместимый сканер передал ссылку сразу
  // установленному Telegram, не открывая промежуточную страницу t.me.
  const rawLink = s.lanLink ?? s.link;
  const qrLink = resolveTelegramQrLink(rawLink);
  useEffect(() => {
    if (!qrLink) {
      setQr(null);
      return;
    }
    // cancelled-гвард ловит и unmount, и гонку порядка ответов: при быстрой смене
    // qrLink (рестарт прокси / приход lanLink) cleanup прошлого прогона отменит
    // его stale-энкод — он не перетрёт свежий QR и не сработает после unmount.
    let cancelled = false;
    QRCode.toDataURL(qrLink, { width: 200 })
      .then((url) => {
        if (!cancelled) setQr(url);
      })
      .catch(() => {
        if (!cancelled) setQr(null);
      });
    return () => {
      cancelled = true;
    };
  }, [qrLink]);

  return (
    <div className="flex h-full flex-col gap-4">
      <StaggerItem standalone className="flex items-center justify-between">
        <div>
          <h1 className="font-display text-3xl font-semibold tracking-tight text-gradient">Telegram-прокси</h1>
          <p className="text-sm text-ink-muted">
            MTProto-прокси через TgWsProxy в один клик
          </p>
        </div>
        <div className="flex items-center gap-2">
          <Uptime active={s.running} />
          <StatusBadge active={s.running} labelOn="Запущен" labelOff="Остановлен" />
        </div>
      </StaggerItem>

      <div className="screen-split screen-split--end-360">
        <GlassPanel scroll contentClassName="flex flex-col items-center gap-6">
          <div className="mt-2 flex flex-col items-center gap-4">
            <Parallax depth={18}>
              <HeroCore
                active={s.running}
                busy={s.transitioning || (!s.running && !canStart)}
                onClick={() => {
                  if (s.running) void s.stop();
                  else if (canStart) void s.start();
                }}
                size={220}
              />
            </Parallax>
            <div className="text-center text-sm text-ink-soft">
              {s.running
                ? "Прокси работает"
                : s.available
                  ? "Нажмите, чтобы запустить"
                  : "tg_ws_proxy.exe отсутствует или повреждён"}
            </div>
          </div>

          {!s.running && !canStart && (
            <GlassPanel className="border-warn/30">
              <p className="text-sm text-warn">
                tg_ws_proxy.exe не найден в защищённых ресурсах или не прошёл
                проверку runtime-manifest.
              </p>
              <p className="text-xs text-ink-muted mt-2">
                Переустановите Obsession, чтобы восстановить точный бинарник и его
                запись SHA-256.
              </p>
            </GlassPanel>
          )}

          {s.error && (
            <div className="w-full rounded-xl border border-danger/40 bg-danger/10 px-3 py-2 text-xs text-danger">
              {s.error}
            </div>
          )}

          <div className="grid w-full grid-cols-2 gap-3">
            <div className="flex flex-col gap-1.5">
              <SectionLabel>Порт</SectionLabel>
              <TextField
                type="number"
                value={String(s.port)}
                onChange={(v) => s.setPort(Number(v) || 1443)}
              />
            </div>
            <div className="flex flex-col gap-1.5">
              <SectionLabel>Fake TLS домен</SectionLabel>
              <Select
                value={s.fakeTlsDomain}
                options={FAKE_TLS_PRESETS}
                placeholder="Без домена"
                onChange={(v) => s.setFakeTlsDomain(v)}
              />
            </div>
          </div>

          <div className="w-full">
            <SectionLabel>Авто-закрытие доступа с телефона</SectionLabel>
            <Select
              value={
                LAN_TIMEOUT_OPTIONS.find((o) => o.secs === lanTimeoutSecs)?.label ??
                LAN_TIMEOUT_OPTIONS[0].label
              }
              options={LAN_TIMEOUT_OPTIONS.map((o) => o.label)}
              onChange={(label) => {
                const opt = LAN_TIMEOUT_OPTIONS.find((o) => o.label === label);
                if (opt) patchSettings({ lan_publish_secs: opt.secs });
              }}
            />
            <p className="mt-1 text-xs text-ink-muted">
              По истечении доступ с телефона закрывается (форвардер и правило
              брандмауэра снимаются). Прокси для Telegram Desktop продолжает работать.
            </p>
            {!proxyLanFirewallAvailable && (
              <p className="mt-2 text-xs text-warn">
                Защищённая LAN-публикация сейчас недоступна. Локальный прокси для
                Telegram Desktop можно запускать; QR и доступ с телефона останутся
                выключены.
              </p>
            )}
          </div>

          {/* Ссылка. */}
          {s.link && (
            <div className="w-full">
              <SectionLabel>Ссылка tg://proxy</SectionLabel>
              <div className="rounded-xl border border-glass-border bg-black/30 p-3 font-mono text-2xs break-all text-accent-cyan">
                {s.link}
              </div>
              <div className="mt-2 flex gap-2">
                <Button onClick={() => s.open()} className="flex-1">
                  Открыть в Telegram
                </Button>
                <Button variant="ghost" onClick={() => s.copy()}>
                  <span className="flex items-center gap-1.5">
                    {s.copied ? <Icon.Check size={15} /> : <Icon.Copy size={15} />}
                    {s.copied ? "Скопировано" : "Копировать"}
                  </span>
                </Button>
              </div>

              {qr && s.lanPublished && (
                <GlassPanel className="mt-4 flex flex-col items-center gap-2">
                  <img src={qr} alt="QR-код" className="rounded-lg" />
                  <span className="text-xs text-ink-muted">
                    {s.lanLink ? "Отсканируйте телефоном (та же Wi-Fi сеть)" : "Отсканируйте телефоном"}
                  </span>
                  <div className="mt-1 flex w-full items-center justify-between gap-2 border-t border-glass-border pt-2">
                    <span className="text-xs text-ink-soft">
                      Доступ с телефона открыт
                      {lanRemaining != null && (
                        <> · закроется через <span className="font-mono text-accent-cyan">{formatMMSS(lanRemaining)}</span></>
                      )}
                    </span>
                    <Button variant="ghost" onClick={() => s.closeLan()}>
                      Закрыть доступ
                    </Button>
                  </div>
                </GlassPanel>
              )}
              {s.running && !s.lanPublished && s.lanLink === null && (
                <p className="mt-3 text-xs text-ink-muted">
                  {proxyLanFirewallAvailable
                    ? "Доступ с телефона закрыт — прокси работает локально для Telegram Desktop."
                    : "LAN firewall capability недоступна — прокси работает только локально для Telegram Desktop."}
                </p>
              )}
            </div>
          )}
        </GlassPanel>

        <GlassPanel className="flex flex-col overflow-hidden">
          <LogStream />
        </GlassPanel>
      </div>
    </div>
  );
}
