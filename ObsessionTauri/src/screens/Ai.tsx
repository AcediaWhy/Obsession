import { useShallow } from "zustand/react/shallow";

import { useHostsStore } from "../store/hostsStore";
import { GlassPanel } from "../design/components/GlassPanel";
import { StaggerItem } from "../design/components/Stagger";
import { LogStream } from "../design/components/LogStream";
import { Button, Chip, SectionLabel, StatusBadge } from "../design/components/atoms";

const UNBLOCKED_SERVICES = [
  "ChatGPT (OpenAI)",
  "Claude (Anthropic)",
  "Gemini (Google)",
  "Perplexity AI",
  "Poe",
  "HuggingFace",
  "Midjourney",
];

const STATUS_LABEL: Record<string, string> = {
  installed: "Установлено (актуально)",
  outdated: "Установлено (есть обновление)",
  not_installed: "Не установлено",
  offline: "Установлено (нет сети для проверки)",
};

const STATUS_COLOR: Record<string, string> = {
  installed: "text-ok",
  outdated: "text-warn",
  not_installed: "text-ink-muted",
  offline: "text-ink-soft",
};

export function AiScreen() {
  const s = useHostsStore(useShallow((state) => ({
    provider: state.provider,
    status: state.status,
    localVersion: state.localVersion,
    remoteVersion: state.remoteVersion,
    busy: state.busy,
    error: state.error,
    rollbackAvailable: state.rollbackAvailable,
    setProvider: state.setProvider,
    refresh: state.refresh,
    install: state.install,
    uninstall: state.uninstall,
    restore: state.restore,
  })));
  const installed = s.status !== "not_installed";

  return (
    <div className="flex h-full flex-col gap-4">
      <StaggerItem standalone className="flex items-center justify-between">
        <div>
          <h1 className="font-display text-3xl font-semibold tracking-tight text-gradient">ИИ-разблокировка</h1>
          <p className="text-sm text-ink-muted">
            Доступ к ИИ-сервисам через системный hosts
          </p>
        </div>
        <StatusBadge active={installed} labelOn="Установлено" labelOff="Не установлено" />
      </StaggerItem>

      <div className="grid flex-1 grid-cols-[1fr_360px] gap-4 overflow-hidden">
        <GlassPanel scroll contentClassName="flex flex-col gap-6">
          <div>
            <SectionLabel>Провайдер DNS</SectionLabel>
            <div className="flex gap-2">
              <Chip
                label="Malw (dns.malw.link)"
                active={s.provider === "malw"}
                disabled={s.busy}
                onClick={() => s.setProvider("malw")}
              />
              <Chip
                label="GeoHide"
                active={s.provider === "geohide"}
                disabled={s.busy}
                onClick={() => s.setProvider("geohide")}
              />
            </div>
            <p className="text-xs text-ink-muted mt-2">
              <strong>Malw:</strong> зеркала резолвятся через DNS Cloudflare.
              {" "}
              <strong>GeoHide:</strong> GeoIP-обход, иногда медленнее.
            </p>
          </div>

          <div className="rounded-xl border border-glass-border bg-white/5 p-4">
            <span className="text-sm font-medium text-ink-soft">Разблокируемые сервисы</span>
            <div className="mt-2 flex flex-wrap gap-2">
              {UNBLOCKED_SERVICES.map((svc) => (
                <Chip key={svc} label={svc} active={false} disabled onClick={() => {}} />
              ))}
            </div>
          </div>

          <div className="rounded-xl border border-glass-border bg-white/5 p-4">
            <div className="flex items-center justify-between">
              <span className="text-sm text-ink-soft">Статус</span>
              <span className={`text-sm font-semibold ${STATUS_COLOR[s.status]}`}>
                {s.busy ? "Проверка…" : STATUS_LABEL[s.status]}
              </span>
            </div>
            {s.localVersion && (
              <div className="mt-2 text-xs text-ink-muted">
                Локальная версия: {s.localVersion}
                {s.remoteVersion && s.remoteVersion !== s.localVersion && (
                  <> · Доступна: {s.remoteVersion}</>
                )}
              </div>
            )}
          </div>

          {s.error && (
            <div className="rounded-xl border border-danger/40 bg-danger/10 px-3 py-2 text-xs text-danger">
              {s.error}
            </div>
          )}

          <div className="flex flex-col gap-2">
            <Button disabled={s.busy} onClick={() => s.install()} className="w-full">
              {installed ? "Переустановить / Обновить" : "Установить"}
            </Button>
            <div className="flex gap-2">
              <Button
                variant="ghost"
                disabled={s.busy || !installed}
                onClick={() => s.uninstall()}
                className="flex-1"
              >
                Удалить
              </Button>
              <Button
                variant="ghost"
                disabled={s.busy}
                onClick={() => s.refresh()}
                className="flex-1"
              >
                Проверить
              </Button>
            </div>
            {s.rollbackAvailable && (
              <Button
                variant="ghost"
                disabled={s.busy}
                onClick={() => s.restore()}
                className="w-full"
              >
                Вернуть рабочую версию
              </Button>
            )}
          </div>

          <p className="text-xs leading-relaxed text-ink-muted">
            Обход изменяет системный файл hosts транзакционно: снимок → запись →
            проверка. При сбое или неудачных пробах откат автоматический; кнопка
            «Вернуть рабочую версию» восстанавливает последнюю рабочую копию.
          </p>
        </GlassPanel>

        <GlassPanel className="flex flex-col overflow-hidden">
          <LogStream height={520} />
        </GlassPanel>
      </div>
    </div>
  );
}
