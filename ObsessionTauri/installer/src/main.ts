import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

import "./style.css";
import eyeWebm from "../../src/assets/eye.webm";
import eyePoster from "../../src/assets/eye-poster.png";
import {
  applyProgressEvent,
  escapeHtml,
  initialProgressState,
  installerFailureCopy,
  normalizeInstallerFailure,
  type InstallerFailure,
  type InstallerOutcome,
  type InstallerProgressEvent,
} from "./installerState";

type Step = "bootstrapping" | "welcome" | "options" | "installing" | "done" | "error";
type InstallMode = "install" | "update" | "repair" | "blocked";
type RetryAction = "snapshot" | "install" | "launch";

interface InstallerSnapshot {
  dir: string;
  mode: InstallMode;
  installedVersion: string | null;
  logPath: string;
}

interface ModeCopy {
  badge: string;
  eyebrow: string;
  title: string;
  description: string;
  optionsTitle: string;
  optionsDescription: string;
  progressTitle: string;
  action: string;
  done: string;
  doneDescription: string;
}

const VERSION = (import.meta.env.VITE_APP_VERSION as string | undefined) ?? "dev";
const REDUCE_MOTION = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
const params = new URLSearchParams(window.location.search);
const PREVIEW = import.meta.env.DEV && params.has("preview");

const STAGE_TEXT: Record<string, string> = {
  prepare: "Проверяем систему и свободное место",
  stop: "Останавливаем запущенный Obsession",
  stage: "Готовим безопасную область обновления",
  install: "Распаковываем проверенную версию",
  verify: "Проверяем целостность файлов",
  commit: "Переключаемся на новую версию",
  registry: "Обновляем системные записи",
  shortcuts: "Синхронизируем ярлыки",
  rollback: "Возвращаем предыдущую рабочую версию",
  cleanup: "Удаляем временные файлы",
  finish: "Завершаем настройку",
};

const PHASES = [
  { id: "prepare", label: "Подготовка", detail: "Система и процессы", stages: ["prepare", "stop", "stage"] },
  { id: "files", label: "Файлы", detail: "Распаковка и проверка", stages: ["install", "verify"] },
  { id: "switch", label: "Переключение", detail: "Версия и ярлыки", stages: ["commit", "registry", "shortcuts"] },
  { id: "finish", label: "Готово", detail: "Финальная очистка", stages: ["cleanup", "finish"] },
] as const;

const STEP_INDEX: Record<Step, number> = {
  bootstrapping: 0,
  welcome: 0,
  options: 1,
  installing: 2,
  error: 2,
  done: 3,
};

const state = {
  step: "bootstrapping" as Step,
  mode: "install" as InstallMode,
  dir: "",
  installedVersion: null as string | null,
  logPath: "",
  desktop: true,
  startMenu: true,
  pct: 0,
  stage: "prepare",
  progressSequence: initialProgressState.sequence,
  failure: null as InstallerFailure | null,
  retryAction: "snapshot" as RetryAction,
};

function activeMode(): Exclude<InstallMode, "blocked"> {
  return state.mode === "blocked" ? "repair" : state.mode;
}

