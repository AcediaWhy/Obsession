import { useState } from "react";

import { useSettingsStore } from "../store/settingsStore";
import { useThemeStore, THEMES, type Theme } from "../store/themeStore";
import { useSecretStore } from "../store/secretStore";
import { GlassPanel } from "../design/components/GlassPanel";
import { ThemePreview } from "../design/components/ThemePreview";
import { Stagger, StaggerItem } from "../design/components/Stagger";
import {
  Button,
  Row,
  Select,
  SectionLabel,
  Switch,
  TextField,
} from "../design/components/atoms";
import { Icon } from "../design/components/icons";
import { api } from "../lib/tauri";

const AI_PROVIDERS = ["malw", "geohide"];

export function SettingsScreen() {
  // Точечные селекторы (как в Overview): подписка на весь стор перерисовывала бы
  // весь экран — с ThemePicker и его канвасами — на каждый флип saving/saved
  // автосохранения. saving/saved читает сам SaveIndicator.
  const cfg = useSettingsStore((s) => s.settings);
  const error = useSettingsStore((s) => s.error);
  const elevated = useSettingsStore((s) => s.elevated);
  const protectedRuntime = useSettingsStore((s) => s.protectedRuntime);
  const protectedRuntimeComplete =
    protectedRuntime.dpi &&
    protectedRuntime.zapret2 &&
    protectedRuntime.adaptiveZapret2 &&
    protectedRuntime.eyesEvents &&
    protectedRuntime.legacyReliabilityControls &&
    protectedRuntime.legacyReliability &&
    protectedRuntime.hosts &&
    protectedRuntime.proxyLanFirewall;
  const autostart = useSettingsStore((s) => s.autostart);
  const setAutostart = useSettingsStore((s) => s.setAutostart);
  const patch = useSettingsStore((s) => s.patch);

  return (
    <div className="flex h-full flex-col gap-4">
      {/* Заголовок + индикатор автосохранения. */}
      <StaggerItem standalone className="flex items-center justify-between">
        <div>
          <h1 className="font-display text-3xl font-semibold tracking-tight text-gradient">Настройки</h1>
          <p className="text-sm text-ink-muted">Параметры приложения · сохраняются автоматически</p>
        </div>
        <SaveIndicator />
      </StaggerItem>

      {!cfg ? (
        <GlassPanel className="flex flex-1 items-center justify-center text-sm text-ink-muted">
          {error ? error : "Загрузка настроек…"}
        </GlassPanel>
      ) : (
        <div className="grid flex-1 grid-cols-2 gap-4 overflow-y-auto pr-1">
          <Stagger className="flex flex-col gap-4">
            {/* Оформление — выбор визуальной темы hero/фона. */}
            <StaggerItem glass>
              <GlassPanel>
                <SectionLabel>Оформление</SectionLabel>
                <ThemePicker />
              </GlassPanel>
            </StaggerItem>

            {/* Общие. */}
            <StaggerItem glass>
              <GlassPanel>
                <SectionLabel>Общие</SectionLabel>
                <div className="divide-y divide-white/5">
                  <Row
                    label="Автозапуск с Windows"
                    hint="Запускать Obsession при входе в систему"
                  >
                    <Switch
                      checked={autostart}
                      onChange={(v) => setAutostart(v)}
                    />
                  </Row>
                  <Row
                    label="Запускать свёрнутым"
                    hint="Стартовать сразу в трее, без открытия окна"
                  >
                    <Switch
                      checked={cfg.start_minimized}
                      onChange={(v) => patch({ start_minimized: v })}
                    />
                  </Row>
                  <Row
                    label="Сворачивать в трей"
                    hint="При закрытии окна прятать в системный трей, а не выходить"
                  >
                    <Switch
                      checked={cfg.minimize_to_tray}
                      onChange={(v) => patch({ minimize_to_tray: v })}
                    />
                  </Row>
                  <Row
                    label="Меньше анимаций"
                    hint="Отключает фоновую анимацию и эффекты"
                  >
                    <Switch
                      checked={cfg.reduce_motion}
                      onChange={(v) => patch({ reduce_motion: v })}
                    />
                  </Row>
                </div>
              </GlassPanel>
            </StaggerItem>

            {/* ИИ. */}
            <StaggerItem glass>
              <GlassPanel>
                <SectionLabel>ИИ-разблокировка</SectionLabel>
                <div className="divide-y divide-white/5">
                  <Row label="Провайдер DNS" hint="Источник hosts для доступа к ИИ-сервисам">
                    <div className="w-40">
                      <Select
                        value={cfg.ai_provider || "malw"}
                        options={AI_PROVIDERS}
                        onChange={(v) => patch({ ai_provider: v })}
                      />
                    </div>
                  </Row>
                </div>
              </GlassPanel>
            </StaggerItem>
          </Stagger>

          <Stagger className="flex flex-col gap-4">
            {/* Telegram-прокси. */}
            <StaggerItem glass>
              <GlassPanel>
                <SectionLabel>Telegram-прокси</SectionLabel>
                <div className="flex flex-col gap-3">
                  <div className="flex flex-col gap-1.5">
                    <span className="text-sm font-medium text-ink">Порт по умолчанию</span>
                    <TextField
                      type="number"
                      value={String(cfg.proxy_port)}
                      onChange={(v) => patch({ proxy_port: Number(v) || 1443 })}
                    />
                  </div>
                  <div className="flex flex-col gap-1.5">
                    <span className="text-sm font-medium text-ink">Fake TLS домен</span>
                    <TextField
                      value={cfg.fake_tls_domain}
                      placeholder="напр. www.google.com"
                      onChange={(v) => patch({ fake_tls_domain: v })}
                    />
                  </div>
                </div>
              </GlassPanel>
            </StaggerItem>

            {/* Система (только чтение). */}
            <StaggerItem glass>
              <GlassPanel>
                <SectionLabel>Система</SectionLabel>
                <div className="divide-y divide-white/5">
                  <Row
                    label="Режим интерфейса"
                    hint="Obsession должна работать без постоянных прав администратора"
                  >
                    <span
                      className={`text-xs font-semibold ${elevated ? "text-warn" : "text-ok"}`}
                    >
                      {elevated ? "Повышенный" : "Обычный"}
                    </span>
                  </Row>
                  <Row
                    label="Защищённый runtime"
                    hint="Отдельный системный компонент для DPI, hosts и сетевых утилит"
                  >
                    <span
                      className={`text-xs font-semibold ${
                        protectedRuntimeComplete ? "text-ok" : "text-warn"
                      }`}
                    >
                      {!protectedRuntime.serviceAvailable
                        ? "Недоступен"
                        : protectedRuntimeComplete
                          ? "Работает"
                          : "Работает частично"}
                    </span>
                  </Row>
                  <Row label="Онбординг" hint="Приветственный экран первого запуска">
                    {cfg.has_completed_onboarding ? (
                      <Button variant="ghost" onClick={() => patch({ has_completed_onboarding: false })}>
                        Показать снова
                      </Button>
                    ) : (
                      <span className="text-xs font-semibold text-ink-soft">Не пройден</span>
                    )}
                  </Row>
                  <HotkeyRow />
                </div>
              </GlassPanel>
            </StaggerItem>

            {/* Окошко пасхалок. */}
            <StaggerItem glass>
              <GlassPanel>
                <SectionLabel>· · ·</SectionLabel>
                <SecretBox />
              </GlassPanel>
            </StaggerItem>
          </Stagger>
        </div>
      )}
    </div>
  );
}

