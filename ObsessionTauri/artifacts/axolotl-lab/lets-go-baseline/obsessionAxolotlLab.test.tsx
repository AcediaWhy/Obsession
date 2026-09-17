import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";

import { ObsessionAxolotlLab } from "./obsessionAxolotlLab";

const labCss = readFileSync(
  fileURLToPath(new URL("./styles/obsessionAxolotlLab.css", import.meta.url)),
  "utf-8",
);

describe("ObsessionAxolotlLab", () => {
  it("uses the production screen geometry instead of a custom dashboard", () => {
    const html = renderToStaticMarkup(<ObsessionAxolotlLab initialTab="dpi" />);
    expect(html).toContain("data-sunken-star-field-lab");
    expect(html).toContain("app-nav-rail");
    expect(html).toContain("screen-split--end-360");
    expect(html).toContain("DPI-обход");
    expect(html).toContain("Zapret");
    expect(html).toContain("Журнал");
  });

  it("starts as an animated idle theme with the full interface visible", () => {
    const html = renderToStaticMarkup(<ObsessionAxolotlLab />);
    expect(html).toContain('data-phase="idle"');
    expect(html).toContain('data-motion="running"');
    expect(html).toContain('data-theme="obsession"');
    expect(html).toContain("Журнал");
    expect(html).toContain("Zapret");
  });

  it("renders the inspector strip next to the screen mock-up", () => {
    const html = renderToStaticMarkup(<ObsessionAxolotlLab />);
    expect(html).toContain("data-axolotl-inspector");
    // Свотчи палитры: четыре рампы по четыре ступени ночного города.
    expect(html).toContain("#0a0c16"); // night.shade
    expect(html).toContain("#ffdd96"); // warm.spec
    // Стенд спрайта показывает все пять фаз рядом.
    for (const phase of ["idle", "engaging", "scanning", "focused", "fault"]) {
      expect(html).toContain(`data-phase="${phase}"`);
    }
  });

  it("publishes the art buffer geometry for the inspector", () => {
    const html = renderToStaticMarkup(<ObsessionAxolotlLab />);
    expect(html).toContain("data-pixel-scale");
    expect(html).toContain("data-art-size");
    expect(html).toContain("--art-px");
  });

  it("mounts a toaster anchor that survives server rendering", () => {
    // sonner читает document.hidden в теле рендера, поэтому её Toaster монтируется
    // только после гидрации — на сервере должен остаться стабильный якорь.
    const html = renderToStaticMarkup(<ObsessionAxolotlLab />);
    expect(html).toContain("data-px-toaster");
    expect(html).not.toContain("data-sonner-toaster");
  });

  it("draws the whole scene procedurally, without raster assets", () => {
    const html = renderToStaticMarkup(<ObsessionAxolotlLab />);
    expect(html).toContain('data-renderer="procedural-canvas"');
    expect(html).not.toContain("<img");
  });

  it("keeps blur out of the lab skin", () => {
    // Доктрина двух разрешений: размытие поверх апскейленного арт-буфера смазывает
    // ровно то, что делает пиксель-арт пиксель-артом. Комментарии из проверки
    // вырезаем — в них слово blur() как раз объясняет, почему его тут нет.
    const rules = labCss.replace(/\/\*[\s\S]*?\*\//g, "");
    const declarations = rules.match(/backdrop-filter:[^;]+/g) ?? [];
    expect(declarations.length).toBeGreaterThan(0);
    for (const declaration of declarations) expect(declaration).toContain("none");
    expect(rules).not.toContain("blur(");
  });
});