function modeCopy(): ModeCopy {
  const installed = state.installedVersion ? `v${state.installedVersion}` : "текущую версию";
  const copies: Record<Exclude<InstallMode, "blocked">, ModeCopy> = {
    install: {
      badge: "Чистая установка",
      eyebrow: "Добро пожаловать",
      title: "Установим Obsession",
      description: "Настроим приложение и ярлыки. Всё займёт меньше минуты — без лишних мастеров и перезагрузок.",
      optionsTitle: "Настройте установку",
      optionsDescription: "Приложение будет защищённо установлено в Program Files. Выберите удобные ярлыки.",
      progressTitle: "Устанавливаем Obsession",
      action: "Установить",
      done: "Obsession установлен",
      doneDescription: "Приложение готово к первому запуску.",
    },
    update: {
      badge: state.installedVersion ? `Обновление с ${installed}` : "Обновление",
      eyebrow: "Доступна новая версия",
      title: `Обновим Obsession до v${VERSION}`,
      description: "Настройки останутся на месте. Текущую версию сохраним до тех пор, пока новая не пройдёт проверку.",
      optionsTitle: "Проверьте параметры обновления",
      optionsDescription: "Папка установки зафиксирована, ярлыки можно синхронизировать заново.",
      progressTitle: "Обновляем Obsession",
      action: "Обновить",
      done: "Obsession обновлён",
      doneDescription: `Новая версия v${VERSION} установлена и проверена.`,
    },
    repair: {
      badge: "Восстановление",
      eyebrow: "Исправим установку",
      title: "Восстановим Obsession",
      description: "Заменим повреждённые файлы, проверим системные записи и заново синхронизируем ярлыки.",
      optionsTitle: "Параметры восстановления",
      optionsDescription: "Личные настройки не затрагиваются — восстанавливаются только файлы приложения.",
      progressTitle: "Восстанавливаем Obsession",
      action: "Восстановить",
      done: "Obsession восстановлен",
      doneDescription: "Файлы приложения проверены и готовы к работе.",
    },
  };
  return copies[activeMode()];
}

const root = document.getElementById("app")!;
root.innerHTML = `
  <div class="ambient" aria-hidden="true">
    <div class="ambient-orb ambient-orb-one"></div>
    <div class="ambient-orb ambient-orb-two"></div>
    <div class="grain"></div>
  </div>
  <header class="titlebar" data-tauri-drag-region>
    <div class="titlebar-brand" data-tauri-drag-region>
      <span class="brand-mark" aria-hidden="true"></span>
      <span data-tauri-drag-region>OBSESSION</span>
      <span class="titlebar-separator" aria-hidden="true"></span>
      <span class="titlebar-context" data-tauri-drag-region>SETUP</span>
    </div>
    <div class="titlebar-actions">
      <button class="window-button" id="btn-min" type="button" title="Свернуть" aria-label="Свернуть окно">
        <svg viewBox="0 0 16 16" aria-hidden="true"><path d="M3 8.5h10" /></svg>
      </button>
      <button class="window-button window-close" id="btn-close" type="button" title="Закрыть" aria-label="Закрыть установщик">
        <svg viewBox="0 0 16 16" aria-hidden="true"><path d="m4 4 8 8m0-8-8 8" /></svg>
      </button>
    </div>
  </header>
  <main class="stage" id="stage"></main>
  <footer class="footer">
    <div class="footer-actions footer-left" id="foot-left"></div>
    <ol class="stepper" id="stepper" aria-label="Этапы установки"></ol>
    <div class="footer-actions footer-right" id="foot-right"></div>
  </footer>
  <div class="sr-only" id="announcer" aria-live="polite" aria-atomic="true"></div>
`;

const el = (id: string) => document.getElementById(id)!;

function bind(id: string, fn: () => void) {
  el(id).addEventListener("click", fn);
}

function eyeHtml(size: number, quiet = false): string {
  const media = REDUCE_MOTION
    ? `<img src="${eyePoster}" alt="" />`
    : `<video src="${eyeWebm}" poster="${eyePoster}" muted loop playsinline autoplay></video>`;
  return `<div class="eye-orbit${quiet ? " eye-quiet" : ""}" style="--eye-size:${size}px" aria-hidden="true">
    <span class="eye-aura"></span>
    <div class="eye-tile">
      ${media}
      <span class="eye-depth"></span>
      <span class="eye-glint"></span>
      <span class="eye-ring"></span>
    </div>
  </div>`;
}

function shieldIcon(): string {
  return `<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M12 3 5.5 5.8v5.4c0 4.2 2.6 8 6.5 9.8 3.9-1.8 6.5-5.6 6.5-9.8V5.8L12 3Z"/><path d="m9 12 2 2 4-4"/></svg>`;
}

