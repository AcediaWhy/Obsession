import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { escapeHtml } from "./installerState";
import cat from "../../src-tauri/icons/icon.png";
import "./style.css";
import "./paper.css";
import "./uninstall.css";

type Choices = { settings: boolean; cache: boolean; temporary: boolean };
type Result = { leftovers: string[]; restartRequired: boolean };
type Location = { path: string; category: string; exists: boolean };
const preview = import.meta.env.DEV && new URLSearchParams(location.search).get("preview") === "uninstall";
const root = document.getElementById("app")!;
const choices: Choices = { settings: false, cache: true, temporary: true };
let phase: "choose" | "confirm" | "busy" | "done" | "error" = "choose";
let locations: Location[] = [];
let result: Result | null = null;
let failure = "";
let pct = 0;
let progressText = "Подготовка";
let sequence = -1;

const icons = {
  settings: '<path d="M6 5h12v15H6zM9 2h6v6H9zM9 12h6M9 16h4"/>',
  cache: '<path d="M4 7h16v13H4zM3 3h18v4H3zM9 11h6"/>',
  temporary: '<path d="M5 7h14M9 7V4h6v3M7 7l1 14h8l1-14M10 10v7M14 10v7"/>',
};
const labels = { settings: "Настройки и профили", cache: "Кэш и журналы", temporary: "Временные файлы" };
const descriptions = {
  settings: "Выбранные конфиги, свои списки, профили и история настройки.",
  cache: "Сетевой кэш, логи и данные WebView.",
  temporary: "Оставшиеся временные помощники установщика. Не весь Temp.",
};

function bind(id: string, action: () => void) {
  document.getElementById(id)?.addEventListener("click", action);
}

