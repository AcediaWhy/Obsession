// Скачивает сабсеты (latin + cyrillic, woff2) шрифтов проекта из Google Fonts и
// кладёт их в src/assets/fonts.
//
// Зачем: css2-эндпоинт Google отдаёт по-настоящему разбитые по unicode-range woff2
// (с комментами /* latin */, /* cyrillic */) ТОЛЬКО для «настоящего» браузера —
// поэтому шлём полный Chrome User-Agent. Иначе прилетает единый .ttf без сабсетов.
// Кириллица у IBM Plex Sans/Mono есть (кроме Plex Sans Condensed — его не берём).
//
// ВАЖНО про начертания:
//   • IBM Plex Sans — ВАРИАТИВНЫЙ (один файл на сабсет покрывает wght 100–700).
//     Запрашиваем ось wght@100..700, кладём как ibm-plex-sans-<subset>.woff2,
//     а в fonts.css объявляем @font-face с диапазоном `font-weight: 100 700`.
//     (Если запросить дискретные веса — Google вернёт ОДИН и тот же файл на каждый,
//      их нельзя пинить одиночным font-weight — вес не интерполируется.)
//   • IBM Plex Mono — СТАТИЧЕСКИЙ (отдельный файл на каждый вес). Берём 400/500/600.
//
// Запуск:  node scripts/fetch-fonts.mjs   (повторно безопасно, перезаписывает файлы)

import { writeFile, mkdir } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

const OUT = join(dirname(fileURLToPath(import.meta.url)), "..", "src", "assets", "fonts");

// Полный desktop-Chrome UA — иначе Google отдаёт .ttf без unicode-range сабсетов.
const UA =
  "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 " +
  "(KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36";

// Какие сабсеты забираем (остальные — greek/vietnamese/latin-ext — не нужны).
const WANT_SUBSETS = new Set(["latin", "cyrillic"]);

const FAMILIES = [
  // Вариативный: одна ось wght@100..700 → один файл на сабсет.
  { family: "IBM+Plex+Sans", slug: "ibm-plex-sans", variable: true, spec: "wght@100..700" },
  // Статический: дискретные веса → файл на каждый вес.
  { family: "IBM+Plex+Mono", slug: "ibm-plex-mono", variable: false, weights: [400, 500, 600] },
];

async function fetchText(url) {
  const res = await fetch(url, { headers: { "User-Agent": UA } });
  if (!res.ok) throw new Error(`css2 → HTTP ${res.status} (${url})`);
  return res.text();
}

// Разбираем: /* <subset> */\n@font-face { ... font-weight: N|N N; ... src: url(URL) ...; }
function parseFaces(css) {
  const re = /\/\*\s*([a-z-]+)\s*\*\/\s*@font-face\s*\{([^}]*)\}/g;
  const faces = [];
  let m;
  while ((m = re.exec(css)) !== null) {
    const subset = m[1];
    const body = m[2];
    const weight = body.match(/font-weight:\s*(\d+)/)?.[1]; // для variable это нижняя граница — не используем
    const src = body.match(/src:\s*url\(([^)]+)\)/)?.[1];
    if (subset && src) faces.push({ subset, weight, src });
  }
  return faces;
}

async function download(url) {
  const res = await fetch(url, { headers: { "User-Agent": UA } });
  if (!res.ok) throw new Error(`woff2 → HTTP ${res.status} (${url})`);
  return Buffer.from(await res.arrayBuffer());
}

async function main() {
  await mkdir(OUT, { recursive: true });
  let written = 0;
  for (const fam of FAMILIES) {
    const spec = fam.variable ? fam.spec : `wght@${fam.weights.join(";")}`;
    const css = await fetchText(
      `https://fonts.googleapis.com/css2?family=${fam.family}:${spec}&display=swap`,
    );
    const faces = parseFaces(css).filter((f) => WANT_SUBSETS.has(f.subset));

    if (fam.variable) {
      // По одному файлу на сабсет (latin/cyrillic), без веса в имени.
      const bySubset = new Map();
      for (const f of faces) if (!bySubset.has(f.subset)) bySubset.set(f.subset, f.src);
      for (const [subset, src] of bySubset) {
        const buf = await download(src);
        const name = `${fam.slug}-${subset}.woff2`;
        await writeFile(join(OUT, name), buf);
        console.log(`  ${name}  (${(buf.length / 1024).toFixed(1)} KB)`);
        written++;
      }
    } else {
      for (const f of faces.filter((f) => fam.weights.includes(Number(f.weight)))) {
        const buf = await download(f.src);
        const name = `${fam.slug}-${f.weight}-${f.subset}.woff2`;
        await writeFile(join(OUT, name), buf);
        console.log(`  ${name}  (${(buf.length / 1024).toFixed(1)} KB)`);
        written++;
      }
    }
  }
  console.log(`\nГотово: записано ${written} файлов в ${OUT}`);
}

main().catch((e) => {
  console.error("Ошибка:", e.message);
  process.exit(1);
});
