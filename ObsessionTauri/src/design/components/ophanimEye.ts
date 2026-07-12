// Великое Око Офанима — единый исходник для hero-ядра (OphanimCore) и фонового
// поля (OphanimField). Раньше глаз дублировался в обоих (побайтово одинаковые
// спрайты + почти идентичный рисунок) — рассинхрон был вопросом времени.
//
// Задача этого модуля — «усадить» глаз в ступицу престольного колеса, чтобы он
// не выглядел «налепленной в фотошопе сферой»:
//  - тёмное гнездо-впадина (socket) чуть за радужкой утапливает Око в сцену;
//  - мягкий край вместо жёсткого clip() — sfumato-веки формируют миндаль;
//  - свет как в сцене (сверху): запечённое верхнее затенение глазного яблока,
//    маленький катчлайт сверху-слева вместо центрального «блика-шарика»,
//    тёплый отражённый подсвет снизу от свечения ступицы;
//  - полупрозрачная радужка (сцена просвечивает) без жёсткого лимбального кольца;
//  - аддитивная глазурь — Око само светит в ступицу как источник.
//
// Перф: ВСЕ градиенты запечены один раз в createSpriteCache; в кадре — только
// drawImage + несколько дуг/заливок. Ни одного createRadial/LinearGradient за кадр.

import { createSpriteCache } from "./glowSprite";

const TAU = Math.PI * 2;

// Золото покоя → магента разогрева (лерп по warm внутри спрайт-корзин).
const EYE_GOLD: [number, number, number] = [253, 224, 71];
const EYE_MAGENTA: [number, number, number] = [240, 171, 252];
const lerpEye = (k: number) =>
  [
    Math.round(EYE_GOLD[0] + (EYE_MAGENTA[0] - EYE_GOLD[0]) * k),
    Math.round(EYE_GOLD[1] + (EYE_MAGENTA[1] - EYE_GOLD[1]) * k),
    Math.round(EYE_GOLD[2] + (EYE_MAGENTA[2] - EYE_GOLD[2]) * k),
  ] as const;

// Мягкий тёплый ореол (bloom) — используется и как внешний ореол, и как внутренняя
// глазурь. Рисуется в "lighter".
const bloomSprite = createSpriteCache(9, 64, (sctx, px, k) => {
  const [r, g, b] = lerpEye(k);
  const R = px / 2;
  const grad = sctx.createRadialGradient(R, R, 0, R, R, R);
  grad.addColorStop(0, `rgba(${r},${g},${b},0.9)`);
  grad.addColorStop(0.5, `rgba(${r},${g},${b},0.32)`);
  grad.addColorStop(1, "rgba(0,0,0,0)");
  sctx.fillStyle = grad;
  sctx.fillRect(0, 0, px, px);
});