function folderIcon(): string {
  return `<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M3.5 6.5h6l2 2h9v9a2 2 0 0 1-2 2h-13a2 2 0 0 1-2-2v-11Z"/></svg>`;
}

function renderStepper() {
  const current = STEP_INDEX[state.step];
  const labels = ["Начало", "Параметры", "Установка", "Готово"];
  el("stepper").innerHTML = labels
    .map((label, index) => {
      const status = index < current ? "complete" : index === current ? (state.step === "error" ? "error" : "current") : "upcoming";
      const currentAttr = index === current ? ` aria-current="step"` : "";
      return `<li class="stepper-item is-${status}"${currentAttr}><span class="stepper-dot"></span><span class="stepper-label">${label}</span></li>`;
    })
    .join("");
}

function setText(id: string, value: string) {
  const target = document.getElementById(id);
  if (target) target.textContent = value;
}

function setPathView() {
  const view = document.getElementById("path-view");
  if (view) {
    view.textContent = state.dir;
    view.title = state.dir;
  }
}

function currentPhaseIndex(stage: string): number {
  // Rollback возможен только после атомарного переключения каталога, поэтому
  // оставляем пользователя на соответствующей фазе, а не прыгаем в начало.
  if (stage === "rollback") return 2;
  const index = PHASES.findIndex((phase) => phase.stages.some((item) => item === stage));
  return index === -1 ? 0 : index;
}

function timelineHtml(): string {
  const current = currentPhaseIndex(state.stage);
  return PHASES.map((phase, index) => {
    const status = index < current ? "complete" : index === current ? "current" : "upcoming";
    return `
      <li class="timeline-item is-${status}" data-phase="${index}">
        <span class="timeline-marker" aria-hidden="true"><span></span></span>
        <span class="timeline-copy"><strong>${phase.label}</strong><small>${phase.detail}</small></span>
      </li>`;
  }).join("");
}

function syncProgressView() {
  const progress = document.getElementById("install-progress");
  if (progress) {
    progress.setAttribute("aria-valuenow", String(state.pct));
    progress.setAttribute("aria-valuetext", STAGE_TEXT[state.stage] ?? "Установка выполняется");
  }
  const bar = document.getElementById("bar");
  if (bar) bar.style.width = `${Math.max(0, Math.min(100, state.pct))}%`;
  setText("progress-pct", `${Math.round(state.pct)}%`);
  setText("stage-line", STAGE_TEXT[state.stage] ?? "Продолжаем установку");

  const current = currentPhaseIndex(state.stage);
  document.querySelectorAll<HTMLElement>("[data-phase]").forEach((item) => {
    const index = Number(item.dataset.phase);
    item.className = `timeline-item is-${index < current ? "complete" : index === current ? "current" : "upcoming"}`;
  });

  const recovery = document.getElementById("recovery-note");
  if (recovery) recovery.classList.toggle("visible", state.stage === "rollback");
  el("announcer").textContent = STAGE_TEXT[state.stage] ?? "Установка выполняется";
}

let lastRenderedStep: Step | null = null;

function focusScreenHeading() {
  if (lastRenderedStep === state.step) return;
  lastRenderedStep = state.step;
  window.requestAnimationFrame(() => {
    document.querySelector<HTMLElement>("[data-screen-title]")?.focus({ preventScroll: true });
  });
}

