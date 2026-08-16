import type { ObsessionVisualPhase } from "../../obsessionVisualState";
import type { ObsessionEyeMotionSample } from "./eyeMotion";

export type ObsessionEyePalette = {
  phase: ObsessionVisualPhase;
  active: boolean;
};

function almondPath(
  ctx: CanvasRenderingContext2D,
  cx: number,
  cy: number,
  rx: number,
  ry: number,
  asymmetry: number,
): void {
  ctx.beginPath();
  ctx.moveTo(cx - rx, cy);
  ctx.bezierCurveTo(
    cx - rx * 0.55,
    cy - ry * (1 + asymmetry),
    cx + rx * 0.54,
    cy - ry * (1 - asymmetry * 0.35),
    cx + rx,
    cy,
  );
  ctx.bezierCurveTo(
    cx + rx * 0.54,
    cy + ry * (0.9 + asymmetry * 0.25),
    cx - rx * 0.56,
    cy + ry * (0.88 - asymmetry),
    cx - rx,
    cy,
  );
  ctx.closePath();
}

function strokeEllipse(
  ctx: CanvasRenderingContext2D,
  cx: number,
  cy: number,
  rx: number,
  ry: number,
  rotation: number,
  color: string,
  width: number,
  dash: number[] = [],
): void {
  ctx.save();
  ctx.setLineDash(dash);
  ctx.strokeStyle = color;
  ctx.lineWidth = width;
  ctx.beginPath();
  ctx.ellipse(cx, cy, rx, ry, rotation, 0, Math.PI * 2);
  ctx.stroke();
  ctx.restore();
}

