import { useEffect, useRef, useState, type CSSProperties, type PointerEvent as ReactPointerEvent } from "react";
import { createRoot } from "react-dom/client";
import { HeroField } from "../../../src/design/components/HeroField";
import { ThemePreview } from "../../../src/design/components/ThemePreview";
import { ThemeNavIcon } from "../../../src/design/components/ThemeNavIcon";
import { HalloweenIcon } from "../../../src/design/components/HalloweenIcon";
import { Icon } from "../../../src/design/components/icons";
import { THEMES, useThemeStore, type Theme } from "../../../src/store/themeStore";
import { useDpiStore } from "../../../src/store/dpiStore";
import { useProxyStore } from "../../../src/store/proxyStore";
import { useSettingsStore } from "../../../src/store/settingsStore";
import { useMotionOff } from "../../../src/design/render";
import { invokeBrowserPreview } from "../../../src/lib/browserPreview";
import type { Settings } from "../../../src/lib/tauri";
import "../../../src/styles/fonts.css";
import "../../../src/styles/globals.css";
import "./style.css";

const concepts: Record<Theme, { material: string; effect: string; stamp: string }> = {
  goldenmeadow: { material: "Тёплая бумага · осенний гербарий", effect: "При смене статуса два листочка слетают с внешнего края", stamp: "ОСЕНЬ / 01" },
  aurora: { material: "Полярное стекло · световой контур", effect: "Бирюзовая волна сияния проходит вдоль кромки", stamp: "СЕВЕР / 02" },
  ophanim: { material: "Записи алхимика · медь и лаванда", effect: "Алхимическая печать загорается, поднимаются искры", stamp: "РЕЦЕПТ / 03" },
  japan: { material: "Дождевое стекло · холодный сланец", effect: "Капли скользят по краю, оставляя короткий след", stamp: "ДОЖДЬ / 04" },
  midnight: { material: "Графит · серебро · ночная плёнка", effect: "Сдержанное отражение фонаря скользит по поверхности", stamp: "НОЧЬ / 05" },
  catnap: { material: "Дорожные билеты · закатное тепло", effect: "Солнечная полоса проходит по бумаге билета", stamp: "БИЛЕТ / 06" },
  fallendown: { material: "Пиксельные рамки · тихие руины", effect: "Звезда сохранения рассыпается на несколько пикселей", stamp: "SAVE / 07" },
  yanineko: { material: "Мятные записки · карандаш и наклейки", effect: "На свободном краю появляются три следа лапки", stamp: "ЗАМЕТКА / 08" },
};
const menu = [
  ["overview", "Обзор"], ["dpi", "DPI-обход"], ["ai", "ИИ-разблокировка"],
  ["telegram", "Telegram"], ["lists", "Списки"], ["profiles", "Профили"], ["settings", "Настройки"],
] as const;

function Leaf({ index }: { index: number }) {
  return <svg className={`oa-leaf oa-leaf-${index}`} viewBox="0 0 32 38"><path d="m16 2 4 9 6-3-1 9 6 2-9 7-5 2-1 8-2-1 1-8-9-3-5-7 8 1-2-9 7 4Z" fill="currentColor" /><path d="m16 10-1 21m0-10-6-4m6 7 8-7" fill="none" stroke="#64381c" strokeWidth="1.1" /></svg>;
}

function Ornament({ theme }: { theme: Theme }) {
  return <span className="oa-effects" aria-hidden="true">
    {theme === "goldenmeadow" ? <><Leaf index={0} /><Leaf index={1} /><span className="oa-pressed-leaf">❧</span></> :
      theme === "ophanim" ? <><span className="oa-seal">✧</span><i className="oa-spark" /><i className="oa-spark oa-second" /></> :
      theme === "japan" ? <><i className="oa-drop" /><i className="oa-drop oa-second" /></> :
      theme === "fallendown" ? <><span className="oa-save">✦</span><i className="oa-pixel" /><i className="oa-pixel oa-second" /></> :
      theme === "yanineko" ? <>{[0, 1, 2].map(i => <svg key={i} className="oa-paw" style={{ "--i": i } as CSSProperties} viewBox="0 0 30 30"><ellipse cx="15" cy="20" rx="8" ry="6" /><ellipse cx="5" cy="12" rx="3" ry="4" /><ellipse cx="12" cy="7" rx="3" ry="4" /><ellipse cx="20" cy="8" rx="3" ry="4" /><ellipse cx="26" cy="14" rx="3" ry="4" /></svg>)}</> :
      <span className="oa-light" />}
  </span>;
}