// Радужка — запечена по корзинам warm; рисуется в "source-over". Полупрозрачная
// середина (сцена просвечивает), мягкий лимб без жёсткого кольца, а сверху —
// запечённое затенение (спрайт не поворачивается → постоянный объём бесплатно).
const irisSprite = createSpriteCache(9, 128, (sctx, px, k) => {
  const [r, g, b] = lerpEye(k);
  const R = px / 2;
  const irisR = R * 0.94;
  const disc = sctx.createRadialGradient(R, R, 0, R, R, irisR);
  disc.addColorStop(0, "rgba(255,250,235,0.95)");
  disc.addColorStop(0.35, `rgba(${r},${g},${b},0.85)`);
  disc.addColorStop(0.72, `rgba(${r},${g},${b},0.5)`); // полупрозрачная середина
  disc.addColorStop(
    0.92,
    `rgba(${Math.round(r * 0.5)},${Math.round(g * 0.42)},${Math.round(b * 0.5)},0.35)`, // мягкий лимб
  );
  disc.addColorStop(1, "rgba(0,0,0,0)");
  sctx.fillStyle = disc;
  sctx.beginPath();
  sctx.arc(R, R, irisR, 0, TAU);
  sctx.fill();
  // Волокна радужки — короткие радиальные штрихи, попеременно ярче/тусклее.
  sctx.lineWidth = 1.1;
  for (let i = 0; i < 44; i++) {
    const a = (i / 44) * TAU;
    sctx.strokeStyle = i % 2 === 0 ? "rgba(255,244,210,0.4)" : "rgba(180,150,90,0.28)";
    sctx.beginPath();
    sctx.moveTo(R + Math.cos(a) * irisR * 0.34, R + Math.sin(a) * irisR * 0.34);
    sctx.lineTo(R + Math.cos(a) * irisR * 0.82, R + Math.sin(a) * irisR * 0.82);
    sctx.stroke();
  }
  // Верхнее затенение глазного яблока (свет сверху). source-atop = затемняем только
  // уже нарисованные пиксели, не добавляя альфы → прозрачные углы остаются пустыми,
  // тёмного ореола по краю не возникает.
  const occ = sctx.createLinearGradient(0, 0, 0, px);
  occ.addColorStop(0, "rgba(0,0,0,0.30)");
  occ.addColorStop(0.55, "rgba(0,0,0,0)");
  sctx.globalCompositeOperation = "source-atop";
  sctx.fillStyle = occ;
  sctx.fillRect(0, 0, px, px);
  sctx.globalCompositeOperation = "source-over";
});

// Гнездо-впадина: тёмное кольцо чуть ЗА радужкой. Рисуется в "source-over" поверх
// аддитивно-светлой ступицы → реальное затемнение = впадина, в которой лежит Око.
// Не зависит от warm (1 корзина). Пик тьмы у ρ≈0.66 (при боксе 2·socketR,
// socketR≈1.6·eyeR это ~1.06·eyeR — сразу за краем радужки).
const socketSprite = createSpriteCache(1, 128, (sctx, px) => {
  const R = px / 2;
  const grad = sctx.createRadialGradient(R, R, 0, R, R, R);
  grad.addColorStop(0.0, "rgba(9,6,2,0)");
  grad.addColorStop(0.5, "rgba(9,6,2,0)"); // дырка под глазное яблоко
  grad.addColorStop(0.66, "rgba(9,6,2,1)"); // пик тьмы сразу за радужкой
  grad.addColorStop(0.84, "rgba(9,6,2,0.4)");
  grad.addColorStop(1.0, "rgba(9,6,2,0)");
  sctx.fillStyle = grad;
  sctx.fillRect(0, 0, px, px);
});

// Зрачок — тёмное ядро с мягким краем (без жёсткого чёрного круга). 1 корзина.
const pupilSprite = createSpriteCache(1, 64, (sctx, px) => {
  const R = px / 2;
  const grad = sctx.createRadialGradient(R, R, 0, R, R, R);
  grad.addColorStop(0.0, "rgba(6,4,2,0.96)");
  grad.addColorStop(0.6, "rgba(6,4,2,0.9)");
  grad.addColorStop(0.82, "rgba(12,8,3,0.4)");
  grad.addColorStop(1.0, "rgba(0,0,0,0)");
  sctx.fillStyle = grad;
  sctx.fillRect(0, 0, px, px);
});

// Катчлайт — маленький мягкий блик (влажная поверхность глаза). Рисуется в "lighter".
const specularSprite = createSpriteCache(1, 32, (sctx, px) => {
  const R = px / 2;
  const grad = sctx.createRadialGradient(R, R, 0, R, R, R);
  grad.addColorStop(0.0, "rgba(255,255,255,1)");
  grad.addColorStop(0.5, "rgba(255,252,240,0.5)");
  grad.addColorStop(1.0, "rgba(255,255,255,0)");
  sctx.fillStyle = grad;
  sctx.fillRect(0, 0, px, px);
});

