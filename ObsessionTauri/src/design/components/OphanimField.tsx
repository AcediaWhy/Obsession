import { useEffect, useRef } from "react";
import { useDpiStore } from "../../store/dpiStore";
import { useProxyStore } from "../../store/proxyStore";
import { createRenderLoop, frameQualityScale, type QualityTier, type RenderLoop } from "../render";
import { createSpriteCache } from "./glowSprite";
import { drawGreatEye } from "./ophanimEye";

// Реактивная среда темы «Ophanim» — «престольное видение» (Иез. 1):
// - созвездие колёс-в-колёсах (главное справа вверху с Великим Оком в ступице,
//   меньшее слева внизу, дальнее призрачное у центра) — усеяны глазами;
// - лучи славы, радиально расходящиеся от престольного Ока (проступают в бдении);
// - редкие глаза, мягко открывающиеся в темноте; иногда один впивается «взором»;
// - нисходящие столпы света с холодным сапфировым подтоном покоя (Иез. 1:26);
// - пылинки, парящие в лучах.
// Настроение читается из телеметрии щита (dpiStore/proxyStore): покой →
// пробуждение (transitioning) → скан конфигов (testing) → бдение (обход/прокси
// активны). Тепло растёт золото→магента, глаза открываются чаще и шире, сапфир
// покоя отступает, вспыхивают лучи славы.

const TAU = Math.PI * 2;

// Золото покоя → магента разогрева (лерп по warm внутри спрайт-корзин).
const GOLD: [number, number, number] = [253, 224, 71];
const MAGENTA: [number, number, number] = [240, 171, 252];
const lerpC = (c: [number, number, number], h: [number, number, number], k: number) =>
  [Math.round(c[0] + (h[0] - c[0]) * k), Math.round(c[1] + (h[1] - c[1]) * k), Math.round(c[2] + (h[2] - c[2]) * k)] as const;

// Свечение глаза (обод колеса и радужки в темноте): белый зрачок-искра →
// цвет по warm → прозрачность. Яркость уходит в globalAlpha, радиус — в размер
// drawImage (см. glowSprite.ts).
const eyeSprite = createSpriteCache(9, 64, (sctx, px, k) => {
  const [r, g, b] = lerpC(GOLD, MAGENTA, k);
  const R = px / 2;
  const grad = sctx.createRadialGradient(R, R, 0, R, R, R);
  grad.addColorStop(0, "rgba(255,255,255,1)");
  grad.addColorStop(0.4, `rgba(${r},${g},${b},${1 / 1.4})`);
  grad.addColorStop(1, "rgba(0,0,0,0)");
  sctx.fillStyle = grad;
  sctx.fillRect(0, 0, px, px);
});

// Радужка Великого Ока теперь в общем модуле ./ophanimEye (drawGreatEye) —
// вместе с усадкой в гнездо; дубль-спрайт здесь убран.

// Луч славы — мягкий клин, вершина у ступицы (сверху), тает к концу. Запечён по
// корзинам warm; поворот/масштаб на кадре — никакого createLinearGradient в цикле.
const gloryRay = createSpriteCache(9, 128, (sctx, px, k) => {
  const [r, g, b] = lerpC(GOLD, MAGENTA, k);
  const grad = sctx.createLinearGradient(0, 0, 0, px);
  grad.addColorStop(0, `rgba(${r},${g},${b},0.85)`);
  grad.addColorStop(0.5, `rgba(${r},${g},${b},0.26)`);
  grad.addColorStop(1, "rgba(0,0,0,0)");
  sctx.fillStyle = grad;
  sctx.beginPath();
  sctx.moveTo(px / 2, 0); // вершина у ступицы
  sctx.lineTo(px * 0.86, px);
  sctx.lineTo(px * 0.14, px);
  sctx.closePath();
  sctx.fill();
});

// Глаз в темноте: жизненный цикл закрыт → открывается → смотрит → закрывается.
// Позиция и размер перевыбираются на каждое закрытие; координаты — доли
// экрана, чтобы переживать resize.
type DarkEye = {
  fx: number; // доля ширины
  fy: number; // доля высоты
  size: number; // ширина глаза, px
  state: 0 | 1 | 2 | 3; // 0 закрыт, 1 открывается, 2 смотрит, 3 закрывается
  tLeft: number; // осталось в текущем состоянии, сек
  dur: number; // полная длительность состояния, сек
  driftPh: number; // фаза дрейфа зрачка и моргания
  gaze: boolean; // редкий «пристальный взор» — дольше, шире зрачок, медленно гаснет
};

