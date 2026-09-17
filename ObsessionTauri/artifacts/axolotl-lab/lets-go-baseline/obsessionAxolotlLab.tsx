import { useCallback, useState } from "react";
import ReactDOM from "react-dom/client";

import { Button, Chip, SectionLabel, Select, StatusBadge } from "./design/components/atoms";
import { InspectorStrip } from "./design/components/axolotl/InspectorStrip";
import { Icon } from "./design/components/axolotl/pixelIcons";
import { type PlaneIndex } from "./design/components/pixelart/pixelCore";
import { PixelLogFeed } from "./design/components/axolotl/PixelLogFeed";
import { labToast, PixelToaster } from "./design/components/axolotl/PixelToaster";
import { GlassPanel } from "./design/components/GlassPanel";
import { type FieldMetrics, SunkenStarFieldLab } from "./design/components/SunkenStarFieldLab";
import { SunkenStarSpriteLab } from "./design/components/SunkenStarSpriteLab";
import type { ObsessionVisualPhase } from "./design/obsessionVisualState";
import { ParallaxProvider } from "./design/parallax";
import "./styles/fonts.css";
import "./styles/globals.css";
import "./styles/obsessionAxolotlLab.css";

// Хром лабы — прежний стеклянный язык приложения: GlassPanel, общие атомы,
// скруглённые плашки. Переделке подверглись только мир (вечер на крыше в низком
// арт-буфере с полосами, дизерингом и одной луной) и лабораторная полоса
// инспектора под макетом. Иконки берём из lucide-react с теми же именами, что у
// общего набора.

type TabId = "overview" | "dpi" | "ai" | "telegram" | "lists" | "profiles" | "settings";

const PHASE_COPY: Record<ObsessionVisualPhase, string> = {
  idle: "Активируйте ядро",
  engaging: "Поднимаем защищённое течение…",
  scanning: "Кот слушает крыши — ищем чистый маршрут…",
  focused: "Обход активен · хранитель держит поток",
  fault: "Течение нарушено · ищем обход",
};

const NAV_ITEMS: ReadonlyArray<{ id: TabId; label: string; icon: typeof Icon.Shield }> = [
  { id: "overview", label: "Обзор", icon: Icon.Shield },
  { id: "dpi", label: "DPI-обход", icon: Icon.Bolt },
  { id: "ai", label: "ИИ-разблокировка", icon: Icon.Robot },
  { id: "telegram", label: "Telegram", icon: Icon.Send },
  { id: "lists", label: "Списки", icon: Icon.List },
  { id: "profiles", label: "Профили", icon: Icon.Layers },
  { id: "settings", label: "Настройки", icon: Icon.Settings },
];

function LabTitleBar() {
  return (
    <div className="theme-morph drag-region flex h-10 items-center justify-between px-4">
      <div className="flex items-center gap-2">
        <div className="h-2.5 w-2.5 rounded-full bg-accent shadow-glow" />
        <span className="text-xs font-semibold tracking-wide text-ink-soft">Obsession</span>
      </div>
      <div className="no-drag flex items-center gap-1" aria-hidden="true">
        <span className="sunken-lab-window-glyph">−</span>
        <span className="sunken-lab-window-glyph">□</span>
        <span className="sunken-lab-window-glyph sunken-lab-window-glyph--close">×</span>
      </div>
    </div>
  );
}