export type GreatEyeOpts = {
  t: number; // часы анимации (замыкание эффекта)
  warm: number; // 0..1 «разогрев» стражи
  scanning: boolean; // идёт тест/автоподбор — Око «ищет» (зрачок метёт по горизонтали)
  alarmed: boolean; // среди результатов есть провал — тревога (красный обод, прищур)
  bloomAlpha: number; // альфа внешнего ореола — настроение сцены (Core ярче, Field сдержаннее)
  socketDepth?: number; // ×глубина гнезда (Field чуть мягче, чтобы не спорить с колёсами)
  glazeStrength?: number; // ×сила внутренней глазури
};

// Рисует Великое Око, усаженное в ступицу. На входе ждёт comp="lighter" (обе сцены
// рисуют аддитивно до этого). На выходе оставляет comp="lighter", globalAlpha=1
// (Field продолжает аддитивно; Core тут же перекрывает своим destination-in).
export function drawGreatEye(
  ctx: CanvasRenderingContext2D,
  cx: number,
  cy: number,
  eyeR: number,
  o: GreatEyeOpts,
): void {
  const { t, warm, scanning, alarmed, bloomAlpha, socketDepth = 1, glazeStrength = 1 } = o;

  // «Взгляд»: в покое лёгкий дрейф; при скане — горизонтальный поиск.
  const look = scanning ? Math.sin(t * 3.0) * eyeR * 0.5 : Math.sin(t * 0.5) * eyeR * 0.1;
  const lookY = scanning ? Math.sin(t * 1.7) * eyeR * 0.12 : Math.sin(t * 0.37) * eyeR * 0.06;
  // Веко (0 раскрыт … 1 сомкнут): редкое быстрое моргание (чаще при скане) +
  // приспущенность в покое + подрагивающий прищур при тревоге.
  const blink = Math.pow(Math.max(0, Math.sin(t * (scanning ? 5.2 : 2.4))), 40);
  const drowsy = (1 - warm) * 0.35;
  const alarmNarrow = alarmed ? 0.32 + 0.18 * Math.sin(t * 12) : 0;
  const lid = Math.min(1, blink + drowsy + alarmNarrow);
  const openH = eyeR * (1 - lid);
  const openFrac = Math.max(0, Math.min(1, openH / eyeR)); // гейт аддитивных бликов

  // 1. Внешний ореол (в "lighter", как рисуют всё до этого).
  const glowR = eyeR * 2.0;
  ctx.globalAlpha = bloomAlpha;
  ctx.drawImage(bloomSprite(warm), cx - glowR, cy - glowR, glowR * 2, glowR * 2);
  ctx.globalAlpha = 1;

  // Тёмные детали — в "source-over" (под "lighter" чёрное = 0, исчезло бы).
  ctx.globalCompositeOperation = "source-over";

  // 2. Гнездо-впадина — тёмное кольцо за радужкой утапливает Око в ступицу.
  const socketR = eyeR * 1.6;
  ctx.globalAlpha = Math.max(0, (0.62 - warm * 0.22) * socketDepth);
  ctx.drawImage(socketSprite(0), cx - socketR, cy - socketR, socketR * 2, socketR * 2);
  ctx.globalAlpha = 1;

  // 3. Радужка — мягкий диск (без клипа; верхнее затенение запечено в спрайт).
  ctx.drawImage(irisSprite(warm), cx - eyeR, cy - eyeR, eyeR * 2, eyeR * 2);

  // 4. Зрачок — тёмное ядро с мягким краем, «смотрит» (look/lookY).
  const pupilR = eyeR * (0.4 - warm * 0.12 + (scanning ? 0.05 : 0));
  ctx.drawImage(pupilSprite(0), cx + look - pupilR, cy + lookY - pupilR, pupilR * 2, pupilR * 2);

  // 5. Sfumato-веки (замена clip()): единый путь «круг радужки минус миндаль» с
  //    evenodd закрашивает веки тёмным, оставляя миндалевидную апертуру. Две крышки
  //    разной высоты дают мягкий, а не резаный край. Самоограничено — без save/clip.
  const lidCap = (mul: number, a: number) => {
    ctx.beginPath();
    ctx.arc(cx, cy, eyeR, 0, TAU); // внешний контур — круг радужки
    ctx.moveTo(cx - eyeR, cy); // миндаль-«дырка» (evenodd вычтет её из круга)
    ctx.quadraticCurveTo(cx, cy - openH * mul, cx + eyeR, cy);
    ctx.quadraticCurveTo(cx, cy + openH * mul, cx - eyeR, cy);
    ctx.closePath();
    ctx.fillStyle = `rgba(7,5,2,${a})`;
    ctx.fill("evenodd");
  };
  lidCap(1.6, 0.55);
  lidCap(1.85, 0.4);

  // 6. Тревога — красноватый пульсирующий обод по краю радужки.
  if (alarmed) {
    const pulse = 0.4 + 0.6 * (0.5 + 0.5 * Math.sin(t * 12));
    ctx.strokeStyle = `rgba(239,68,68,${0.5 * pulse})`;
    ctx.lineWidth = 2;
    ctx.beginPath();
    ctx.arc(cx, cy, eyeR * 0.9, 0, TAU);
    ctx.stroke();
  }

  // 7. Световая кромка века — тонкая люминесцентная линия миндаля; верхняя дуга
  //    ярче (свет сверху). Держит контур глаза читаемым при мягкой sfumato-заливке.
  const [lr, lg, lb] = lerpEye(warm);
  const rimA = Math.max(0, 0.5 - lid * 0.3);
  ctx.lineWidth = 1.5;
  ctx.strokeStyle = `rgba(${lr},${lg},${lb},${rimA})`;
  ctx.beginPath();
  ctx.moveTo(cx - eyeR, cy);
  ctx.quadraticCurveTo(cx, cy - openH * 1.6, cx + eyeR, cy);
  ctx.stroke();
  ctx.strokeStyle = `rgba(${lr},${lg},${lb},${rimA * 0.5})`;
  ctx.beginPath();
  ctx.moveTo(cx + eyeR, cy);
  ctx.quadraticCurveTo(cx, cy + openH * 1.6, cx - eyeR, cy);
  ctx.stroke();

  // Аддитивные блики — в "lighter": Око светит в ступицу как источник.
  ctx.globalCompositeOperation = "lighter";

  // 8. Тёплый отражённый подсвет по нижнему лимбу (свечение ступицы, отражённое).
  if (openFrac > 0) {
    ctx.strokeStyle = `rgba(${lr},${lg},${lb},${0.12 * openFrac})`;
    ctx.lineWidth = 2;
    ctx.beginPath();
    ctx.arc(cx, cy, eyeR * 0.7, Math.PI * 0.15, Math.PI * 0.85); // нижняя дуга
    ctx.stroke();
  }

  // 9. Катчлайт — маленький мягкий блик сверху-слева (~11 часов), не по центру.
  const specR = pupilR * 0.6;
  const sx = cx + look * 0.6 - eyeR * 0.16;
  const sy = cy + lookY * 0.6 - eyeR * 0.4;
  ctx.globalAlpha = 0.55 * openFrac;
  ctx.drawImage(specularSprite(0), sx - specR, sy - specR, specR * 2, specR * 2);
  ctx.globalAlpha = 1;

  // 10. Внутренняя глазурь — Око излучает в ступицу (аддитивно поверх тела).
  const glazeR = eyeR * 0.9;
  ctx.globalAlpha = (0.1 + warm * 0.16) * openFrac * glazeStrength;
  ctx.drawImage(bloomSprite(warm), cx - glazeR, cy - glazeR, glazeR * 2, glazeR * 2);
  ctx.globalAlpha = 1;
}