// Окошко пасхалок: ввод кодового слова открывает скрытые бонусы. Если награда —
// тема, сразу переключаемся на неё, чтобы бонус было видно немедленно.
function SecretBox() {
  const redeem = useSecretStore((s) => s.redeem);
  const setTheme = useThemeStore((s) => s.setTheme);
  const [value, setValue] = useState("");
  const [msg, setMsg] = useState<{ tone: "ok" | "dim" | "err"; text: string } | null>(null);

  const submit = () => {
    const raw = value.trim();
    if (!raw) return;
    const res = redeem(raw);
    if (res.status === "unlocked") {
      // Если id награды совпадает с секретной темой — применяем её.
      const themed = THEMES.find((t) => t.secret === res.id);
      if (themed) setTheme(themed.id);
      setMsg({ tone: "ok", text: `Открыто: ${res.title}` });
      setValue("");
    } else if (res.status === "already") {
      setMsg({ tone: "dim", text: `Уже открыто: ${res.title}` });
    } else {
      setMsg({ tone: "err", text: "Ничего не происходит…" });
    }
  };

  const toneClass =
    msg?.tone === "ok" ? "text-ok" : msg?.tone === "err" ? "text-ink-muted" : "text-ink-soft";

  return (
    <div className="flex flex-col gap-2">
      <p className="text-xs text-ink-muted">Кодовое слово?</p>
      <div className="flex gap-2">
        <div className="flex-1">
          <TextField
            value={value}
            placeholder="……"
            onChange={(v) => {
              setValue(v);
              if (msg) setMsg(null);
            }}
          />
        </div>
        <Button variant="ghost" disabled={!value.trim()} onClick={submit}>
          OK
        </Button>
      </div>
      {msg && <span className={`text-xs ${toneClass}`}>{msg.text}</span>}
    </div>
  );
}