function render() {
  const stage = el("stage");
  const left = el("foot-left");
  const right = el("foot-right");
  const copy = Object.fromEntries(
    Object.entries(modeCopy()).map(([key, value]) => [key, escapeHtml(value)]),
  ) as unknown as ModeCopy;
  const safeVersion = escapeHtml(VERSION);

  document.body.dataset.step = state.step;
  document.body.dataset.mode = state.mode;
  left.innerHTML = "";
  right.innerHTML = "";
  el("btn-close").classList.toggle("hidden", state.step === "installing");
  el("btn-min").classList.toggle("hidden", state.step === "installing");
  renderStepper();

  switch (state.step) {
    case "bootstrapping":
      stage.innerHTML = `
        <section class="screen bootstrap-screen" aria-busy="true">
          ${eyeHtml(118, true)}
          <p class="eyebrow">Obsession Setup</p>
          <h1 class="screen-title" data-screen-title tabindex="-1">Проверяем установленную версию</h1>
          <p class="screen-description">Читаем только локальное состояние и завершаем незакрытое восстановление, если оно требуется.</p>
          <span class="bootstrap-progress" role="progressbar" aria-label="Подготовка установщика"><i></i></span>
        </section>`;
      break;

    case "welcome":
      stage.innerHTML = `
        <section class="screen welcome-screen">
          <div class="welcome-copy">
            <div class="mode-badge"><span></span>${copy.badge}</div>
            <p class="eyebrow">${copy.eyebrow}</p>
            <h1 class="hero-title" data-screen-title tabindex="-1">${copy.title}</h1>
            <p class="hero-description">${copy.description}</p>
            <div class="meta-row" aria-label="Информация о версии">
              <span>v${safeVersion}</span><i></i><span>Windows x64</span><i></i><span>~45 МБ</span>
            </div>
            <div class="trust-note">
              <span class="trust-icon">${shieldIcon()}</span>
              <span><strong>Безопасная замена файлов</strong><small>${state.mode === "repair" ? "При восстановлении рабочая версия сохраняется до успешной проверки." : state.mode === "update" ? "При обновлении рабочая версия сохраняется до успешной проверки." : "Файлы копируются только в выбранную папку Obsession."}</small></span>
            </div>
          </div>
          <div class="brand-visual" aria-label="Obsession">
            <div class="visual-halo" aria-hidden="true"></div>
            ${eyeHtml(148)}
            <div class="wordmark">OBSESSION</div>
            <div class="visual-caption">PRIVATE NETWORK TOOLKIT</div>
          </div>
        </section>`;
      right.innerHTML = `<button class="btn btn-primary" id="go-options" type="button">Продолжить<span class="btn-arrow" aria-hidden="true">→</span></button>`;
      bind("go-options", () => {
        state.step = "options";
        render();
      });
      break;

    case "options": {
      const browse = `<span class="path-lock" title="Защищённая установка использует Program Files">Зафиксирован</span>`;
      stage.innerHTML = `
        <section class="screen options-screen">
          <header class="screen-header">
            <p class="eyebrow">Шаг 2 из 4</p>
            <h1 class="screen-title" data-screen-title tabindex="-1">${copy.optionsTitle}</h1>
            <p class="screen-description">${copy.optionsDescription}</p>
          </header>
          <div class="options-layout">
            <div class="panel path-panel">
              <div class="panel-heading">
                <span class="panel-icon">${folderIcon()}</span>
                <span><strong>Папка приложения</strong><small>Защищённая системная папка Program Files</small></span>
              </div>
              <div class="path-row">
                <div class="path-input" id="path-view" aria-label="Путь установки"></div>
                ${browse}
              </div>
            </div>
            <fieldset class="shortcut-fieldset">
              <legend>Ярлыки</legend>
              <div class="shortcut-grid">
                <label class="choice-card" for="cb-desktop">
                  <input type="checkbox" id="cb-desktop" ${state.desktop ? "checked" : ""}/>
                  <span class="choice-control" aria-hidden="true"></span>
                  <span class="choice-copy"><strong>Рабочий стол</strong><small>Быстрый запуск с рабочего стола</small></span>
                </label>
                <label class="choice-card" for="cb-start">
                  <input type="checkbox" id="cb-start" ${state.startMenu ? "checked" : ""}/>
                  <span class="choice-control" aria-hidden="true"></span>
                  <span class="choice-copy"><strong>Меню «Пуск»</strong><small>Obsession появится в списке приложений</small></span>
                </label>
              </div>
            </fieldset>
            <div class="safety-callout">
              <span class="safety-icon">${shieldIcon()}</span>
              <p><strong>${state.mode === "install" ? "Готово к установке" : "Настройки останутся на месте"}</strong><span>${state.mode === "install" ? "Запущенный Obsession будет аккуратно закрыт перед копированием файлов." : "До успешного завершения можно автоматически вернуться к предыдущей рабочей версии."}</span></p>
            </div>
          </div>
        </section>`;
      setPathView();
      left.innerHTML = `<button class="btn btn-ghost" id="back" type="button"><span aria-hidden="true">←</span>Назад</button>`;
      right.innerHTML = `<button class="btn btn-primary" id="do-install" type="button">${copy.action}<span class="btn-arrow" aria-hidden="true">→</span></button>`;
      bind("back", () => {
        state.step = "welcome";
        render();
      });
      bind("do-install", () => void startInstall());
      (el("cb-desktop") as HTMLInputElement).onchange = (event) => {
        state.desktop = (event.target as HTMLInputElement).checked;
      };
      (el("cb-start") as HTMLInputElement).onchange = (event) => {
        state.startMenu = (event.target as HTMLInputElement).checked;
      };
      break;
    }

    case "installing":
      stage.innerHTML = `
        <section class="screen progress-screen">
          <div class="progress-heading">
            ${eyeHtml(70, true)}
            <div>
              <p class="eyebrow">Шаг 3 из 4</p>
              <h1 class="screen-title" data-screen-title tabindex="-1">${copy.progressTitle}</h1>
              <p class="screen-description">Можно откинуться на спинку кресла — остальное сделаем сами.</p>
            </div>
            <strong class="progress-percent" id="progress-pct">${state.pct}%</strong>
          </div>
          <div class="progress-card panel">
            <div class="progress-labels"><span id="stage-line" role="status" aria-live="polite">${STAGE_TEXT[state.stage]}</span><span>v${safeVersion}</span></div>
            <div class="progress-track" id="install-progress" role="progressbar" aria-label="Прогресс установки" aria-valuemin="0" aria-valuemax="100" aria-valuenow="${state.pct}" aria-valuetext="${STAGE_TEXT[state.stage]}">
              <div class="progress-fill" id="bar" style="width:${state.pct}%"><span></span></div>
            </div>
            <ol class="timeline">${timelineHtml()}</ol>
            <div class="recovery-note" id="recovery-note">Не получилось применить новую версию. Возвращаем предыдущую установку — ваши данные в безопасности.</div>
          </div>
          <p class="do-not-close"><span aria-hidden="true"></span>Не выключайте компьютер и не закрывайте установщик</p>
        </section>`;
      syncProgressView();
      break;

    case "done":
      stage.innerHTML = `
        <section class="screen done-screen">
          <div class="done-visual">
            ${eyeHtml(94, true)}
            <span class="success-badge" aria-hidden="true"><svg viewBox="0 0 24 24"><path d="m6.5 12.5 3.5 3.5 7.5-8"/></svg></span>
          </div>
          <div class="mode-badge success"><span></span>Готово</div>
          <h1 class="hero-title done-title" data-screen-title tabindex="-1">${copy.done}</h1>
          <p class="hero-description done-description">${copy.doneDescription}</p>
          <div class="result-path"><span>${folderIcon()}</span><code id="done-path"></code></div>
          <div class="result-meta"><span>Версия v${safeVersion}</span><i></i><span>Ярлыки синхронизированы</span></div>
        </section>`;
      setText("done-path", state.dir);
      left.innerHTML = `<button class="btn btn-ghost" id="quit" type="button">Закрыть</button>`;
      right.innerHTML = `<button class="btn btn-primary" id="launch" type="button">Запустить Obsession<span class="btn-arrow" aria-hidden="true">→</span></button>`;
      bind("quit", () => void closeSetup());
      bind("launch", () => void launchApp());
      break;

    case "error": {
      const failure = state.failure ?? normalizeInstallerFailure(null, "INSTALL_FAILED");
      const failureCopy = installerFailureCopy(failure.code);
      stage.innerHTML = `
        <section class="screen error-screen">
          <div class="error-symbol" aria-hidden="true"><svg viewBox="0 0 24 24"><path d="M12 8v5m0 3.5v.1"/><path d="M10.2 4.5 3.4 17a2 2 0 0 0 1.8 3h13.6a2 2 0 0 0 1.8-3L13.8 4.5a2 2 0 0 0-3.6 0Z"/></svg></div>
          <div class="error-content">
            <p class="eyebrow">${failureCopy.eyebrow}</p>
            <h1 class="screen-title error-heading" data-screen-title tabindex="-1">${failureCopy.title}</h1>
            <p class="error-summary">${failureCopy.summary}</p>
            <div class="error-details panel">
              <strong>Безопасное действие</strong>
              <p>${failure.code === "ROLLBACK_INCOMPLETE" ? "Обычный retry отключён. Новый запуск setup сначала продолжит recovery по защищённому журналу." : "Можно закрыть setup. При допустимом повторе операция снова начнётся с предварительной проверки."}</p>
            </div>
            ${state.logPath ? `<div class="log-row"><span><small>Журнал установки</small><code id="log-path"></code></span><button class="btn btn-secondary btn-compact" id="copy-log" type="button">Скопировать путь</button></div>` : ""}
          </div>
        </section>`;
      setText("log-path", state.logPath);
      left.innerHTML = `<button class="btn btn-ghost" id="quit" type="button">Закрыть</button>`;
      right.innerHTML = failure.retryable ? `<button class="btn btn-primary" id="retry" type="button">${failureCopy.action}<span class="btn-arrow" aria-hidden="true">↻</span></button>` : "";
      bind("quit", () => void closeSetup());
      if (state.logPath) bind("copy-log", () => void copyLogPath());
      if (failure.retryable) {
        bind("retry", () => {
          if (state.retryAction === "snapshot") void loadSnapshot();
          else if (state.retryAction === "launch") void launchApp();
          else void startInstall();
        });
      }
      break;
    }
  }

  if (state.step === "error") el("announcer").textContent = "Установка остановлена";
  else if (state.step === "done") el("announcer").textContent = copy.done;
  else if (state.step !== "installing") el("announcer").textContent = "";
  focusScreenHeading();
}