// Повтор заменяет только слой эффекта: карточка и фокус остаются на месте.
function StatusEffect({ theme, event, replay, disabled }: { theme: Theme; event: string; replay: number; disabled: boolean }) {
  const previous = useRef({ event, replay });
  const [generation, setGeneration] = useState(0);
  const [visible, setVisible] = useState(false);
  useEffect(() => {
    const changed = previous.current.event !== event || previous.current.replay !== replay;
    previous.current = { event, replay };
    if (disabled || !changed) {
      setVisible(false);
      return;
    }
    setGeneration(value => value + 1);
    setVisible(true);
    // Слой удаляется после затухания, включая эффекты с задержкой запуска.
    const timeout = window.setTimeout(() => setVisible(false), 4600);
    return () => window.clearTimeout(timeout);
  }, [event, replay, disabled]);
  return visible ? <span key={generation} className="oa-event" aria-hidden="true">
    <Ornament theme={theme} />
  </span> : null;
}

function ServiceCard({ theme, service, index, replay, disabled, materialOn }: {
  theme: Theme; service: { name: string; icon: typeof Icon.Bolt; state: string; detail: string; tone: string };
  index: number; replay: number; disabled: boolean; materialOn: boolean;
}) {
  const materialEnabled = materialOn && !disabled && (theme === "goldenmeadow" || theme === "aurora" || theme === "japan");
  const moveMaterial = (event: ReactPointerEvent<HTMLElement>) => {
    if (!materialEnabled || event.pointerType === "touch") return;
    const card = event.currentTarget;
    const bounds = card.getBoundingClientRect();
    const x = Math.max(0, Math.min(100, ((event.clientX - bounds.left) / bounds.width) * 100));
    const y = Math.max(0, Math.min(100, ((event.clientY - bounds.top) / bounds.height) * 100));
    card.style.setProperty("--oa-pointer-x", `${x}%`);
    card.style.setProperty("--oa-pointer-y", `${y}%`);
    card.style.setProperty("--oa-surface-energy", "1");
  };
  const settleMaterial = (event: ReactPointerEvent<HTMLElement>) => {
    const card = event.currentTarget;
    card.style.setProperty("--oa-surface-energy", "0");
    card.style.setProperty("--oa-pointer-x", "50%");
    card.style.setProperty("--oa-pointer-y", "50%");
  };
  return <section className="oa-card oa-service" data-tone={service.tone}
    data-material={materialEnabled} aria-label={`${service.name}: ${service.state}`}
    onPointerEnter={moveMaterial} onPointerMove={moveMaterial} onPointerLeave={settleMaterial}>
    <span className="oa-material" aria-hidden="true" />
    <span className="oa-hover" aria-hidden="true"><Ornament theme={theme} /></span>
    <StatusEffect theme={theme} event={service.state} replay={replay} disabled={disabled} />
    <span className="oa-service-icon"><service.icon size={22} /></span>
    <div className="oa-service-copy"><h2>{service.name}</h2><div className="oa-status" aria-live="polite"><i />{service.state}</div></div>
    <span className="oa-card-number">0{index + 1}</span>
    <p className="oa-detail">{service.detail}</p>
  </section>;
}

