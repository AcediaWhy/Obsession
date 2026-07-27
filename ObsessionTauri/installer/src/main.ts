import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";

import "./style.css";
// Ассеты бренда — напрямую из основного приложения (единый источник, без копий).
import eyeWebm from "../../src/assets/eye.webm";
import eyePoster from "../../src/assets/eye-poster.png";

// Классический визард в одном окне: welcome → options → installing → done,
// error — боковая ветка из installing. Сама установка — тихий NSIS-движок
// на Rust-стороне (см. src-tauri/src/lib.rs), сюда прилетают события прогресса.

type Step = "welcome" | "options" | "installing" | "done" | "error";
type InstallMode = "install" | "update" | "repair" | "blocked";

interface InstallerSnapshot {
  dir: string;
  mode: InstallMode;
  installedVersion: string | null;
  logPath: string;
}

const VERSION = (import.meta.env.VITE_APP_VERSION as string | undefined) ?? "dev";
const REDUCE_MOTION = window.matchMedia("(prefers-reduced-motion: reduce)").matches;

const STAGE_TEXT: Record<string, string> = {
  prepare: "подготовка…",
  stop: "остановка установленной версии…",
  stage: "подготовка staging-каталога…",
  install: "распаковка проверенной версии…",
  verify: "проверка целостности…",
  commit: "атомарная замена файлов…",
  registry: "фиксация системных записей…",
  shortcuts: "настройка ярлыков…",
  rollback: "восстановление предыдущей версии…",
  cleanup: "очистка временных файлов…",
  finish: "завершение…",
};
// Порядок точек степпера; error остаётся на точке установки, но красной.
const DOT_INDEX: Record<Step, number> = { welcome: 0, options: 1, installing: 2, error: 2, done: 3 };

const state = {
  step: "welcome" as Step,
  mode: "install" as InstallMode,
  dir: "",
  installedVersion: null as string | null,
  logPath: "",
  desktop: true,
  startMenu: true,
  pct: 0,
  stage: "prepare",
  error: "",
};

const MODE_TEXT: Record<Exclude<InstallMode, "blocked">, { noun: string; action: string; done: string }> = {
  install: { noun: "установки", action: "Установить", done: "Установка завершена" },
  update: { noun: "обновления", action: "Обновить", done: "Обновление завершено" },
  repair: { noun: "восстановления", action: "Восстановить", done: "Восстановление завершено" },
};

function activeMode(): Exclude<InstallMode, "blocked"> {
  return state.mode === "blocked" ? "repair" : state.mode;
}

const root = document.getElementById("app")!;
root.innerHTML = `
  <div class="glow" aria-hidden="true"></div>
  <header class="titlebar" data-tauri-drag-region>
    <span class="titlebar-label" data-tauri-drag-region>OBSESSION SETUP</span>
    <div class="titlebar-btns">
      <button class="tb-btn" id="btn-min" title="Свернуть" aria-label="Свернуть">–</button>
      <button class="tb-btn tb-close" id="btn-close" title="Закрыть" aria-label="Закрыть">✕</button>
    </div>
  </header>
  <main class="stage" id="stage"></main>
  <footer class="footer">
    <div class="foot-left" id="foot-left"></div>
    <div class="dots" id="dots" aria-hidden="true"></div>
    <div class="foot-right" id="foot-right"></div>
  </footer>
`;

const el = (id: string) => document.getElementById(id)!;
function bind(id: string, fn: () => void) {
  el(id).addEventListener("click", fn);
}

// Живой глаз: video с альфой; при reduce-motion — статичный постер (как EyeLogo).
function eyeHtml(size: number): string {
  const media = REDUCE_MOTION
    ? `<img src="${eyePoster}" alt="" />`
    : `<video src="${eyeWebm}" poster="${eyePoster}" muted loop playsinline autoplay></video>`;
  return `<div class="eye-tile" style="width:${size}px;height:${size}px">${media}</div>`;
}