export function drawObsessionEye(
  ctx: CanvasRenderingContext2D,
  size: number,
  motion: ObsessionEyeMotionSample,
  palette: ObsessionEyePalette,
): void {
  ctx.clearRect(0, 0, size, size);
  const cx = size * 0.5;
  const cy = size * 0.5;
  const tension = motion.bodyTension;
  const open = motion.lidOpen;
  const bodyX = motion.gazeX * size * 0.029;
  const bodyY = motion.gazeY * size * 0.019;
  const tilt = motion.gazeX * 0.075 + (tension - 0.5) * 0.042;
  const eyeRx = size * (0.424 + tension * 0.034);
  const fullRy = size * (0.225 + tension * 0.022);
  const eyeRy = fullRy * (0.085 + open * 0.915);
  const irisX = motion.gazeX * size * 0.047;
  const irisY = motion.gazeY * size * 0.03;
  const irisR = size * (0.118 - tension * 0.009);
  const pupilR = irisR * 0.34 * motion.pupilScale;
  const carmine = palette.active || palette.phase !== "idle";

  // ─── 1. ВНЕШНИЕ ВЕКТОРНЫЕ ОРБИТЫ И ШКАЛЫ (ОПТИЧЕСКАЯ АСТРОЛЯБИЯ) ────────────
  ctx.save();
  ctx.translate(cx, cy);
  ctx.rotate(motion.irisRotation * 0.15);
  ctx.globalCompositeOperation = "screen";

  // Наружное кольцо с микро-засечками
  strokeEllipse(ctx, 0, 0, size * 0.46, size * 0.44, 0, "rgba(220,226,236,.18)", size * 0.0035, [size * 0.012, size * 0.04]);
  // Внутреннее карминово-серебряное кольцо с угловыми секциями
  strokeEllipse(ctx, 0, 0, size * 0.39, size * 0.37, -motion.irisRotation * 0.22, carmine ? "rgba(180,26,62,.32)" : "rgba(140,145,160,.2)", size * 0.005, [size * 0.08, size * 0.04]);
  // Тонкое координатное кольцо
  strokeEllipse(ctx, 0, 0, size * 0.32, size * 0.30, motion.irisRotation * 0.08, "rgba(235,238,245,.14)", size * 0.0025, [size * 0.004, size * 0.025]);

  // Радиальные векторные риски по кругу
  for (let rIndex = 0; rIndex < 12; rIndex += 1) {
    const rAngle = (rIndex / 12) * Math.PI * 2;
    ctx.strokeStyle = rIndex % 3 === 0 ? "rgba(235,238,245,.35)" : "rgba(180,185,200,.14)";
    ctx.lineWidth = size * (rIndex % 3 === 0 ? 0.003 : 0.0018);
    ctx.beginPath();
    ctx.moveTo(Math.cos(rAngle) * size * 0.37, Math.sin(rAngle) * size * 0.35);
    ctx.lineTo(Math.cos(rAngle) * size * 0.44, Math.sin(rAngle) * size * 0.42);
    ctx.stroke();
  }
  ctx.restore();

  // ─── 2. ТЕЛО И ВЕКТОРНЫЕ УГЛОВЫЕ СКОБЫ ОКА ───────────────────────────────────
  ctx.save();
  ctx.translate(cx + bodyX, cy + bodyY);
  ctx.rotate(tilt);
  ctx.translate(-cx, -cy);

  // Векторные оптические скобы по углам
  ctx.strokeStyle = "rgba(230,235,245,.45)";
  ctx.lineWidth = size * 0.0035;
  ctx.beginPath();
  // Левая скоба
  ctx.arc(cx - eyeRx * 0.96, cy, size * 0.022, -Math.PI * 0.6, Math.PI * 0.6);
  // Правая скоба
  ctx.arc(cx + eyeRx * 0.96, cy, size * 0.022, Math.PI * 0.4, Math.PI * 1.6);
  ctx.stroke();

  // Объемная тень тела Ока
  ctx.save();
  ctx.shadowColor = palette.phase === "fault" ? "rgba(165,16,52,.7)" : carmine ? "rgba(110,12,38,.58)" : "rgba(0,0,0,.75)";
  ctx.shadowBlur = size * (0.09 + tension * 0.025);
  almondPath(ctx, cx, cy, eyeRx, eyeRy, (tension - 0.5) * 0.08);
  ctx.fillStyle = "rgba(6,7,10,.97)";
  ctx.fill();
  ctx.restore();

  almondPath(ctx, cx, cy, eyeRx, eyeRy, (tension - 0.5) * 0.08);
  ctx.save();
  ctx.clip();

  // Склера с глубоким жемчужно-угольным переливом
  const sclera = ctx.createRadialGradient(
    cx - eyeRx * 0.22,
    cy - eyeRy * 0.42,
    size * 0.015,
    cx,
    cy,
    eyeRx,
  );
  sclera.addColorStop(0, "rgba(240,236,232,.92)");
  sclera.addColorStop(0.28, "rgba(145,143,145,.55)");
  sclera.addColorStop(0.66, "rgba(28,27,32,.85)");
  sclera.addColorStop(1, "rgba(3,3,5,.99)");
  ctx.fillStyle = sclera;
  ctx.fillRect(cx - eyeRx, cy - fullRy, eyeRx * 2, fullRy * 2);

  const sideShade = ctx.createLinearGradient(cx - eyeRx, cy, cx + eyeRx, cy);
  sideShade.addColorStop(0, "rgba(0,0,0,.85)");
  sideShade.addColorStop(0.22, "rgba(0,0,0,.08)");
  sideShade.addColorStop(0.78, "rgba(25,2,10,.08)");
  sideShade.addColorStop(1, "rgba(0,0,0,.9)");
  ctx.fillStyle = sideShade;
  ctx.fillRect(cx - eyeRx, cy - fullRy, eyeRx * 2, fullRy * 2);

  // ─── 3. ВЕКТОРНАЯ РАДУЖКА И ОПТИЧЕСКИЙ ЗРАЧОК ────────────────────────────────
  const ix = cx + irisX;
  const iy = cy + irisY;
  const irisGlow = ctx.createRadialGradient(ix - irisR * 0.24, iy - irisR * 0.28, 0, ix, iy, irisR * 1.18);
  irisGlow.addColorStop(0, "rgba(250,242,236,.92)");
  irisGlow.addColorStop(0.12, carmine ? "rgba(165,24,62,.98)" : "rgba(95,92,98,.92)");
  irisGlow.addColorStop(0.42, carmine ? "rgba(85,6,26,.99)" : "rgba(38,36,42,.99)");
  irisGlow.addColorStop(0.76, "rgba(15,3,9,.99)");
  irisGlow.addColorStop(1, "rgba(1,1,2,1)");
  ctx.fillStyle = irisGlow;
  ctx.beginPath();
  ctx.arc(ix, iy, irisR * 1.08, 0, Math.PI * 2);
  ctx.fill();

  // Параметрические лучи и спирали радужки
  ctx.save();
  ctx.translate(ix, iy);
  ctx.rotate(motion.irisRotation);
  ctx.globalCompositeOperation = "screen";
  for (let index = 0; index < 48; index += 1) {
    const a = (index / 48) * Math.PI * 2;
    const irregular = 0.76 + ((index * 19) % 13) / 36;
    ctx.strokeStyle = index % 4 === 0
      ? "rgba(245,238,232,.45)"
      : carmine ? "rgba(185,28,68,.38)" : "rgba(215,210,205,.18)";
    ctx.lineWidth = size * (index % 6 === 0 ? 0.0045 : 0.0022);
    ctx.beginPath();
    ctx.moveTo(Math.cos(a) * pupilR * 1.1, Math.sin(a) * pupilR * 1.1);
    ctx.quadraticCurveTo(
      Math.cos(a + 0.1) * irisR * 0.65,
      Math.sin(a + 0.1) * irisR * 0.65,
      Math.cos(a + 0.02) * irisR * irregular,
      Math.sin(a + 0.02) * irisR * irregular,
    );
    ctx.stroke();
  }
  ctx.restore();

  // Концентрические кольца радужки
  strokeEllipse(ctx, ix, iy, irisR * 0.95, irisR * 0.95, 0, "rgba(240,232,225,.38)", size * 0.004);
  strokeEllipse(ctx, ix, iy, irisR * 0.74, irisR * 0.74, 0, carmine ? "rgba(145,18,52,.65)" : "rgba(220,212,208,.22)", size * 0.004);
  strokeEllipse(ctx, ix, iy, irisR * 0.48, irisR * 0.48, 0, "rgba(255,248,242,.4)", size * 0.0025);

  if (motion.faultSplit > 0) {
    ctx.globalCompositeOperation = "screen";
    ctx.strokeStyle = `rgba(180,18,56,${0.28 + motion.faultSplit * 0.42})`;
    ctx.lineWidth = size * 0.009;
    ctx.beginPath();
    ctx.arc(ix + size * 0.018 * motion.faultSplit, iy - size * 0.006, irisR * 0.96, 0, Math.PI * 2);
    ctx.stroke();
    ctx.globalCompositeOperation = "source-over";
  }

  // Зрачок с прецизионной оптической апертурой
  const pupil = ctx.createRadialGradient(ix - pupilR * 0.2, iy - pupilR * 0.25, 0, ix, iy, pupilR);
  pupil.addColorStop(0, "#0a080c");
  pupil.addColorStop(0.7, "#020203");
  pupil.addColorStop(1, "#000");
  ctx.fillStyle = pupil;
  ctx.beginPath();
  ctx.arc(ix, iy, pupilR, 0, Math.PI * 2);
  ctx.fill();
  ctx.strokeStyle = "rgba(248,242,236,.65)";
  ctx.lineWidth = size * 0.004;
  ctx.stroke();

  // Влажный блик роговицы
  const sweepAngle = motion.highlightPhase * Math.PI * 2;
  const hx = ix + Math.cos(sweepAngle) * irisR * 0.3 - irisR * 0.24;
  const hy = iy + Math.sin(sweepAngle) * irisR * 0.2 - irisR * 0.3;
  const glint = ctx.createRadialGradient(hx, hy, 0, hx, hy, irisR * 0.45);
  glint.addColorStop(0, "rgba(255,255,252,.99)");
  glint.addColorStop(0.18, "rgba(248,242,236,.7)");
  glint.addColorStop(1, "rgba(248,242,236,0)");
  ctx.fillStyle = glint;
  ctx.fillRect(hx - irisR * 0.5, hy - irisR * 0.5, irisR, irisR);

  const cornea = ctx.createLinearGradient(cx - eyeRx * 0.45, cy - eyeRy, cx + eyeRx * 0.35, cy + eyeRy);
  cornea.addColorStop(0, "rgba(255,255,255,.24)");
  cornea.addColorStop(0.34, "rgba(255,255,255,.04)");
  cornea.addColorStop(0.62, "rgba(95,10,32,.1)");
  cornea.addColorStop(1, "rgba(0,0,0,.25)");
  ctx.fillStyle = cornea;
  ctx.fillRect(cx - eyeRx, cy - fullRy, eyeRx * 2, fullRy * 2);
  ctx.restore();

  // ─── 4. ВЕКТОРНАЯ ОКАНТОВКА ВЕК ──────────────────────────────────────────────
  almondPath(ctx, cx, cy, eyeRx, eyeRy, (tension - 0.5) * 0.08);
  const rim = ctx.createLinearGradient(cx - eyeRx, cy - eyeRy, cx + eyeRx, cy + eyeRy);
  rim.addColorStop(0, "rgba(45,40,46,.55)");
  rim.addColorStop(0.27, "rgba(246,240,234,.82)");
  rim.addColorStop(0.55, carmine ? "rgba(135,20,50,.8)" : "rgba(185,180,178,.42)");
  rim.addColorStop(0.83, "rgba(244,238,232,.65)");
  rim.addColorStop(1, "rgba(22,20,24,.65)");
  ctx.strokeStyle = rim;
  ctx.lineWidth = size * (0.01 + tension * 0.004);
  ctx.shadowColor = "rgba(240,235,230,.22)";
  ctx.shadowBlur = size * 0.028;
  ctx.stroke();
  ctx.shadowBlur = 0;

  ctx.globalCompositeOperation = "screen";
  ctx.strokeStyle = "rgba(255,252,248,.38)";
  ctx.lineWidth = size * 0.0045;
  ctx.beginPath();
  ctx.moveTo(cx - eyeRx * 0.72, cy - eyeRy * 0.48);
  ctx.quadraticCurveTo(cx - eyeRx * 0.05, cy - eyeRy * 1.08, cx + eyeRx * 0.6, cy - eyeRy * 0.5);
  ctx.stroke();
  ctx.restore();
}