function LabNavRail({
  activeTab,
  onSelectTab,
  phase,
  paused,
}: {
  activeTab: TabId;
  onSelectTab: (id: TabId) => void;
  phase: ObsessionVisualPhase;
  paused: boolean;
}) {
  return (
    <nav className="app-nav-rail flex w-[220px] flex-col px-4 pb-4 pt-2" aria-label="Навигация Obsession">
      <div className="app-nav-brand mb-8 flex items-center gap-3 px-2">
        <div className="sunken-lab-brand-core">
          <SunkenStarSpriteLab detail="base" paused={paused} phase={phase} size={64} />
        </div>
        <div className="leading-tight">
          <div className="wordmark font-display text-lg font-semibold tracking-tight text-ink">Obsession</div>
          <div className="font-mono text-2xs tabular-nums tracking-widest text-ink-muted">V1.1.0</div>
        </div>
      </div>

      <div className="app-nav-section-label mb-2 px-2 text-3xs font-semibold uppercase tracking-[0.14em] text-ink-muted">
        Меню
      </div>
      <div className="relative flex flex-col gap-1">
        {NAV_ITEMS.map((item) => {
          const isActive = activeTab === item.id;
          return (
            <button
              className="nav-item no-drag theme-morph group relative flex items-center gap-3 rounded-xl px-3 py-2.5 text-sm transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent/70"
              data-active={isActive || undefined}
              key={item.id}
              onClick={() => onSelectTab(item.id)}
              type="button"
            >
              {isActive && (
                <span
                  aria-hidden
                  className="nav-active-plate absolute inset-0 rounded-xl border border-accent/40 bg-accent/15 shadow-glow"
                />
              )}
              <span
                className={`relative z-10 flex items-center gap-3 ${isActive ? "text-accent-cyan" : "text-ink-muted"}`}
              >
                <item.icon size={20} />
                <span className={`whitespace-nowrap font-medium ${isActive ? "text-ink" : "text-ink-soft"}`}>
                  {item.label}
                </span>
              </span>
            </button>
          );
        })}
      </div>
      <div className="app-nav-footer mt-auto px-2 text-3xs text-ink-muted opacity-70">made by AcediaWhy</div>
    </nav>
  );
}

function HeroCore({
  phase,
  paused,
  size,
  onToggle,
}: {
  phase: ObsessionVisualPhase;
  paused: boolean;
  size: number;
  onToggle: () => void;
}) {
  return (
    <button aria-label="Разбудить кота" className="sunken-lab-hero-button" onClick={onToggle} type="button">
      {/* Размер кратен 64 И совпадает с шагом сетки мира: иначе у кота в панели
          пиксели крупнее, чем у крыши за ней, и два разрешения дерутся в одном
          кадре. Именно это и делало прежние версии «нарисованными поверх». */}
      <SunkenStarSpriteLab detail="hero" paused={paused} phase={phase} size={size} />
    </button>
  );
}

function OverviewScreen({
  phase,
  paused,
  active,
  heroSize,
  onToggle,
  onScan,
}: {
  phase: ObsessionVisualPhase;
  paused: boolean;
  active: boolean;
  heroSize: number;
  onToggle: () => void;
  onScan: () => void;
}) {
  return (
    <div className="flex h-full min-h-0 flex-col gap-4">
      <header className="flex items-center justify-between">
        <div>
          <h1 className="font-display text-3xl font-semibold tracking-tight text-gradient">Обзор</h1>
          <p className="text-sm text-ink-muted">Состояние защиты одним взглядом</p>
        </div>
        <StatusBadge active={active} />
      </header>

      {/* Порог md, а не lg: боевое окно Obsession живёт около 994 px, и на lg (1024)
          сетка сваливалась в одну колонку — чего в самом приложении не происходит. */}
      <div className="sunken-lab-stage grid min-h-0 grid-cols-1 gap-4 md:grid-cols-12">
        {/* GlassPanel применяет contentClassName только при scroll, поэтому раскладку
            держим своей обёрткой — иначе содержимое липнет к левому верхнему углу. */}
        <GlassPanel className="sunken-lab-hero-panel md:col-span-7">
          <div className="flex h-full w-full flex-col items-center justify-center gap-5">
            <HeroCore onToggle={onToggle} paused={paused} phase={phase} size={heroSize} />
            <div className="text-center">
              <div className="font-display text-xl font-medium text-ink">{PHASE_COPY[phase]}</div>
              <p className="mt-1 text-xs text-ink-muted">
                {active ? "Служба Zapret держит соединение стабильным" : "Позовите кота, чтобы включить защиту"}
              </p>
            </div>
            <div className="flex gap-3">
              <Button onClick={onToggle} variant={active ? "ghost" : "primary"}>
                {active ? "Отключить защиту" : "Активировать поток"}
              </Button>
              <Button onClick={onScan} variant="ghost">
                <span className="flex items-center gap-1.5">
                  <Icon.Refresh size={16} /> Проверить DPI
                </span>
              </Button>
            </div>
          </div>
        </GlassPanel>

        <div className="flex min-h-0 flex-col gap-4 md:col-span-5">
          <GlassPanel>
            <div className="flex flex-col gap-3">
              <SectionLabel>Службы защиты</SectionLabel>
              <div className="flex items-center justify-between rounded-xl bg-white/5 p-3">
                <div className="flex items-center gap-3">
                  <div className="flex h-9 w-9 items-center justify-center rounded-lg bg-accent/20 text-accent">
                    <Icon.Bolt size={20} />
                  </div>
                  <div>
                    <div className="text-sm font-semibold text-ink">DPI-обход (Zapret2)</div>
                    <div className="text-2xs text-ink-muted">профиль general-alt2</div>
                  </div>
                </div>
                <span className={`text-xs font-semibold ${active ? "text-ok" : "text-ink-muted"}`}>
                  {active ? "активно" : "пауза"}
                </span>
              </div>

              <div className="flex items-center justify-between rounded-xl bg-white/5 p-3">
                <div className="flex items-center gap-3">
                  <div className="flex h-9 w-9 items-center justify-center rounded-lg bg-accent-cyan/20 text-accent-cyan">
                    <Icon.Shield size={20} />
                  </div>
                  <div>
                    <div className="text-sm font-semibold text-ink">DNS-криптография</div>
                    <div className="text-2xs text-ink-muted">14 ms · DNS-over-HTTPS</div>
                  </div>
                </div>
                <span className="text-xs font-semibold text-ok">защищено</span>
              </div>
            </div>
          </GlassPanel>

          <GlassPanel className="min-h-0 flex-1 overflow-hidden" padded={false}>
            <PixelLogFeed paused={paused} phase={phase} />
          </GlassPanel>
        </div>
      </div>
    </div>
  );
}