function render() {
  root.classList.add("uninstall-app");
  const heading = phase === "busy" ? "Убираем за собой…" : phase === "done" ? "До встречи." : phase === "error" ? "Не всё получилось." : phase === "confirm" ? "Всё верно?" : "Уже уходишь?";
  const content = phase === "choose" ? `
    <p class="un-subtitle">Obsession удалим. А что сделать с твоими данными?</p>
    <div class="un-cards">${(Object.keys(choices) as (keyof Choices)[]).map(key => `
      <button class="un-card ${choices[key] ? "selected" : ""}" id="choice-${key}" role="checkbox" aria-checked="${choices[key]}">
        <span class="un-check" aria-hidden="true">${choices[key] ? "✓" : ""}</span>
        <svg viewBox="0 0 24 24" aria-hidden="true">${icons[key]}</svg>
        <strong>${labels[key]}</strong><span>${descriptions[key]}</span>
        <small>${choices[key] ? "Удалить" : "Оставить"}</small>
      </button>`).join("")}</div>
    <div class="un-presets"><button id="keep-settings">Сохранить настройки</button><span>·</span><button id="all-data">Удалить все данные</button></div>
    <details class="un-details"><summary>Где будем убирать?</summary>
      <p>Program Files\\Obsession, служба, служебные данные в ProgramData, ярлыки и автозагрузка.</p>
      ${locations.map(item => `<p><code>${escapeHtml(item.path)}</code>${item.category === "temporary" ? " — только файлы помощника Obsession" : item.exists ? "" : " — не найдено"}</p>`).join("")}
      <p>Только текущий пользователь. Чужие профили, скачанные вручную установщики и общие компоненты Windows не трогаем. Если WebView удерживает файлы, покажем их в остатках.</p>
    </details>` : phase === "confirm" ? `
    <p class="un-subtitle">Приложение и его служба будут удалены.</p>
    <div class="un-receipt">${(Object.keys(choices) as (keyof Choices)[]).map(key => `<p><span>${labels[key]}</span><strong>${choices[key] ? "Удаляем" : "Оставляем"}</strong></p>`).join("")}</div>
    <p class="un-note">Удалённые данные не попадут в корзину. Windows попросит разрешение администратора. Заблокированные системные файлы будут убраны после перезагрузки.</p>` : phase === "busy" ? `
    <p class="un-subtitle" id="un-stage">${escapeHtml(progressText)}</p>
    <div class="un-progress" role="progressbar" aria-valuemin="0" aria-valuemax="100" aria-valuenow="${pct}"><div id="un-fill" style="width:${pct}%"></div></div>
    <p class="un-note">Пока не закрывай окно.</p>` : phase === "done" ? `
    <p class="un-subtitle">Приложение удалено.${!choices.settings ? " Настройки оставлены для возвращения." : ""}</p>
    <p class="un-note">${result?.restartRequired ? "Перезагрузи Windows, чтобы удалить заблокированные файлы." : ""}</p>
    ${result?.leftovers.length ? `<div class="un-warning"><strong>Некоторые данные остались</strong><p>Удалить их не удалось. Не будем скрывать это за надписью «всё чисто».</p><details><summary>Показать остатки (${result.leftovers.length})</summary>${result.leftovers.map(s => `<p>${escapeHtml(s)}</p>`).join("")}</details></div>` : ""}` : `
    <p class="un-subtitle">Удаление остановлено. Часть действий уже могла выполниться.</p><div class="un-warning">${escapeHtml(failure)}</div><p class="un-note">Если служба повреждена, сначала запусти восстановление через инсталлер.</p>`;
  root.innerHTML = `<header class="titlebar" data-tauri-drag-region><div class="titlebar-brand" data-tauri-drag-region>◆ OBSESSION <span class="titlebar-context">/ удаление</span></div><div class="titlebar-actions"><button class="window-button" id="un-min" aria-label="Свернуть">−</button><button class="window-button window-close" id="un-close" aria-label="Закрыть" ${phase === "busy" ? "disabled" : ""}>×</button></div></header>
    <main class="un-main"><section class="un-sheet" aria-labelledby="un-title"><img class="un-cat" src="${cat}" alt=""/><h1 id="un-title" tabindex="-1">${heading}</h1>${content}</section></main>
    <footer class="un-footer"><span class="un-footer-note">${phase === "choose" ? "Отмеченное удалим. Остальное сохраним." : ""}</span><div>${phase === "confirm" ? '<button class="un-secondary" id="un-back">Назад</button>' : ""}<button class="un-action" id="un-next" ${phase === "busy" ? "disabled" : ""}>${phase === "choose" ? "Продолжить →" : phase === "confirm" ? "Удалить Obsession" : phase === "busy" ? "Удаляем…" : "Закрыть"}</button></div></footer>`;
  for (const key of Object.keys(choices) as (keyof Choices)[]) bind(`choice-${key}`, () => {
    choices[key] = !choices[key]; render(); document.getElementById(`choice-${key}`)?.focus();
  });
  bind("keep-settings", () => { Object.assign(choices, { settings: false, cache: true, temporary: true }); render(); });
  bind("all-data", () => { Object.assign(choices, { settings: true, cache: true, temporary: true }); render(); });
  bind("un-back", () => { phase = "choose"; render(); });
  bind("un-min", () => { if (!preview) void invoke("minimize_setup"); });
  bind("un-close", () => void close());
  bind("un-next", () => {
    if (phase === "choose") { phase = "confirm"; render(); document.getElementById("un-title")?.focus(); }
    else if (phase === "confirm") void execute();
    else if (phase !== "busy") void close();
  });
}

async function close() {
  if (phase !== "busy" && !preview) await invoke("close_setup");
}

async function execute() {
  phase = "busy"; render();
  try {
    if (preview) {
      await new Promise(resolve => setTimeout(resolve, 1400));
      result = { leftovers: [], restartRequired: true };
    } else result = await invoke<Result>("uninstall_execute", { options: choices });
    phase = "done";
  } catch (error) { failure = String(error); phase = "error"; }
  render(); document.getElementById("un-title")?.focus();
}

if (!preview) {
  await listen<{ sequence: number; pct: number; stage: string }>("setup-progress", ({ payload }) => {
    if (payload.sequence <= sequence || phase !== "busy") return;
    sequence = payload.sequence; pct = Math.max(pct, Math.min(100, payload.pct));
    progressText = ({ prepare: "Подготавливаем удаление", install: "Останавливаем службу", verify: "Удаляем системные компоненты", cleanup: "Убираем выбранные данные", finish: "Завершаем удаление" } as Record<string, string>)[payload.stage] ?? "Удаляем Obsession";
    const fill = document.getElementById("un-fill"); if (fill) fill.style.width = `${pct}%`;
    fill?.parentElement?.setAttribute("aria-valuenow", String(pct));
    const label = document.getElementById("un-stage"); if (label) label.textContent = progressText;
  });
  try { locations = await invoke<Location[]>("uninstall_preview"); }
  catch (error) { failure = String(error); phase = "error"; }
} else locations = [{ path: "%LOCALAPPDATA%\\vlarpsu\\Obsession", category: "data", exists: true }, { path: "%APPDATA%\\Obsession", category: "data", exists: true }];
render();