// Выбор темы живыми плитками-превью: каждая крутит свой hero в мини-масштабе.
// Скрытые темы (с полем secret) показываются только после разблокировки.
// Все превью живут постоянно: после спрайтовой оптимизации шесть мини-ядер
// стоят копейки, а механика «замри, если не выбран/не наведён» давала уродливые
// стоп-кадры при переключении темы.
function ThemePicker() {
  const theme = useThemeStore((s) => s.theme);
  const setTheme = useThemeStore((s) => s.setTheme);
  const unlocked = useSecretStore((s) => s.unlocked);

  const visible = THEMES.filter((th) => !th.secret || unlocked.includes(th.secret));

  return (
    <div className="grid grid-cols-2 gap-3">
      {visible.map((th) => (
        <ThemeTile
          key={th.id}
          id={th.id}
          label={th.label}
          selected={theme === th.id}
          onSelect={() => setTheme(th.id)}
        />
      ))}
    </div>
  );
}

function ThemeTile({
  id,
  label,
  selected,
  onSelect,
}: {
  id: Theme;
  label: string;
  selected: boolean;
  onSelect: () => void;
}) {
  return (
    <button
      type="button"
      onClick={onSelect}
      aria-pressed={selected}
      aria-label={`Тема «${label}»${selected ? ", выбрана" : ""}`}
      className={[
        "no-drag group relative flex flex-col items-center gap-2 rounded-xl border p-3 transition-[color,background-color,border-color,box-shadow,opacity]",
        "focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent/70",
        selected
          ? "border-accent/60 bg-accent/10 shadow-glow"
          : "border-glass-border bg-white/5 hover:bg-white/10",
      ].join(" ")}
    >
      {/* Статичное декоративное preview: внутри плитки нет второй кнопки/canvas. */}
      <div className="pointer-events-none flex h-[104px] items-center justify-center">
        <ThemePreview theme={id} selected={selected} size={104} />
      </div>
      <div className="flex items-center gap-1.5">
        {selected && <Icon.Check size={14} />}
        <span className={`text-sm font-semibold ${selected ? "text-ink" : "text-ink-soft"}`}>
          {label}
        </span>
      </div>
    </button>
  );
}

// Бейдж автосохранения подписан на saving/saved сам: их флипы (saving → saved →
// таймаут) перерисовывают только его, а не весь экран с превью-канвасами.
function SaveIndicator() {
  const saving = useSettingsStore((s) => s.saving);
  const saved = useSettingsStore((s) => s.saved);
  return (
    <div className="flex items-center gap-2 rounded-full bg-white/5 px-3 py-1.5 text-xs font-medium">
      {saving ? (
        <>
          <span className="h-2 w-2 animate-pulse rounded-full bg-accent-cyan" />
          <span className="text-ink-soft">Сохранение…</span>
        </>
      ) : saved ? (
        <>
          <Icon.Check size={14} />
          <span className="text-ok">Сохранено</span>
        </>
      ) : (
        <span className="text-ink-muted">Автосохранение</span>
      )}
    </div>
  );
}

