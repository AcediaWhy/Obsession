import { useEffect, useRef } from "react";
import { useDpiStore } from "../../store/dpiStore";
import { useProxyStore } from "../../store/proxyStore";
import { createRenderLoop, frameQualityScale, type QualityTier, type RenderLoop } from "../render";
import { createSpriteCache } from "./glowSprite";

// Реактивная среда «Aurora»: звёздное небо, над ним — шторы полярного сияния с
// вертикальными лучами, зарево у горизонта и редкие падающие звёзды. В покое —
// прохладный индиго/циан/бирюза-дрейф; при активном обходе/прокси пространство
// «разогревается»: шторы ускоряются и теплеют в магенту, зарево поднимается,
// звёзды разгораются.

const TAU = Math.PI * 2;

// Блик крупной звезды (мягкое ядро + 4-лучевой крест), запечён в спрайт по
// корзинам яркости. Яркость уходит в globalAlpha, радиус — в размер drawImage
// (см. glowSprite.ts). Раньше такой глинт стоил бы createRadialGradient на звезду.
const starGlint = createSpriteCache(6, 48, (sctx, px, k) => {
  const R = px / 2;
  const b = 0.55 + k * 0.45;
  const core = sctx.createRadialGradient(R, R, 0, R, R, R * 0.5);
  core.addColorStop(0, `rgba(255,255,255,${b})`);
  core.addColorStop(0.5, `rgba(202,222,255,${b * 0.4})`);
  core.addColorStop(1, "rgba(202,222,255,0)");
  sctx.fillStyle = core;
  sctx.fillRect(0, 0, px, px);
  // Лучи-спайки: тонкие градиентные полосы по осям.
  const spike = (horizontal: boolean) => {
    const g = horizontal
      ? sctx.createLinearGradient(0, R, px, R)
      : sctx.createLinearGradient(R, 0, R, px);
    g.addColorStop(0, "rgba(220,235,255,0)");
    g.addColorStop(0.5, `rgba(235,244,255,${b * 0.7})`);
    g.addColorStop(1, "rgba(220,235,255,0)");
    sctx.fillStyle = g;
    if (horizontal) sctx.fillRect(0, R - 1, px, 2);
    else sctx.fillRect(R - 1, 0, 2, px);
  };
  spike(true);
  spike(false);
});