let installing = false;
let previewTimer: number | undefined;

async function runPreviewInstall() {
  const sequence = [
    [8, "prepare"], [18, "stop"], [28, "stage"], [43, "install"], [59, "verify"],
    [72, "commit"], [82, "registry"], [89, "shortcuts"], [96, "cleanup"], [100, "finish"],
  ] as const;
  for (const [pct, stage] of sequence) {
    await new Promise<void>((resolve) => {
      previewTimer = window.setTimeout(resolve, REDUCE_MOTION ? 80 : 420);
    });
    state.pct = pct;
    state.stage = stage;
    syncProgressView();
  }
}

async function startInstall() {
  if (installing) return;
  installing = true;
  if (previewTimer !== undefined) window.clearTimeout(previewTimer);
  state.step = "installing";
  state.pct = 0;
  state.stage = "prepare";
  state.failure = null;
  state.retryAction = "install";
  render();
  try {
    if (PREVIEW) {
      await runPreviewInstall();
    } else {
      const outcome = await invoke<InstallerOutcome>("install", {
        dir: state.dir,
        desktop: state.desktop,
        startMenu: state.startMenu,
      });
      state.dir = outcome.dir;
      state.mode = outcome.mode;
    }
    state.step = "done";
  } catch (error) {
    state.failure = normalizeInstallerFailure(error, "INSTALL_FAILED");
    state.logPath = state.failure.logPath ?? state.logPath;
    state.step = "error";
  } finally {
    installing = false;
  }
  render();
}

