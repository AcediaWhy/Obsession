import { useEffect, useState } from "react";
import QRCode from "qrcode";

import { useProxyStore } from "../store/proxyStore";
import { GlassPanel } from "../design/components/GlassPanel";
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

const FAKE_TLS_PRESETS = ["", "www.google.com", "www.bing.com", "www.cloudflare.com"];

export function TelegramScreen() {
  const s = useProxyStore();
  const [qr, setQr] = useState<string | null>(null);

  // QR-код для телефона: используем LAN-ссылку (с LAN IP), не 127.0.0.1.
  const qrLink = s.lanLink ?? s.link;
  useEffect(() => {
    if (qrLink) {
      QRCode.toDataURL(qrLink, { width: 200 })
        .then(setQr)
        .catch(() => setQr(null));
    } else {
      setQr(null);
    }
  }, [qrLink]);

  return (
    <div className="flex h-full flex-col gap-4">
      <div className="flex items-center justify-between">
        <div>
          <h1 className="font-display text-3xl font-bold text-gradient">Telegram-прокси</h1>
          <p className="text-sm text-ink-muted">
            MTProto-прокси через TgWsProxy в один клик
          </p>
        </div>
        <div className="flex items-center gap-2">
          <Uptime active={s.running} />
          <StatusBadge active={s.running} labelOn="Запущен" labelOff="Остановлен" />
        </div>
      </div>

      <div className="grid flex-1 grid-cols-[1fr_360px] gap-4 overflow-hidden">
        <GlassPanel scroll contentClassName="flex flex-col items-center gap-6">
          <div className="mt-2 flex flex-col items-center gap-4">
            <Parallax depth={18}>
              <HeroCore
                active={s.running}
                busy={s.transitioning}
                onClick={() => (s.running ? s.stop() : s.start())}
                size={220}
              />
            </Parallax>
            <div className="text-center text-sm text-ink-soft">
              {s.available
                ? s.running
                  ? "Прокси работает"
                  : "Нажмите, чтобы запустить"
                : "TgWsProxy.exe не найден"}
            </div>
          </div>

          {!s.available && (
            <GlassPanel className="border-warn/30">
              <p className="text-sm text-warn">
                TgWsProxy.exe не найден в bundled-ресурсах.
              </p>
              <p className="text-xs text-ink-muted mt-2">
                Проверьте, что приложение установлено корректно.
                Переустановите Obsession или скачайте последнюю версию.
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

          {/* Ссылка. */}
          {s.link && (
            <div className="w-full">
              <SectionLabel>Ссылка tg://proxy</SectionLabel>
              <div className="rounded-xl border border-glass-border bg-black/30 p-3 font-mono text-[11px] break-all text-accent-cyan">
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

              {qr && (
                <GlassPanel className="mt-4 flex flex-col items-center gap-2">
                  <img src={qr} alt="QR-код" className="rounded-lg" />
                  <span className="text-xs text-ink-muted">
                    {s.lanLink ? "Отсканируйте телефоном (та же Wi-Fi сеть)" : "Отсканируйте телефоном"}
                  </span>
                </GlassPanel>
              )}
            </div>
          )}
        </GlassPanel>

        <GlassPanel className="flex flex-col overflow-hidden">
          <LogStream height={520} />
        </GlassPanel>
      </div>
    </div>
  );
}