const ENGINES = ["Zapret2 · v2.3 · Beta", "Zapret Legacy · v1.7"] as const;
const CATEGORIES = ["Discord", "YouTube / Twitch", "Gaming", "Universal"] as const;
const PROFILES = ["general-alt2", "general-alt3", "discord-fix"];

function DpiScreen({
  phase,
  paused,
  active,
  heroSize,
  onToggle,
  onScan,
}: {
  phase: ObsessionVisualPhase;
  paused: boolean;
  active: boolean;
  heroSize: number;
  onToggle: () => void;
  onScan: () => void;
}) {
  const [engine, setEngine] = useState<string>(ENGINES[0]);
  const [categories, setCategories] = useState<readonly string[]>(["Discord", "YouTube / Twitch", "Universal"]);
  const [profile, setProfile] = useState<string>(PROFILES[0]);

  const toggleCategory = (label: string) =>
    setCategories((current) =>
      current.includes(label) ? current.filter((item) => item !== label) : [...current, label],
    );

  return (
    <div className="flex h-full min-h-0 flex-col gap-4">
      <header className="flex items-center justify-between">
        <div>
          <h1 className="font-display text-3xl font-semibold tracking-tight text-gradient">DPI-обход</h1>
          <p className="text-sm text-ink-muted">Обход блокировок через Zapret (winws)</p>
        </div>
        <StatusBadge active={active} />
      </header>

      <div className="sunken-lab-stage screen-split screen-split--end-360">
        <GlassPanel scroll contentClassName="flex flex-col items-center gap-6">
          <section className="mt-2 flex flex-col items-center gap-4">
            <HeroCore onToggle={onToggle} paused={paused} phase={phase} size={heroSize} />
            <div className="text-center text-sm text-ink-soft">{PHASE_COPY[phase]}</div>
          </section>

          <section className="w-full">
            <SectionLabel>Движок</SectionLabel>
            <div className="flex flex-wrap gap-2">
              {ENGINES.map((item) => (
                <Chip
                  active={engine === item}
                  ariaChecked={engine === item}
                  key={item}
                  label={item}
                  onClick={() => {
                    setEngine(item);
                    labToast.info(`движок: ${item}`);
                  }}
                  role="radio"
                />
              ))}
            </div>
          </section>

          <section className="w-full">
            <SectionLabel>Категории</SectionLabel>
            <div className="flex flex-wrap gap-2">
              {CATEGORIES.map((item) => (
                <Chip
                  active={categories.includes(item)}
                  key={item}
                  label={item}
                  onClick={() => toggleCategory(item)}
                />
              ))}
            </div>
          </section>

          <section className="w-full">
            <SectionLabel>Профиль стратегии</SectionLabel>
            <Select
              onChange={(value) => {
                setProfile(value);
                labToast.success(`профиль применён: ${value}`);
              }}
              options={PROFILES}
              value={profile}
            />
          </section>

          <div className="flex w-full gap-2">
            <Button
              className="flex-1"
              onClick={() => {
                onScan();
                labToast.info("авто-подбор запущен");
              }}
              variant="ghost"
            >
              Авто-подбор
            </Button>
            <Button onClick={onScan} variant="ghost">
              <span className="flex items-center gap-1.5">
                <Icon.Radar size={16} /> Тест
              </span>
            </Button>
          </div>
        </GlassPanel>

        <GlassPanel className="flex flex-col overflow-hidden" padded={false}>
          <PixelLogFeed paused={paused} phase={phase} />
        </GlassPanel>
      </div>
    </div>
  );
}