function AtmosphereLab() {
  const [theme, setTheme] = useState<Theme>("goldenmeadow");
  const [active, setActive] = useState(true);
  const [warning, setWarning] = useState(false);
  const [quiet, setQuiet] = useState(false);
  const [replay, setReplay] = useState(0);
  const [background, setBackground] = useState(true);
  const [offline, setOffline] = useState(false);
  const [materialOn, setMaterialOn] = useState(true);
  const motionOff = useMotionOff();
  const concept = concepts[theme];
  const choose = (next: Theme) => {
    setTheme(next);
    // Только память текущей вкладки: выбор темы приложения не перезаписывается.
    useThemeStore.setState({ theme: next });
  };
  const toggle = () => {
    const next = !active;
    setActive(next);
    useDpiStore.setState({ active: next });
    useProxyStore.setState({ running: next });
  };
  const services = [
    { name: "DPI-обход", icon: Icon.Bolt, state: active ? "Активен" : "Выключен", detail: "Discord, YouTube / Twitch", tone: active ? "ok" : "off" },
    { name: "Telegram-прокси", icon: Icon.Send, state: active ? "Работает" : "Выключен", detail: active ? "MTProto · 127.0.0.1:1443" : "Готов к запуску", tone: active ? "ok" : "off" },
    { name: "ИИ-разблокировка", icon: Icon.Robot, state: warning ? "Есть обновление" : "Установлено", detail: "Провайдер malw · 29 августа 2026", tone: warning ? "warn" : "ok" },
    { name: "Сеть", icon: Icon.Globe, state: offline ? "Нет подключения" : "Определена", detail: offline ? "Ожидание подключения к сети" : "AS12389 PJSC Rostelecom", tone: offline ? "warn" : "neutral" },
  ];
  return <div className="oa-lab" data-theme={theme} data-still={motionOff} data-reduce-motion={motionOff}>
    <header className="oa-tools">
      <div className="oa-lab-title"><span className="oa-lab-dot" /> OBSESSION <span>Лаборатория плашек</span></div>
      <div className="oa-controls">
        <label><input type="checkbox" checked={background} onChange={e => setBackground(e.target.checked)} /> Фон темы</label>
        <label><input type="checkbox" checked={materialOn} onChange={e => setMaterialOn(e.target.checked)} /> Живая поверхность</label>
        <label><input type="checkbox" checked={warning} onChange={e => setWarning(e.target.checked)} /> Обновление ИИ</label>
        <label><input type="checkbox" checked={offline} onChange={e => setOffline(e.target.checked)} /> Нет сети</label>
        <label><input type="checkbox" checked={quiet} onChange={e => {
          setQuiet(e.target.checked);
          useSettingsStore.setState(s => ({ settings: s.settings ? { ...s.settings, reduce_motion: e.target.checked } : null }));
        }} /> Без анимаций</label>
        <button disabled={motionOff} onClick={() => setReplay(value => value + 1)}>Повторить эффекты</button>
      </div>
    </header>
    <div className="oa-theme-strip" role="group" aria-label="Темы лаборатории">
      {THEMES.map((t, i) => <button key={t.id} aria-pressed={theme === t.id} onClick={() => choose(t.id)}><span>{String(i + 1).padStart(2, "0")}</span>{t.label}{t.secret && <small title="Секретная тема">✧</small>}</button>)}
    </div>
    <div className="oa-stage">
      <div className="oa-backdrop">{background && <HeroField key={theme} theme={theme} frozen={motionOff} phase={active ? "focused" : "idle"} screen="overview" />}</div>
      <aside className="oa-sidebar" aria-label="Контекст меню — демонстрация">
        <div className="oa-brand"><span>Obsession</span><small>V1.1.0</small></div>
        <span className="oa-menu-label">МЕНЮ</span>
        {menu.map(([id, label]) => <div key={id} className="oa-menu-item" data-selected={id === "overview"}>
          {theme === "goldenmeadow" ? <HalloweenIcon item={id} /> : <ThemeNavIcon theme={theme} item={id} />}{label}
        </div>)}
        <small className="oa-sidebar-foot">made by AcediaWhy</small>
      </aside>
      <main className="oa-main">
        <div className="oa-heading"><div><h1>Обзор</h1><p>Состояние защиты одним взглядом</p></div><span className="oa-edition">{concept.stamp}</span></div>
        <div key={theme} className="oa-board">
          <section className="oa-card oa-command" data-tone={active ? "ok" : "off"}>
            <StatusEffect theme={theme} event={String(active)} replay={replay} disabled={motionOff} />
            <div className="oa-mascot"><ThemePreview theme={theme} selected={active} size={60} /></div>
            <div className="oa-command-copy"><span className="oa-eyebrow">OBSESSION / СОСТОЯНИЕ</span><h2>{active ? "Под защитой" : "Защита выключена"}</h2><p>{active ? "Обход активен · Discord, YouTube / Twitch" : "Включите обход с текущими настройками"}</p><span className="oa-session"><i />{active ? "Сеанс работает стабильно" : "Готово к подключению"}</span></div>
            <button className="oa-toggle" onClick={toggle}>{active ? "Выключить" : "Включить защиту"}</button>
          </section>
          <div className="oa-grid">{services.map((service, i) => <ServiceCard key={service.name} theme={theme} service={service} index={i} replay={replay} disabled={motionOff} materialOn={materialOn} />)}</div>
        </div>
      </main>
    </div>
    <footer className="oa-notes"><div><strong>{concept.material}</strong><span>{concept.effect}.</span></div><p>Наведите курсор на карточки. Переключите статус, чтобы увидеть волну и тематический эффект. Данные демонстрационные.</p></footer>
  </div>;
}

// Стенд не импортирует App и не запускает bootstrap, команды Tauri или службы.
if (import.meta.env.DEV && !("__TAURI_INTERNALS__" in window)) {
  void invokeBrowserPreview<Settings>("get_settings").then(settings => {
    useSettingsStore.setState({ settings, loaded: true });
    useThemeStore.setState({ theme: "goldenmeadow" });
    useDpiStore.setState({ active: true });
    useProxyStore.setState({ running: true });
    createRoot(document.getElementById("root")!).render(<AtmosphereLab />);
  });
}