export function OphanimField({ paused = false }: { paused?: boolean }) {
  const dpiActive = useDpiStore((s) => s.active);
  const proxyRunning = useProxyStore((s) => s.running);
  const transitioning = useDpiStore((s) => s.transitioning);
  const testing = useDpiStore((s) => s.testing);
  const testResults = useDpiStore((s) => s.testResults);
  const hot = dpiActive || proxyRunning;
  // Тревога — среди результатов текущего теста есть провал.
  const alarm = testing && Object.values(testResults).some((v) => !v);
  // Всё настроение — в ref, чтобы цикл не пересоздавался на смене состояния.
  const moodRef = useRef({ hot, scanning: testing, alarm, transitioning });
  moodRef.current = { hot, scanning: testing, alarm, transitioning };

  const ref = useRef<HTMLCanvasElement>(null);
  const loopRef = useRef<RenderLoop | null>(null);

  useEffect(() => {
    const canvas = ref.current;
    if (!canvas) return;
    const ctx = canvas.getContext("2d");
    if (!ctx) return;

    // Пониженное разрешение — как в AuroraField: главный рычаг против лагов.
    let backingScale = 0.75 * frameQualityScale("high");
    let w = 0;
    let h = 0;
    const resize = () => {
      w = canvas.clientWidth;
      h = canvas.clientHeight;
      canvas.width = Math.max(1, Math.round(w * backingScale));
      canvas.height = Math.max(1, Math.round(h * backingScale));
      ctx.setTransform(backingScale, 0, 0, backingScale, 0, 0);
    };
    resize();
    // resize стирает кадр — под reduce-motion дорисуем стоп-кадр (живому — no-op).
    const onResize = () => {
      resize();
      loop.invalidate();
    };
    window.addEventListener("resize", onResize);

    // Наклонные столпы света, нисходящие сверху. Левый столп холодно-сапфировый
    // в покое (Иез. 1:26 «как вид камня сапфира») — тёплая тема с холодной тенью.
    type Shaft = {
      x: number; // доля ширины (верхняя точка)
      width: number; // доля ширины
      lean: number; // горизонтальный снос к низу, доля ширины
      speed: number; // скорость мерцания
      phase: number;
      cold: [number, number, number];
      hot: [number, number, number];
    };
    const shafts: Shaft[] = [
      { x: 0.22, width: 0.10, lean: 0.06, speed: 0.5, phase: 0.0, cold: [92, 116, 196], hot: [232, 121, 249] },
      { x: 0.40, width: 0.14, lean: -0.05, speed: 0.36, phase: 1.4, cold: [253, 224, 71], hot: [240, 171, 252] },
      { x: 0.60, width: 0.11, lean: 0.07, speed: 0.6, phase: 2.7, cold: [250, 204, 21], hot: [217, 70, 239] },
      { x: 0.80, width: 0.09, lean: -0.06, speed: 0.44, phase: 4.1, cold: [253, 230, 138], hot: [244, 114, 182] },
    ];

    // Пылинки, парящие в лучах.
    const N = 26;
    const motes = Array.from({ length: N }, (_, i) => ({
      x: ((i * 97) % 100) / 100,
      y: ((i * 53) % 100) / 100,
      r: 0.6 + ((i * 31) % 10) / 10,
      drift: 0.004 + ((i * 17) % 10) / 1000,
      sway: ((i * 13) % 100) / 100,
    }));

    // Глаза в темноте: пул из 5, паузы длинные — одновременно открыт обычно
    // один. Начальные таймеры разнесены, чтобы не открылись хором на старте.
    const spawnEye = (e: DarkEye) => {
      e.fx = 0.08 + Math.random() * 0.84;
      e.fy = 0.15 + Math.random() * 0.68;
      e.size = 20 + Math.random() * 20;
      e.driftPh = Math.random() * TAU;
      e.gaze = false;
    };
    const darkEyes: DarkEye[] = Array.from({ length: 5 }, (_, i) => {
      const e: DarkEye = { fx: 0, fy: 0, size: 0, state: 0, tLeft: 3 + i * 4 + Math.random() * 4, dur: 1, driftPh: 0, gaze: false };
      spawnEye(e);
      return e;
    });

    // Точка на наклонённом эллипсе вокруг произвольного центра.
    const ellipsePt = (cx: number, cy: number, a: number, rx: number, ry: number, axis: number) => {
      const lx = rx * Math.cos(a);
      const ly = ry * Math.sin(a);
      const c = Math.cos(axis);
      const s = Math.sin(axis);
      return { x: cx + lx * c - ly * s, y: cy + lx * s + ly * c };
    };

    // Обод кольца: двойной штрих (широкое тусклое свечение + узкая яркая нить).
    const strokeRing = (
      cx: number, cy: number, rx: number, ry: number, axis: number,
      rgb: readonly [number, number, number], alpha: number, lw: number,
    ) => {
      ctx.beginPath();
      for (let i = 0; i <= 72; i++) {
        const p = ellipsePt(cx, cy, (i / 72) * TAU, rx, ry, axis);
        i === 0 ? ctx.moveTo(p.x, p.y) : ctx.lineTo(p.x, p.y);
      }
      ctx.closePath();
      const [r, g, b] = rgb;
      ctx.strokeStyle = `rgba(${r},${g},${b},${alpha * 0.5})`;
      ctx.lineWidth = lw * 3;
      ctx.stroke();
      ctx.strokeStyle = `rgba(${Math.min(r + 30, 255)},${Math.min(g + 30, 255)},${b},${alpha})`;
      ctx.lineWidth = lw;
      ctx.stroke();
    };

    let t = 0;
    // Сеем от текущего настроения стражи — вход в тему без прогрева на глазах.
    const mood0 = moodRef.current;
    let warm = mood0.hot ? 1 : mood0.scanning ? 0.6 : mood0.transitioning ? 0.4 : 0;
    let wave = -1; // фаза бегущей «волны моргания» по ободу главного колеса; -1 = неактивна
    let waveCd = 12 + Math.random() * 10; // до первой волны, сек

    // Колесо-в-колесе: два кольца (внешнее почти круглое + внутреннее,
    // «переворачивающееся» в 3D) и бегущие по ободу глаза. waveRim=true — по
    // ободу катится «волна моргания» (сигнатурное событие «Взор»).
    const drawWheel = (
      wcx: number, wcy: number, wR: number,
      axisBase: number, inPhase: number, spin: number, eyes: number, alpha: number, waveRim: boolean,
    ) => {
      const rim = lerpC([234, 179, 8], MAGENTA, warm);
      const rimA = (0.05 + warm * 0.05) * alpha;
      const spinBoost = 1 + warm * 0.8;

      const axisOut = axisBase + 0.08 * Math.sin(t * 0.04 + inPhase);
      strokeRing(wcx, wcy, wR, wR * 0.96, axisOut, rim, rimA, 2);
      const yScaleIn = 0.15 + 0.85 * Math.abs(Math.sin(t * (TAU / 45) * spinBoost + inPhase + 2.1));
      const axisIn = axisBase + 1.2 + 0.05 * Math.sin(t * 0.06 + inPhase + 1);
      strokeRing(wcx, wcy, wR * 0.62, wR * 0.62 * yScaleIn, axisIn, rim, rimA * 1.1, 1.6);

      // Глаза по внешнему ободу: медленно бегут, поодиночке моргают, на «дальней»
      // стороне тусклее (объём). Волна добавляет бегущую вспышку раскрытия.
      for (let e = 0; e < eyes; e++) {
        const a = (e / eyes) * TAU + t * spin * spinBoost;
        const p = ellipsePt(wcx, wcy, a, wR, wR * 0.96, axisOut);
        const depth = 0.55 + 0.45 * (0.5 + 0.5 * Math.sin(a + axisOut));
        const blink = 0.5 + 0.5 * Math.sin(t * 0.9 + e * 1.7);
        let bump = 0;
        if (waveRim && wave >= 0) {
          const an = ((a % TAU) + TAU) % TAU;
          let d = Math.abs(an - wave);
          d = Math.min(d, TAU - d);
          bump = Math.exp(-(d * d) / 0.25) * 0.85; // σ≈0.35 рад
        }
        const ea = (0.10 + warm * 0.20) * depth * (0.35 + 0.65 * blink + bump);
        const rad = 3 + warm * 2 + blink * 1 + bump * 2;
        const R = rad * 2.4;
        ctx.globalAlpha = Math.min(ea * 1.4, 1);
        ctx.drawImage(eyeSprite(warm), p.x - R, p.y - R, R * 2, R * 2);
      }
      ctx.globalAlpha = 1;
    };

    // Великое Око рисует общий модуль ./ophanimEye (drawGreatEye) — усадка в
    // гнездо-ступицу; на выходе он оставляет comp="lighter" для dark-eyes/motes.

    const draw = (dt: number) => {
      const { hot: isHot, scanning, alarm: alarmed, transitioning: waking } = moodRef.current;
      t += dt;
      // Настроение стражи: покой → пробуждение (transitioning) → скан (testing) →
      // бдение (hot). Больше уровней warm, чем прежнее вкл/выкл.
      const warmTarget = isHot ? 1 : scanning ? 0.6 : waking ? 0.4 : 0;
      warm += (warmTarget - warm) * (1 - Math.exp(-dt * 2.4));

      // «Взор» — редкая волна моргания катится по ободу главного колеса (аналог
      // падающей звезды Aurora). В бдении/скане чаще.
      if (wave >= 0) {
        wave += dt * (TAU / 1.3); // прокатывается за ~1.3 с
        if (wave > TAU + 0.6) wave = -1;
      } else {
        waveCd -= dt;
        if (waveCd <= 0) {
          wave = 0;
          waveCd = (16 + Math.random() * 16) * (1 - warm * 0.45);
        }
      }

      ctx.clearRect(0, 0, w, h);
      ctx.globalCompositeOperation = "lighter";

      // ── Столпы света: трапеции сверху вниз, дышат шириной. ──
      for (const sh of shafts) {
        const topX = sh.x * w;
        const botX = topX + sh.lean * w;
        const half = ((sh.width * w) / 2) * (1 + 0.08 * Math.sin(t * 0.25 + sh.phase));
        const flick = 0.75 + Math.sin(t * sh.speed + sh.phase) * 0.25;

        const [r, g, b] = lerpC(sh.cold, sh.hot, warm);
        const peak = (0.07 + warm * 0.10) * flick;

        ctx.beginPath();
        ctx.moveTo(topX - half * 0.5, 0);
        ctx.lineTo(topX + half * 0.5, 0);
        ctx.lineTo(botX + half, h);
        ctx.lineTo(botX - half, h);
        ctx.closePath();

        const grad = ctx.createLinearGradient(0, 0, 0, h);
        grad.addColorStop(0.0, `rgba(${r},${g},${b},${peak})`);
        grad.addColorStop(0.55, `rgba(${r},${g},${b},${peak * 0.6})`);
        grad.addColorStop(1.0, `rgba(${r},${g},${b},0)`);
        ctx.fillStyle = grad;
        ctx.fill();
      }

      // ── Нимбы-дуги у источника света (над верхней кромкой). ──
      const hcx = 0.5 * w;
      const hcy = -0.12 * h;
      const [nr, ng, nb] = lerpC(GOLD, MAGENTA, warm);
      for (let i = 0; i < 2; i++) {
        const rad = (0.3 + i * 0.07) * h;
        const pulse = 0.7 + 0.3 * Math.sin(t * 0.5 + i * 1.6);
        ctx.beginPath();
        ctx.arc(hcx, hcy, rad, TAU * 0.08, TAU * 0.42);
        ctx.strokeStyle = `rgba(${nr},${ng},${nb},${(0.035 + warm * 0.035) * pulse})`;
        ctx.lineWidth = 1.5;
        ctx.stroke();
      }

      // ── Лучи славы от престольного Ока — радиальная глориоль. Гейт по warm:
      //    в покое погашены (сцена спокойна), в бдении вспыхивают. ──
      const gcx = 0.74 * w;
      const gcy = 0.18 * h;
      const glory = warm * 0.16;
      if (glory > 0.015) {
        const rays = 12;
        const rayLen = 0.85 * Math.hypot(w, h);
        const rayW = 0.24 * h;
        for (let i = 0; i < rays; i++) {
          const a = (i / rays) * TAU + t * 0.03;
          ctx.save();
          ctx.translate(gcx, gcy);
          ctx.rotate(a);
          ctx.globalAlpha = glory * (0.7 + 0.3 * Math.sin(t * 0.8 + i));
          ctx.drawImage(gloryRay(warm), -rayW / 2, 0, rayW, rayLen);
          ctx.restore();
        }
        ctx.globalAlpha = 1;
      }

      // ── Созвездие колёс: главное справа вверху (с Оком в ступице), меньшее
      //    слева внизу, дальнее призрачное у центра (только кольца — глубина). ──
      drawWheel(0.74 * w, 0.18 * h, 0.55 * h, -0.3, 0, 0.06, 16, 1, true);
      drawWheel(0.2 * w, 0.82 * h, 0.34 * h, 0.5, 2.3, -0.08, 11, 0.7, false);
      drawWheel(0.46 * w, 0.5 * h, 0.16 * h, 1.1, 4.0, 0.05, 0, 0.4, false);
      drawGreatEye(ctx, 0.74 * w, 0.18 * h, 0.55 * h * 0.34, {
        t,
        warm,
        scanning,
        alarmed,
        bloomAlpha: 0.16 + warm * 0.46,
        socketDepth: 0.8,
      });

      // ── Глаза, открывающиеся в темноте. ──
      for (const e of darkEyes) {
        e.tLeft -= dt;
        if (e.tLeft <= 0) {
          // Смена состояния; паузы закрытости при разогреве короче.
          if (e.state === 0) {
            e.state = 1;
            e.dur = e.tLeft = 0.8;
          } else if (e.state === 1) {
            e.state = 2;
            // Редкий «пристальный взор» — держится дольше, гаснет медленнее.
            e.gaze = Math.random() < 0.18;
            e.dur = e.tLeft = e.gaze ? 3.2 + Math.random() * 2.5 : 2 + Math.random() * 3;
          } else if (e.state === 2) {
            e.state = 3;
            e.dur = e.tLeft = e.gaze ? 1.5 : 0.7;
          } else {
            e.state = 0;
            e.dur = e.tLeft = (6 + Math.random() * 10) * (1 - warm * 0.5);
            spawnEye(e); // новое место — «взгляд из другого угла»
          }
        }
        if (e.state === 0) continue;

        // Открытость века: подъём/спад по состоянию + редкое быстрое моргание
        // (у «взора» — ровный немигающий взгляд).
        let openK =
          e.state === 1 ? 1 - e.tLeft / e.dur : e.state === 3 ? e.tLeft / e.dur : 1;
        if (e.state === 2 && !e.gaze) {
          openK *= 1 - 0.92 * Math.pow(Math.max(0, Math.sin(t * 2.6 + e.driftPh * 3)), 32);
        }
        if (openK < 0.03) continue;

        const ex = e.fx * w;
        const ey = e.fy * h;
        const halfW = e.size / 2;
        const lidH = e.size * 0.32 * openK;

        // Миндалевидные веки — ими же клипаем радужку.
        ctx.save();
        ctx.beginPath();
        ctx.moveTo(ex - halfW, ey);
        ctx.quadraticCurveTo(ex, ey - lidH * 2, ex + halfW, ey);
        ctx.quadraticCurveTo(ex, ey + lidH * 2, ex - halfW, ey);
        ctx.closePath();
        ctx.clip();

        // Радужка — глоу-спрайт; зрачок медленно дрейфует («смотрит»).
        const look = Math.sin(t * 0.5 + e.driftPh) * e.size * 0.08;
        const irisR = e.size * 0.30;
        const R = irisR * 2.2;
        ctx.globalAlpha = 0.32 * openK * (0.7 + 0.3 * warm) * (e.gaze ? 1.15 : 1);
        ctx.drawImage(eyeSprite(warm), ex + look - R, ey - R, R * 2, R * 2);
        ctx.globalAlpha = 1;

        // Зрачок — тёмная точка поверх свечения; у «взора» расширен.
        ctx.globalCompositeOperation = "source-over";
        ctx.beginPath();
        ctx.arc(ex + look, ey, irisR * 0.34 * (0.7 + 0.3 * openK) * (e.gaze ? 1.25 : 1), 0, TAU);
        ctx.fillStyle = "rgba(10,8,4,0.85)";
        ctx.fill();
        ctx.globalCompositeOperation = "lighter";
        ctx.restore();

        // Тонкая световая кромка век.
        const [lr, lg, lb] = lerpC(GOLD, MAGENTA, warm);
        ctx.beginPath();
        ctx.moveTo(ex - halfW, ey);
        ctx.quadraticCurveTo(ex, ey - lidH * 2, ex + halfW, ey);
        ctx.quadraticCurveTo(ex, ey + lidH * 2, ex - halfW, ey);
        ctx.closePath();
        ctx.strokeStyle = `rgba(${lr},${lg},${lb},${0.22 * openK})`;
        ctx.lineWidth = 1;
        ctx.stroke();
      }

      // ── Пылинки — мягко плывут вверх, покачиваясь; ярче при разогреве. ──
      const mr = Math.round(253 - 13 * warm);
      const mg = Math.round(224 - 53 * warm);
      const mb = Math.round(71 + 181 * warm);
      for (const m of motes) {
        const y = (m.y - t * m.drift) % 1;
        const yy = (y < 0 ? y + 1 : y) * h;
        const xx = (m.x + Math.sin(t * 0.4 + m.sway * 6.28) * 0.01) * w;
        const a = (0.12 + warm * 0.28) * (0.4 + 0.6 * Math.sin(t * 1.5 + m.sway * 6.28) ** 2);
        ctx.beginPath();
        ctx.arc(xx, yy, m.r, 0, TAU);
        ctx.fillStyle = `rgba(${mr},${mg},${mb},${a})`;
        ctx.fill();
      }
      ctx.globalCompositeOperation = "source-over";
    };
    const onQualityChange = (qualityTier: QualityTier) => {
      const nextScale = 0.75 * frameQualityScale(qualityTier);
      if (Math.abs(nextScale - backingScale) < 0.001) return;
      backingScale = nextScale;
      resize();
    };
    const loop = createRenderLoop(draw, { role: "field", onQualityChange, paused });
    loopRef.current = loop;
    loop.start();

    return () => {
      loop.dispose();
      loopRef.current = null;
      window.removeEventListener("resize", onResize);
    };
  }, []);

  // Под reduce-motion кадр статичен, но смену настроения стражи отражаем стоп-кадром.
  useEffect(() => {
    loopRef.current?.invalidate();
  }, [hot, transitioning, testing, alarm]);

  useEffect(() => {
    loopRef.current?.setPaused(paused);
  }, [paused]);

  return (
    <div className="pointer-events-none absolute inset-0 overflow-hidden">
      {/* Базовый градиент глубины — тёплое тёмное золото. */}
      <div className="absolute inset-0 bg-[radial-gradient(ellipse_at_50%_0%,#161006_0%,#0a0806_46%,#050404_100%)]" />
      {/* Сапфировая тень престола по низу — холодный подтон покоя (Иез. 1:26),
          отступает, когда престол теплеет в магенту. */}
      <div
        className="absolute inset-0 transition-opacity duration-[1400ms]"
        style={{
          background: "radial-gradient(ellipse at 50% 116%, rgba(56,74,150,0.22), transparent 58%)",
          opacity: hot ? 0 : 1,
        }}
      />
      {/* Мягкие тёплые пятна для объёма. */}
      <div
        className="absolute left-[20%] -top-[6%] h-[420px] w-[420px] rounded-full opacity-35"
        style={{ background: "radial-gradient(circle, rgba(234,179,8,0.5), transparent 66%)", filter: "blur(90px)" }}
      />
      <div
        className="absolute right-[18%] -top-[4%] h-[360px] w-[360px] rounded-full opacity-25"
        style={{ background: "radial-gradient(circle, rgba(250,204,21,0.45), transparent 66%)", filter: "blur(90px)" }}
      />
      <canvas ref={ref} className="absolute inset-0 h-full w-full" />
      {/* Свечение престола по низу — проступает при активности. */}
      <div
        className="absolute inset-0 transition-opacity duration-[1400ms]"
        style={{
          background: "radial-gradient(ellipse at 50% 120%, rgba(240,171,252,0.16), transparent 55%)",
          opacity: hot ? 1 : 0,
        }}
      />
      <div className="absolute inset-0 bg-[radial-gradient(ellipse_at_center,transparent_38%,rgba(0,0,0,0.5))]" />
    </div>
  );
}