export function ObsessionAxolotlLab({ initialTab = "overview" }: { initialTab?: TabId } = {}) {
  const [activeTab, setActiveTab] = useState<TabId>(initialTab);
  const [phase, setPhase] = useState<ObsessionVisualPhase>("idle");
  const [paused, setPaused] = useState(false);
  const [isolate, setIsolate] = useState<PlaneIndex | null>(null);
  const [dither, setDither] = useState(true);
  const [scale, setScale] = useState<number | null>(null);
  const [metrics, setMetrics] = useState<FieldMetrics>({ scale: 2, width: 480, height: 330 });

  const active = phase === "focused";
  // Кот в панели живёт на той же сетке, что и мир за ней: 64 арт-пикселя спрайта
  // умножаются на тот же шаг, что выбрало поле. Разные сетки в одном кадре — та
  // самая причина, по которой ядро раньше выглядело наклеенным.
  const heroSize = 64 * metrics.scale;
  const handleMetrics = useCallback((next: FieldMetrics) => setMetrics(next), []);

  const toggleProtection = () => {
    setPhase((current) => {
      const next = current === "focused" ? "idle" : "focused";
      if (next === "focused") labToast.success("защита включена · маршрут держится");
      else labToast.info("защита выключена");
      return next;
    });
  };
  const runScan = () => {
    setPhase("scanning");
    labToast.info("сканирую маршруты…");
  };

  return (
    <div
      className="sunken-actual-lab relative h-screen w-screen overflow-hidden"
      data-phase={phase}
      data-theme="obsession"
      style={{ ["--art-px" as string]: `${metrics.scale}px` }}
    >
      <ParallaxProvider paused={paused}>
        <div className="pointer-events-none absolute inset-0">
          <SunkenStarFieldLab
            dither={dither}
            isolate={isolate}
            onMetrics={handleMetrics}
            paused={paused}
            phase={phase}
            scale={scale}
          />
          <div className="sunken-actual-lab__veil" />
        </div>

        <div className="absolute inset-x-0 top-0 z-20">
          <LabTitleBar />
        </div>

        <div className="absolute inset-0 top-10 z-10 flex">
          <LabNavRail activeTab={activeTab} onSelectTab={setActiveTab} paused={paused} phase={phase} />

          <main className="sunken-lab-main flex-1">
            <div className="sunken-lab-screen">
              {activeTab === "overview" ? (
                <OverviewScreen
                  active={active}
                  heroSize={heroSize}
                  onScan={runScan}
                  onToggle={toggleProtection}
                  paused={paused}
                  phase={phase}
                />
              ) : (
                <DpiScreen
                  active={active}
                  heroSize={heroSize}
                  onScan={runScan}
                  onToggle={toggleProtection}
                  paused={paused}
                  phase={phase}
                />
              )}
            </div>

            <InspectorStrip
              handlers={{
                onDither: setDither,
                onIsolate: setIsolate,
                onPaused: setPaused,
                onPhase: setPhase,
                onScale: setScale,
              }}
              state={{
                artHeight: metrics.height,
                artScale: metrics.scale,
                artWidth: metrics.width,
                dither,
                isolate,
                paused,
                phase,
                scale,
              }}
            />
          </main>
        </div>

        <PixelToaster />
      </ParallaxProvider>
    </div>
  );
}

const rootElement = typeof document === "undefined" ? null : document.getElementById("root");
if (rootElement) {
  const root = ReactDOM.createRoot(rootElement);
  root.render(<ObsessionAxolotlLab />);

  if (import.meta.hot) import.meta.hot.dispose(() => root.unmount());
}