async function closeSetup() {
  if (PREVIEW) {
    el("announcer").textContent = "В preview-режиме окно не закрывается";
    return;
  }
  await invoke("close_setup");
}

async function launchApp() {
  if (PREVIEW) {
    el("announcer").textContent = "Запуск приложения доступен в собранном установщике";
    return;
  }
  try {
    await invoke("launch_app", { dir: state.dir });
  } catch (error) {
    state.failure = normalizeInstallerFailure(error, "LAUNCH_FAILED");
    state.logPath = state.failure.logPath ?? state.logPath;
    state.retryAction = "launch";
    state.step = "error";
    render();
  }
}

async function copyLogPath() {
  try {
    await navigator.clipboard.writeText(state.logPath);
    setText("copy-log", "Скопировано");
    el("announcer").textContent = "Путь к журналу скопирован";
  } catch {
    setText("copy-log", "Выделите путь");
    document.getElementById("log-path")?.classList.add("selectable");
  }
}

function applyPreviewState() {
  const preview = params.get("preview") ?? "welcome";
  const requestedMode = params.get("mode");
  const aliases: Record<string, Exclude<InstallMode, "blocked">> = { install: "install", update: "update", repair: "repair" };
  state.mode = aliases[requestedMode ?? ""] ?? aliases[preview] ?? "install";
  state.installedVersion = state.mode === "install" ? null : "1.0.4";
  state.dir = "C:\\Program Files\\Obsession";
  state.logPath = "C:\\Users\\User\\AppData\\Local\\Obsession\\installer.log";

  if (preview === "options") state.step = "options";
  else if (preview === "progress" || preview === "installing") {
    state.step = "installing";
    state.pct = Number(params.get("pct") ?? "59");
    state.stage = params.get("stage") ?? "verify";
  } else if (preview === "done") {
    state.step = "done";
    state.pct = 100;
    state.stage = "finish";
  } else if (preview === "error") {
    state.step = "error";
    state.failure = normalizeInstallerFailure({
      code: "ROLLBACK_RESTORED",
      retryable: true,
      messageCode: "installer.error.rollback_restored",
      logPath: state.logPath,
    }, "INSTALL_FAILED");
    state.retryAction = "install";
  } else if (preview === "blocked") {
    state.step = "error";
    state.mode = "blocked";
    state.installedVersion = "1.3.0";
    state.failure = normalizeInstallerFailure({
      code: "DOWNGRADE_BLOCKED",
      retryable: false,
      messageCode: "installer.error.downgrade_blocked",
      logPath: state.logPath,
    }, "DOWNGRADE_BLOCKED");
  }
}