function renderDots() {
  const idx = DOT_INDEX[state.step];
  el("dots").innerHTML = [0, 1, 2, 3]
    .map((i) => {
      const cls =
        i === idx ? (state.step === "error" ? "dot error" : "dot active") : i < idx ? "dot done" : "dot";
      return `<div class="${cls}"></div>`;
    })
    .join("");
}

function setPathView() {
  const view = document.getElementById("path-view");
  if (view) {
    view.textContent = state.dir;
    view.title = state.dir;
  }
}

function render() {
  const stage = el("stage");
  const left = el("foot-left");
  const right = el("foot-right");
  left.innerHTML = "";
  right.innerHTML = "";
  // Пока NSIS работает, прервать установку безопасно нельзя — прячем ✕
  // (бэкенд дополнительно блокирует CloseRequested).
  el("btn-close").classList.toggle("hidden", state.step === "installing");
  el("btn-min").classList.toggle("hidden", state.step === "installing");
  renderDots();

  switch (state.step) {
    case "welcome":
      {
        const installed = state.installedVersion ? `Установлена v${state.installedVersion}` : "Инструмент обхода DPI-блокировок";
      stage.innerHTML = `
        <section class="step step-center">
          ${eyeHtml(96)}
          <h1 class="wordmark">OBSESSION</h1>
          <div class="version">v${VERSION} · x64</div>
          <p class="tagline">${installed}</p>
        </section>`;
      right.innerHTML = `<button class="btn btn-primary" id="go-options">${state.mode === "install" ? "Далее" : MODE_TEXT[activeMode()].action}</button>`;
      bind("go-options", () => {
        state.step = "options";
        render();
      });
      break;
      }

    case "options": {
      const copy = MODE_TEXT[activeMode()];
      const browse = state.mode === "install" ? `<button class="btn btn-ghost btn-sm" id="browse">Обзор…</button>` : "";
      stage.innerHTML = `
        <section class="step">
          <h2 class="step-title">Параметры ${copy.noun}</h2>
          <div class="glass options-panel">
            <label class="field-label">Путь установки</label>
            <div class="path-row">
              <div class="path-input" id="path-view"></div>
              ${browse}
            </div>
            <label class="check"><input type="checkbox" id="cb-desktop" ${state.desktop ? "checked" : ""}/><span>Ярлык на рабочем столе</span></label>
            <label class="check"><input type="checkbox" id="cb-start" ${state.startMenu ? "checked" : ""}/><span>Ярлык в меню «Пуск»</span></label>
            <div class="hint">${state.mode === "install" ? "Потребуется ~45 МБ свободного места." : "Текущая версия останется доступна до атомарной замены файлов."} Запущенный Obsession будет закрыт.</div>
          </div>
        </section>`;
      setPathView();
      left.innerHTML = `<button class="btn btn-ghost" id="back">Назад</button>`;
      right.innerHTML = `<button class="btn btn-primary" id="do-install">${copy.action}</button>`;
      bind("back", () => {
        state.step = "welcome";
        render();
      });
      if (state.mode === "install") bind("browse", () => void pickDir());
      bind("do-install", () => void startInstall());
      (el("cb-desktop") as HTMLInputElement).onchange = (e) =>
        (state.desktop = (e.target as HTMLInputElement).checked);
      (el("cb-start") as HTMLInputElement).onchange = (e) =>
        (state.startMenu = (e.target as HTMLInputElement).checked);
      break;
    }

    case "installing":
      {
        const copy = MODE_TEXT[activeMode()];
      stage.innerHTML = `
        <section class="step step-center">
          ${eyeHtml(84)}
          <h2 class="step-title">${copy.action}</h2>
          <div class="progress-track"><div class="progress-fill" id="bar" style="width:${state.pct}%"></div></div>
          <div class="stage-line" id="stage-line">${STAGE_TEXT[state.stage] ?? "…"}</div>
        </section>`;
      break;
      }

    case "done":
      {
        const copy = MODE_TEXT[activeMode()];
      stage.innerHTML = `
        <section class="step step-center">
          ${eyeHtml(84)}
          <h2 class="step-title">${copy.done}</h2>
          <div class="stage-line" id="done-path"></div>
        </section>`;
      el("done-path").textContent = state.dir;
      left.innerHTML = `<button class="btn btn-ghost" id="quit">Закрыть</button>`;
      right.innerHTML = `<button class="btn btn-primary" id="launch">Запустить Obsession</button>`;
      bind("quit", () => void invoke("close_setup"));
      bind("launch", () => {
        void invoke("launch_app", { dir: state.dir }).catch((e) => {
          state.error = String(e);
          state.step = "error";
          render();
        });
      });
      break;
      }

    case "error":
      stage.innerHTML = `
        <section class="step step-center">
          <div class="glass error-panel">
            <div class="error-title">Операция не выполнена</div>
            <div class="error-msg" id="err-msg"></div>
          </div>
        </section>`;
      el("err-msg").textContent = state.error;
      left.innerHTML = `<button class="btn btn-ghost" id="quit">Закрыть</button>`;
      right.innerHTML = state.mode === "blocked" ? "" : `<button class="btn btn-primary" id="retry">Повторить</button>`;
      bind("quit", () => void invoke("close_setup"));
      if (state.mode !== "blocked") bind("retry", () => void startInstall());
      break;
  }
}

