import { useShallow } from "zustand/react/shallow";

import { useHostsStore } from "../store/hostsStore";
import { useSettingsStore } from "../store/settingsStore";
import { useOnboardingStore } from "../store/onboardingStore";
import { GlassPanel } from "../design/components/GlassPanel";
import { StaggerItem } from "../design/components/Stagger";
import { LogStream } from "../design/components/LogStream";
import { Button, Chip, SectionLabel, StatusBadge } from "../design/components/atoms";
import type {
  AiRouteFailureReason,
  AiRouteHealth,
  AiRouteKind,
  AiService,
} from "../lib/tauri";

const SERVICES: { id: AiService; name: string; owner: string }[] = [
  { id: "chatgpt", name: "ChatGPT", owner: "OpenAI" },
  { id: "claude", name: "Claude", owner: "Anthropic" },
  { id: "gemini", name: "Gemini", owner: "Google" },
];

const HEALTH_LABEL: Record<AiRouteHealth, string> = {
  working: "Маршрут отвечает",
  unavailable: "Маршрут недоступен",
  inconclusive: "Проверка не завершена",
  unchecked: "Ещё не проверен",
};

const HEALTH_COLOR: Record<AiRouteHealth, string> = {
  working: "border-ok/30 bg-ok/10 text-ok",
  unavailable: "border-danger/30 bg-danger/10 text-danger",
  inconclusive: "border-warn/30 bg-warn/10 text-warn",
  unchecked: "border-glass-border bg-white/5 text-ink-muted",
};

const ROUTE_LABEL: Record<AiRouteKind, string> = {
  preferred: "основной",
  fallback: "резервный",
  direct: "прямой, без обхода",
};

const REASON_LABEL: Record<AiRouteFailureReason, string> = {
  timeout: "сервер не ответил вовремя",
  tls: "не удалось установить защищённое соединение",
  dns: "ошибка разрешения адреса",
  routeMissing: "оба источника маршрута недоступны",
  offline: "нет подтверждённого доступа к сети",
  externalChange: "hosts изменён другой программой",
};

const STATUS_LABEL: Record<string, string> = {
  installed: "Установлено (актуально)",
  outdated: "Установлено (есть обновление)",
  not_installed: "Не установлено",
  offline: "Установлено (нет сети для проверки)",
  external: "Изменён другой программой",
};

const STATUS_COLOR: Record<string, string> = {
  installed: "text-ok",
  outdated: "text-warn",
  not_installed: "text-ink-muted",
  offline: "text-ink-soft",
  external: "text-warn",
};

export function AiScreen() {
  const launchRepair = useOnboardingStore((state) => state.launchRepair);
  const protectedHostsAvailable = useSettingsStore(
    (state) => state.protectedRuntime.hosts,
  );
  const s = useHostsStore(useShallow((state) => ({
    provider: state.provider,
    status: state.status,
    localVersion: state.localVersion,
    remoteVersion: state.remoteVersion,
    busy: state.busy,
    error: state.error,
    rollbackAvailable: state.rollbackAvailable,
    health: state.health,
    setProvider: state.setProvider,
    checkRoutes: state.checkRoutes,
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
            <SectionLabel>Предпочтительный источник</SectionLabel>
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
              <strong>GeoHide:</strong> GeoIP-обход, иногда медленнее. Если
              выбранный источник не отвечает для отдельного сервиса, runtime
              подставляет проверенный маршрут второго источника.
            </p>
          </div>

          <div>
            <SectionLabel>Проверяемые маршруты</SectionLabel>
            <div className="grid grid-cols-3 gap-2">
              {SERVICES.map((service) => {
                const route = s.health?.services.find(
                  (entry) => entry.service === service.id,
                );
                const health = route?.health ?? "unchecked";
                return (
                  <div
                    key={service.id}
                    className={`rounded-xl border p-3 ${HEALTH_COLOR[health]}`}
                  >
                    <div className="flex items-start justify-between gap-2">
                      <div>
                        <div className="text-sm font-semibold text-ink">
                          {service.name}
                        </div>
                        <div className="text-3xs text-ink-muted">{service.owner}</div>
                      </div>
                      <span className="mt-1 h-2 w-2 shrink-0 rounded-full bg-current" />
                    </div>
                    <div className="mt-3 text-xs font-medium">
                      {HEALTH_LABEL[health]}
                    </div>
                    <div className="mt-1 min-w-0 break-words text-3xs leading-4 text-ink-muted">
                      {route
                        ? `${ROUTE_LABEL[route.route]}${route.provider ? ` · ${route.provider === "malw" ? "Malw" : "GeoHide"}` : ""}`
                        : "источник пока не определён"}
                    </div>
                    {route?.reason && (
                      <div className="mt-1 text-3xs leading-4 text-current/80">
                        {REASON_LABEL[route.reason]}
                      </div>
                    )}
                  </div>
                );
              })}
            </div>
            <p className="mt-2 text-3xs leading-4 text-ink-muted">
              Проверяется HTTPS-маршрут без аккаунта, cookies и содержимого
              переписки. Это не проверка всех функций внутри сервиса.
            </p>
          </div>

          {!protectedHostsAvailable && (
            <div className="flex items-center justify-between gap-3 rounded-xl border border-warn/40 bg-warn/10 px-3 py-2 text-xs leading-relaxed text-warn">
              <span>
                Защищённая служба hosts недоступна или использует несовместимый
                protocol. Установка заблокирована до восстановления runtime.
              </span>
              <Button
                variant="ghost"
                className="shrink-0"
                onClick={() => void launchRepair()}
              >
                Восстановить
              </Button>
            </div>
          )}

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
            <Button
              disabled={s.busy || !protectedHostsAvailable}
              onClick={() => s.install()}
              className="w-full"
            >
              {installed
                ? s.health?.repairRecommended
                  ? "Исправить конфигурацию"
                  : "Переустановить / обновить"
                : "Установить"}
            </Button>
            <div className="flex gap-2">
              <Button
                variant="ghost"
                disabled={s.busy || !installed || !protectedHostsAvailable}
                onClick={() => s.uninstall()}
                className="flex-1"
              >
                Удалить
              </Button>
              <Button
                variant="ghost"
                disabled={s.busy}
                onClick={() => s.checkRoutes(0)}
                className="flex-1"
              >
                Проверить маршруты
              </Button>
            </div>
            {s.rollbackAvailable && (
              <Button
                variant="ghost"
                disabled={s.busy || !protectedHostsAvailable}
                onClick={() => s.restore()}
                className="w-full"
              >
                Вернуть проверенную конфигурацию
              </Button>
            )}
          </div>

          <p className="text-xs leading-relaxed text-ink-muted">
            Обход изменяет системный файл hosts только после явного действия:
            снимок → выбор маршрутов → запись → системная HTTPS-проверка.
            Неожиданный сбой записи или post-write проверки возвращает точный
            предыдущий файл. Фоновая проверка никогда не переписывает hosts.
          </p>
        </GlassPanel>

        <GlassPanel className="flex flex-col overflow-hidden">
          <LogStream height={520} />
        </GlassPanel>
      </div>
    </div>
  );
}