export function AuroraField({ paused = false }: { paused?: boolean }) {
  const dpiActive = useDpiStore((s) => s.active);
  const proxyRunning = useProxyStore((s) => s.running);
  const hot = dpiActive || proxyRunning;
  const hotRef = useRef(hot);
  hotRef.current = hot;

  const ref = useRef<HTMLCanvasElement>(null);
  const loopRef = useRef<RenderLoop | null>(null);

  useEffect(() => {
    const canvas = ref.current;
    if (!canvas) return;
    const ctx = canvas.getContext("2d");
    if (!ctx) return;

    // Намеренно рендерим фон в пониженном разрешении: блюр/виньетка скрывают
    // мягкость, зато fill-rate падает в разы (главный источник лагов в WebView2).
    let backingScale = 0.75 * frameQualityScale("high");
    let w = 0;
    let h = 0;

    // Звезда: позиция (px), радиус, флаг «крупная» (получает глинт-спрайт), фазы.
    type Star = { x: number; y: number; r: number; big: boolean; twSpd: number; twPh: number; base: number };
    let stars: Star[] = [];

    // Падающая звезда: почти всегда простаивает, изредка чертит диагональ.
    type Shooter = { on: boolean; x: number; y: number; vx: number; vy: number; life: number; max: number; wait: number };
    const shooters: Shooter[] = [
      { on: false, x: 0, y: 0, vx: 0, vy: 0, life: 0, max: 1, wait: 3 + Math.random() * 8 },
      { on: false, x: 0, y: 0, vx: 0, vy: 0, life: 0, max: 1, wait: 10 + Math.random() * 12 },
    ];

    const seedStars = () => {
      const n = Math.max(60, Math.min(130, Math.round((w * h) / 24000)));
      stars = Array.from({ length: n }, () => {
        const big = Math.random() < 0.12;
        return {
          x: Math.random() * w,
          y: Math.random() * h * 0.9, // низ отдаём под зарево
          r: big ? 1.4 + Math.random() * 1.4 : 0.5 + Math.random() * 0.9,
          big,
          twSpd: 0.4 + Math.random() * 1.5,
          twPh: Math.random() * TAU,
          base: 0.3 + Math.random() * 0.5,
        };
      });
    };

    const resize = (reseed = true) => {
      w = canvas.clientWidth;
      h = canvas.clientHeight;
      canvas.width = Math.max(1, Math.round(w * backingScale));
      canvas.height = Math.max(1, Math.round(h * backingScale));
      ctx.setTransform(backingScale, 0, 0, backingScale, 0, 0); // рисуем в координатах CSS-пикселей
      if (reseed) seedStars();
    };
    resize();
    // resize меняет размеры канваса и стирает кадр — под reduce-motion
    // (замерший цикл) дорисовываем стоп-кадр; на живом цикле invalidate — no-op.
    const onResize = () => {
      resize();
      loop.invalidate();
    };
    window.addEventListener("resize", onResize);

    // Крупные вертикальные шторы, дрейфующие по горизонтали. В палитре —
    // индиго/циан вперемешку с бирюзой/изумрудом; hot теплеет в магенту.
    type Curtain = {
      x: number; width: number; amp: number; freq: number; speed: number; phase: number;
      rays: number; cold: [number, number, number]; hot: [number, number, number];
    };
    const curtains: Curtain[] = [
      { x: 0.14, width: 0.15, amp: 0.05, freq: 1.1, speed: 0.10, phase: 0.0, rays: 7, cold: [99, 102, 241], hot: [139, 92, 246] },
      { x: 0.34, width: 0.20, amp: 0.06, freq: 0.8, speed: 0.07, phase: 1.6, rays: 9, cold: [45, 212, 191], hot: [240, 171, 252] },
      { x: 0.52, width: 0.17, amp: 0.055, freq: 1.3, speed: 0.12, phase: 3.0, rays: 8, cold: [34, 211, 238], hot: [56, 189, 248] },
      { x: 0.70, width: 0.19, amp: 0.05, freq: 0.95, speed: 0.085, phase: 4.5, rays: 8, cold: [16, 185, 129], hot: [244, 114, 182] },
      { x: 0.87, width: 0.14, amp: 0.045, freq: 1.15, speed: 0.11, phase: 5.6, rays: 6, cold: [56, 189, 248], hot: [253, 230, 138] },
    ];

    let t = 0;
    // Сеем от текущего состояния щита: при входе в тему с уже включённым
    // обходом сцена сразу «тёплая», без прогрева на глазах (~1 с).
    let warm = hotRef.current ? 1 : 0;

    const draw = (dt: number) => {
      // Время и разогрев — кадронезависимые (одинаковая скорость на любом мониторе).
      t += dt;
      warm += ((hotRef.current ? 1 : 0) - warm) * (1 - Math.exp(-dt * 2.4));
      ctx.clearRect(0, 0, w, h);
      ctx.globalCompositeOperation = "lighter";

      // ── Звёзды ──
      for (const s of stars) {
        const tw = 0.4 + 0.6 * (0.5 + 0.5 * Math.sin(t * s.twSpd + s.twPh));
        const a = Math.min(1, s.base * tw * (1 + warm * 0.3));
        if (s.big) {
          const R = s.r * 6;
          ctx.globalAlpha = a;
          ctx.drawImage(starGlint(a), s.x - R, s.y - R, R * 2, R * 2);
        } else {
          ctx.globalAlpha = 1;
          ctx.fillStyle = `rgba(214,228,255,${a})`;
          ctx.beginPath();
          ctx.arc(s.x, s.y, s.r, 0, TAU);
          ctx.fill();
        }
      }
      ctx.globalAlpha = 1;

      // ── Зарево у горизонта (без силуэта): волнистая полоса свечения снизу. ──
      const horY = h * (0.82 - warm * 0.06);
      const gr = Math.round(45 + (240 - 45) * warm);
      const gg = Math.round(212 + (171 - 212) * warm);
      const gb = Math.round(191 + (252 - 191) * warm);
      ctx.beginPath();
      ctx.moveTo(0, h);
      for (let x = 0; x <= w; x += 24) {
        const y = horY + Math.sin(x * 0.008 + t * 0.5) * 10 + Math.sin(x * 0.021 - t * 0.3) * 6;
        x === 0 ? ctx.lineTo(0, y) : ctx.lineTo(x, y);
      }
      ctx.lineTo(w, h);
      ctx.closePath();
      const horGrad = ctx.createLinearGradient(0, horY - 40, 0, h);
      const horA = 0.06 + warm * 0.06;
      horGrad.addColorStop(0, `rgba(${gr},${gg},${gb},0)`);
      horGrad.addColorStop(1, `rgba(${gr},${gg},${gb},${horA})`);
      ctx.fillStyle = horGrad;
      ctx.fill();

      // ── Шторы сияния + вертикальные лучи. ──
      const step = 14;
      for (const c of curtains) {
        const cx = c.x * w;
        const half = (c.width * w) / 2;
        const amp = c.amp * w * (1 + warm * 0.5);
        const drift = c.speed * (1 + warm * 0.9);

        const centerAt = (y: number) =>
          cx +
          Math.sin((y / h) * TAU * c.freq + t * drift * 6 + c.phase) * amp +
          Math.sin((y / h) * Math.PI * 1.3 * c.freq - t * drift * 3 + c.phase) * amp * 0.5;

        const [cr, cg, cb] = c.cold;
        const [hr, hg, hb] = c.hot;
        const r = Math.round(cr + (hr - cr) * warm);
        const g = Math.round(cg + (hg - cg) * warm);
        const b = Math.round(cb + (hb - cb) * warm);
        const peak = 0.06 + warm * 0.07;

        // Тело шторы — мягкая заливка вертикальным градиентом.
        ctx.beginPath();
        for (let y = -step; y <= h + step; y += step) {
          const x = centerAt(y) - half;
          y === -step ? ctx.moveTo(x, y) : ctx.lineTo(x, y);
        }
        for (let y = h + step; y >= -step; y -= step) {
          ctx.lineTo(centerAt(y) + half, y);
        }
        ctx.closePath();
        const grad = ctx.createLinearGradient(0, 0, 0, h);
        grad.addColorStop(0.0, `rgba(${r},${g},${b},0)`);
        grad.addColorStop(0.4, `rgba(${r},${g},${b},${peak})`);
        grad.addColorStop(1.0, `rgba(${r},${g},${b},0)`);
        ctx.fillStyle = grad;
        ctx.fill();

        // Лучи: тонкие вертикальные штрихи внутри шторы, мерцают по фазе —
        // «расчёсанная» текстура настоящего сияния. Один градиент на штору.
        const rayGrad = ctx.createLinearGradient(0, 0, 0, h);
        rayGrad.addColorStop(0.0, `rgba(${Math.min(r + 40, 255)},${Math.min(g + 40, 255)},${b},0)`);
        rayGrad.addColorStop(0.42, `rgba(${Math.min(r + 40, 255)},${Math.min(g + 40, 255)},${b},1)`);
        rayGrad.addColorStop(1.0, `rgba(${Math.min(r + 40, 255)},${Math.min(g + 40, 255)},${b},0)`);
        ctx.strokeStyle = rayGrad;
        ctx.lineWidth = 1.4;
        for (let i = 0; i < c.rays; i++) {
          const off = (i / (c.rays - 1) - 0.5) * c.width * w * 0.9;
          const shimmer = 0.12 + 0.2 * (0.5 + 0.5 * Math.sin(t * (1.2 + c.speed * 8) + i * 1.7 + c.phase));
          ctx.globalAlpha = shimmer * (0.6 + warm * 0.8);
          ctx.beginPath();
          for (let y = 0; y <= h; y += step) {
            const x = centerAt(y) + off;
            y === 0 ? ctx.moveTo(x, y) : ctx.lineTo(x, y);
          }
          ctx.stroke();
        }
        ctx.globalAlpha = 1;
      }

      // ── Падающие звёзды. ──
      for (const sh of shooters) {
        if (!sh.on) {
          sh.wait -= dt;
          if (sh.wait <= 0) {
            sh.on = true;
            sh.max = sh.life = 0.8 + Math.random() * 0.5;
            sh.x = w * (0.15 + Math.random() * 0.7);
            sh.y = h * (0.02 + Math.random() * 0.25);
            const dir = Math.random() < 0.5 ? -1 : 1;
            const spd = w * (0.7 + Math.random() * 0.4);
            sh.vx = dir * spd * 0.9;
            sh.vy = spd * 0.4;
          }
          continue;
        }
        sh.life -= dt;
        if (sh.life <= 0) {
          sh.on = false;
          sh.wait = (8 + Math.random() * 12) * (1 - warm * 0.4);
          continue;
        }
        sh.x += sh.vx * dt;
        sh.y += sh.vy * dt;
        const k = sh.life / sh.max; // 1 → 0
        const fade = Math.sin(Math.min(1, k) * Math.PI); // вспыхнула и угасла
        const tail = 0.09;
        const tx = sh.x - sh.vx * tail;
        const ty = sh.y - sh.vy * tail;
        const tg = ctx.createLinearGradient(tx, ty, sh.x, sh.y);
        tg.addColorStop(0, "rgba(220,235,255,0)");
        tg.addColorStop(1, `rgba(235,244,255,${0.6 * fade})`);
        ctx.strokeStyle = tg;
        ctx.lineWidth = 2;
        ctx.beginPath();
        ctx.moveTo(tx, ty);
        ctx.lineTo(sh.x, sh.y);
        ctx.stroke();
        ctx.fillStyle = `rgba(255,255,255,${0.85 * fade})`;
        ctx.beginPath();
        ctx.arc(sh.x, sh.y, 1.6, 0, TAU);
        ctx.fill();
      }

      ctx.globalCompositeOperation = "source-over";
    };
    const onQualityChange = (qualityTier: QualityTier) => {
      const nextScale = 0.75 * frameQualityScale(qualityTier);
      if (Math.abs(nextScale - backingScale) < 0.001) return;
      backingScale = nextScale;
      resize(false);
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

  // Под reduce-motion кадр статичен, но смену состояния щита отражаем:
  // дорисовываем один стоп-кадр (на живом цикле — no-op).
  useEffect(() => {
    loopRef.current?.invalidate();
  }, [hot]);

  useEffect(() => {
    loopRef.current?.setPaused(paused);
  }, [paused]);

  return (
    <div className="pointer-events-none absolute inset-0 overflow-hidden">
      {/* Базовый градиент глубины. */}
      <div className="absolute inset-0 bg-[radial-gradient(ellipse_at_30%_30%,#0c1024_0%,#070810_48%,#040509_100%)]" />
      {/* Мягкие цветные пятна для объёма (индиго, циан, бирюза). */}
      <div
        className="absolute -left-[8%] top-[12%] h-[440px] w-[440px] rounded-full opacity-40"
        style={{ background: "radial-gradient(circle, rgba(99,102,241,0.5), transparent 66%)", filter: "blur(90px)" }}
      />
      <div
        className="absolute right-[4%] top-[6%] h-[380px] w-[380px] rounded-full opacity-30"
        style={{ background: "radial-gradient(circle, rgba(34,211,238,0.45), transparent 66%)", filter: "blur(90px)" }}
      />
      <div
        className="absolute left-[38%] bottom-[6%] h-[360px] w-[360px] rounded-full opacity-25"
        style={{ background: "radial-gradient(circle, rgba(45,212,191,0.4), transparent 66%)", filter: "blur(100px)" }}
      />
      <canvas ref={ref} className="absolute inset-0 h-full w-full" />
      {/* Тёплое свечение по краю — проступает при активности. */}
      <div
        className="absolute inset-0 transition-opacity duration-[1400ms]"
        style={{
          background: "radial-gradient(ellipse at 50% 120%, rgba(240,171,252,0.16), transparent 55%)",
          opacity: hot ? 1 : 0,
        }}
      />
      {/* Виньетка для глубины. */}
      <div className="absolute inset-0 bg-[radial-gradient(ellipse_at_center,transparent_38%,rgba(0,0,0,0.5))]" />
    </div>
  );
}