async function pickDir() {
  const sel = await open({
    directory: true,
    defaultPath: state.dir,
    title: "Куда установить Obsession",
  });
  if (typeof sel === "string" && sel) {
    // В выбранный «голый» каталог ставим подпапкой Obsession, чтобы не
    // рассыпать файлы по чужой директории (NSIS кладёт прямо в /D=).
    state.dir = /\\obsession\\?$/i.test(sel) ? sel.replace(/\\+$/, "") : sel.replace(/\\+$/, "") + "\\Obsession";
    setPathView();
  }
}

let installing = false;

async function startInstall() {
  // Защита от двойного клика по «Повторить»: второй конкурентный invoke
  // ударит по NSIS-движку, пока первый ещё работает.
  if (installing) return;
  installing = true;
  state.step = "installing";
  state.pct = 0;
  state.stage = "prepare";
  render();
  try {
    await invoke("install", { dir: state.dir, desktop: state.desktop, startMenu: state.startMenu });
    state.step = "done";
  } catch (e) {
    state.error = String(e);
    state.step = "error";
  } finally {
    installing = false;
  }
  render();
}

// Прогресс от Rust: обновляем DOM точечно, без пере-рендера шага,
// чтобы не перезапускать видео глаза на каждом событии.
void listen<{ pct: number; stage: string }>("setup-progress", (ev) => {
  state.pct = ev.payload.pct;
  state.stage = ev.payload.stage;
  const bar = document.getElementById("bar");
  if (bar) bar.style.width = `${state.pct}%`;
  const line = document.getElementById("stage-line");
  if (line) line.textContent = STAGE_TEXT[state.stage] ?? "…";
});

bind("btn-min", () => void invoke("minimize_setup"));
bind("btn-close", () => void invoke("close_setup"));
document.addEventListener("contextmenu", (e) => e.preventDefault());

void (async () => {
  try {
    const snapshot = await invoke<InstallerSnapshot>("installer_snapshot");
    state.dir = snapshot.dir;
    state.mode = snapshot.mode;
    state.installedVersion = snapshot.installedVersion;
    state.logPath = snapshot.logPath;
    if (snapshot.mode === "blocked") {
      state.error = `Установлена более новая версия Obsession (${snapshot.installedVersion ?? "неизвестно"}). Понижение до ${VERSION} заблокировано.`;
      state.step = "error";
    }
  } catch (error) {
    state.dir = "C:\\Obsession";
    state.mode = "blocked";
    state.error = String(error);
    state.step = "error";
  }
  render();
})();
