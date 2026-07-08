import { useEffect, useState } from "react";
import DepthScene from "./DepthScene";
import { SnowLayer } from "./SnowLayer";
import { RussiaField } from "./RussiaField";

// Тема «Russia» — гибрид: фото-глубина (depth-parallax) как среда + генеративный
// снег/грейд поверх. Целевой вайб — vibe.jpg: синие сумерки, заснеженная тропа,
// ели, дальний тёплый огонёк, падающий снег, тихая меланхолия.
//
// Ассеты AI-генерируются офлайн (см. public/depth/README.md) и кладутся в
// public/depth/russia/. Пока их НЕТ — мягко откатываемся на существующую 2D-сцену
// `RussiaField` (у неё свои снег/зерно/виньетка), без битых картинок.
const PHOTO = "/depth/russia/photo.jpg";
const DEPTH = "/depth/russia/depth.png";

function preload(src: string): Promise<void> {
  return new Promise((resolve, reject) => {
    const img = new Image();
    img.onload = () => resolve();
    img.onerror = reject;
    img.src = src;
  });
}

export default function RussiaHybrid() {
  const [ready, setReady] = useState(false);

  useEffect(() => {
    let ok = true;
    // Гейт по фото: если оно есть — заходим в depth-сцену (карта глубины
    // опциональна, DepthScene подставит синтетическую при её отсутствии).
    preload(PHOTO)
      .then(() => ok && setReady(true))
      .catch(() => {
        /* фото нет — остаёмся на 2D-фолбэке RussiaField */
      });
    return () => {
      ok = false;
    };
  }, []);

  // Пока фото+глубина не подтверждены — показываем 2D-сцену Russia.
  if (!ready) return <RussiaField />;

  // invert: карта ideal.png в конвенции «далёкое = светлое», а движок ждёт
  // «ближнее = светлое» — инвертируем. Если параллакс поедет наоборот —
  // поставь invert={false}. Силу — через scale, фокус-плоскость — focus.
  return (
    <DepthScene photoSrc={PHOTO} depthSrc={DEPTH} scale={38} focus={0.5} invert>
      <SnowLayer />
      {/* Грейд под меланхолию: лёгкий холодный синий тон + виньетка. Держим
          деликатным — AI-фото уже в синих сумерках. */}
      <div className="absolute inset-0 bg-[#0e1a2c] opacity-[0.14] mix-blend-soft-light" />
      <div className="absolute inset-0 bg-[radial-gradient(ellipse_at_center,transparent_42%,rgba(4,8,16,0.6))]" />
    </DepthScene>
  );
}