if (!PREVIEW) {
  void listen<InstallerProgressEvent>("setup-progress", (event) => {
    const progress = applyProgressEvent(
      { sequence: state.progressSequence, pct: state.pct, stage: state.stage },
      event.payload,
    );
    if (progress.sequence === state.progressSequence) return;
    state.progressSequence = progress.sequence;
    state.pct = progress.pct;
    state.stage = progress.stage;
    syncProgressView();
  });
}

bind("btn-min", () => {
  if (!PREVIEW) void invoke("minimize_setup");
});
bind("btn-close", () => void closeSetup());

if (!PREVIEW) document.addEventListener("contextmenu", (event) => event.preventDefault());

async function loadSnapshot() {
  state.step = "bootstrapping";
  state.failure = null;
  state.retryAction = "snapshot";
  render();
  try {
    const snapshot = await invoke<InstallerSnapshot>("installer_snapshot");
    state.dir = snapshot.dir;
    state.mode = snapshot.mode;
    state.installedVersion = snapshot.installedVersion;
    state.logPath = snapshot.logPath;
    state.step = "welcome";
    if (snapshot.mode === "blocked") {
      state.failure = {
        code: "DOWNGRADE_BLOCKED",
        retryable: false,
        messageCode: "installer.error.downgrade_blocked",
        logPath: snapshot.logPath,
      };
      state.step = "error";
    }
  } catch (error) {
    state.failure = normalizeInstallerFailure(error, "PREFLIGHT_FAILED");
    state.logPath = state.failure.logPath ?? state.logPath;
    state.step = "error";
  }
  render();
}

render();

void (async () => {
  if (PREVIEW) {
    applyPreviewState();
    render();
    return;
  }
  await loadSnapshot();
})();