// ─── Настраиваемый глобальный хоткей ──────────────────────────────────────────
// Хранится как Tauri-акселератор с Code-именем клавиши (то, что даёт
// KeyboardEvent.code): "Ctrl+Shift+KeyO". Захват читает модификаторы + code
// напрямую, поэтому формат всегда совпадает с тем, что парсит бэкенд.
const DEFAULT_HOTKEY = "Ctrl+Shift+KeyO";

const MOD_LABEL: Record<string, string> = { Ctrl: "Ctrl", Alt: "Alt", Shift: "Shift", Super: "Win" };
const ARROW_LABEL: Record<string, string> = { ArrowUp: "↑", ArrowDown: "↓", ArrowLeft: "←", ArrowRight: "→" };
// Чистые модификаторы: ждём, пока к ним не добавят основную клавишу.
const MODIFIER_CODES = new Set([
  "ControlLeft", "ControlRight", "ShiftLeft", "ShiftRight",
  "AltLeft", "AltRight", "MetaLeft", "MetaRight", "OSLeft", "OSRight",
]);

// Человекочитаемое сочетание: "Ctrl+Shift+KeyO" → "Ctrl + Shift + O".
function formatHotkey(accel: string): string {
  if (!accel) return "выключен";
  return accel
    .split("+")
    .map((t) => {
      if (MOD_LABEL[t]) return MOD_LABEL[t];
      if (t.startsWith("Key")) return t.slice(3);
      if (t.startsWith("Digit")) return t.slice(5);
      if (t.startsWith("Numpad")) return "Num " + t.slice(6);
      return ARROW_LABEL[t] ?? t; // F5, Space, Enter, Tab, …
    })
    .join(" + ");
}

function HotkeyRow() {
  const current = useSettingsStore((s) => s.settings?.hotkey_toggle ?? "");
  const [capturing, setCapturing] = useState(false);
  const [err, setErr] = useState("");

  const commit = async (accel: string) => {
    setCapturing(false);
    setErr("");
    try {
      await api.setHotkey(accel);
      // Синхронизируем локальную копию настроек (бэкенд уже персистнул).
      const s = useSettingsStore.getState().settings;
      if (s) useSettingsStore.setState({ settings: { ...s, hotkey_toggle: accel } });
    } catch (e) {
      setErr(String(e));
    }
  };

  return (
    <Row label="Горячая клавиша" hint="Вкл/выкл защиты — работает даже из трея">
      <div className="flex flex-col items-end gap-1">
        {capturing ? (
          <button
            autoFocus
            onBlur={() => {
              setCapturing(false);
              setErr("");
            }}
            onKeyDown={(e) => {
              e.preventDefault();
              e.stopPropagation();
              if (e.code === "Escape") {
                setCapturing(false);
                setErr("");
                return;
              }
              if (MODIFIER_CODES.has(e.code)) return; // ждём основную клавишу
              const mods: string[] = [];
              if (e.ctrlKey) mods.push("Ctrl");
              if (e.altKey) mods.push("Alt");
              if (e.shiftKey) mods.push("Shift");
              if (e.metaKey) mods.push("Super");
              if (mods.length === 0) {
                setErr("Добавьте Ctrl, Alt, Shift или Win");
                return;
              }
              void commit([...mods, e.code].join("+"));
            }}
            className="no-drag animate-pulse rounded-lg border border-accent/60 bg-accent/10 px-3 py-1.5 text-xs text-ink focus-visible:outline-none"
          >
            Нажмите сочетание…
          </button>
        ) : (
          <button
            onClick={() => {
              setCapturing(true);
              setErr("");
            }}
            className="no-drag rounded-lg border border-glass-border bg-white/5 px-3 py-1.5 font-mono text-xs text-ink-soft transition-colors hover:bg-white/10 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent/70"
          >
            {formatHotkey(current)}
          </button>
        )}
        {err ? (
          <span className="max-w-[190px] text-right text-3xs text-danger">{err}</span>
        ) : !capturing && current !== DEFAULT_HOTKEY ? (
          <button
            onClick={() => void commit(DEFAULT_HOTKEY)}
            className="no-drag text-3xs text-ink-muted transition-colors hover:text-ink-soft focus-visible:outline-none"
          >
            сбросить по умолчанию
          </button>
        ) : null}
      </div>
    </Row>
  );
}
