import type { ObsessionChoirMotionSample } from "./types";

const TAU = Math.PI * 2;
const BLADE_COUNT = 9;
const clamp01 = (value: number) => Math.max(0, Math.min(1, value));

export function obsessionChoirSealSpinSpeed(activity: number): number {
  // Nine-fold symmetry makes a small angular velocity clearly visible: the
  // OFF pose repeats roughly every 15 s, the ON pose roughly every 3 s.
  return 0.045 + clamp01(activity) * 0.19;
}

export type ObsessionChoirSealGeometry = {
  outerRadius: number;
  shoulderRadius: number;
  apertureRadius: number;
  pupilRadius: number;
  bladeArc: number;
};

export function obsessionChoirSealGeometry(
  size: number,
  motion: ObsessionChoirMotionSample,
): ObsessionChoirSealGeometry {
  const opening = clamp01(motion.apertureOpen);
  return {
    outerRadius: size * (0.355 + opening * 0.028),
    shoulderRadius: size * (0.205 + clamp01(motion.lineTension) * 0.018),
    apertureRadius: size * (0.105 + opening * 0.024),
    pupilRadius: size * (0.035 + (1 - opening) * 0.018),
    bladeArc: (TAU / BLADE_COUNT) * (0.43 + opening * 0.08),
  };
}

function traceBlade(
  ctx: CanvasRenderingContext2D,
  angle: number,
  geometry: ObsessionChoirSealGeometry,
  curl: number,
) {
  const halfArc = geometry.bladeArc * 0.5;
  const rootA = angle - halfArc * 0.78;
  const rootB = angle + halfArc * 0.7;
  const shoulderA = angle - halfArc - curl * 0.08;
  const shoulderB = angle + halfArc * 0.92 + curl * 0.12;
  const tipA = angle - 0.045 - curl * 0.025;
  const tipB = angle + 0.055 + curl * 0.02;

  ctx.beginPath();
  ctx.moveTo(
    Math.cos(rootA) * geometry.apertureRadius,
    Math.sin(rootA) * geometry.apertureRadius,
  );
  ctx.bezierCurveTo(
    Math.cos(shoulderA) * geometry.shoulderRadius * 0.88,
    Math.sin(shoulderA) * geometry.shoulderRadius * 0.88,
    Math.cos(tipA) * geometry.outerRadius * 0.86,
    Math.sin(tipA) * geometry.outerRadius * 0.86,
    Math.cos(angle) * geometry.outerRadius,
    Math.sin(angle) * geometry.outerRadius,
  );
  ctx.bezierCurveTo(
    Math.cos(tipB) * geometry.outerRadius * 0.77,
    Math.sin(tipB) * geometry.outerRadius * 0.77,
    Math.cos(shoulderB) * geometry.shoulderRadius,
    Math.sin(shoulderB) * geometry.shoulderRadius,
    Math.cos(rootB) * geometry.apertureRadius,
    Math.sin(rootB) * geometry.apertureRadius,
  );
  ctx.bezierCurveTo(
    Math.cos(angle + halfArc * 0.16) * geometry.apertureRadius * 0.78,
    Math.sin(angle + halfArc * 0.16) * geometry.apertureRadius * 0.78,
    Math.cos(angle - halfArc * 0.18) * geometry.apertureRadius * 0.78,
    Math.sin(angle - halfArc * 0.18) * geometry.apertureRadius * 0.78,
    Math.cos(rootA) * geometry.apertureRadius,
    Math.sin(rootA) * geometry.apertureRadius,
  );
  ctx.closePath();
}

export function drawObsessionChoirSeal(
  ctx: CanvasRenderingContext2D,
  size: number,
  motion: ObsessionChoirMotionSample,
  activity: number,
  spin = 0,
) {
  ctx.clearRect(0, 0, size, size);
  ctx.globalAlpha = 1;
  ctx.globalCompositeOperation = "source-over";

  const geometry = obsessionChoirSealGeometry(size, motion);
  const active = clamp01(activity);
  const gazeX = motion.masterGazeX * size * 0.012;
  const gazeY = -motion.masterGazeY * size * 0.008;
  const breath = 0.982 + motion.lineTension * 0.034;
  const rotation = -Math.PI * 0.5
    + motion.masterGazeX * 0.018
    + motion.faultShear * 0.035
    + (motion.apertureOpen - 0.5) * 0.08
    + spin;

  ctx.save();
  ctx.translate(size * 0.5 + gazeX, size * 0.5 + gazeY);
  ctx.scale(breath, breath * (0.99 + motion.lineTension * 0.018));
  ctx.rotate(rotation);

  const glow = ctx.createRadialGradient(0, 0, size * 0.025, 0, 0, size * 0.48);
  glow.addColorStop(0, `rgba(104, 5, 34, ${0.2 + motion.carmineDepth * 0.2 + active * 0.08})`);
  glow.addColorStop(0.42, "rgba(29, 3, 13, .16)");
  glow.addColorStop(1, "rgba(0, 0, 0, 0)");
  ctx.fillStyle = glow;
  ctx.fillRect(-size * 0.5, -size * 0.5, size, size);

  // Rear crown: a second, offset set of shorter petals lives behind the main
  // seal. It only shows through the gaps, giving the silhouette real depth
  // without changing the nine long blades the user already recognises.
  ctx.save();
  ctx.rotate(-0.11 - motion.apertureOpen * 0.09 + motion.masterGazeY * 0.025 - spin * 0.34);
  ctx.globalCompositeOperation = "screen";
  const rearFill = ctx.createRadialGradient(
    0,
    0,
    geometry.apertureRadius,
    0,
    0,
    geometry.outerRadius * 0.88,
  );
  rearFill.addColorStop(0, `rgba(128, 9, 47, ${0.22 + active * 0.1})`);
  rearFill.addColorStop(0.58, "rgba(50, 17, 31, .2)");
  rearFill.addColorStop(1, "rgba(205, 193, 189, .055)");
  for (let blade = 0; blade < BLADE_COUNT; blade += 1) {
    const angle = ((blade + 0.5) / BLADE_COUNT) * TAU;
    const rootRadius = geometry.apertureRadius * 1.12;
    const rearRadius = geometry.outerRadius * (0.72 + (blade % 3 === 0 ? 0.08 : 0));
    const halfWidth = geometry.bladeArc * 0.38;
    ctx.beginPath();
    ctx.moveTo(
      Math.cos(angle - halfWidth) * rootRadius,
      Math.sin(angle - halfWidth) * rootRadius,
    );
    ctx.bezierCurveTo(
      Math.cos(angle - halfWidth * 1.5) * geometry.shoulderRadius * 0.78,
      Math.sin(angle - halfWidth * 1.5) * geometry.shoulderRadius * 0.78,
      Math.cos(angle - 0.07) * rearRadius * 0.82,
      Math.sin(angle - 0.07) * rearRadius * 0.82,
      Math.cos(angle) * rearRadius,
      Math.sin(angle) * rearRadius,
    );
    ctx.bezierCurveTo(
      Math.cos(angle + 0.08) * rearRadius * 0.74,
      Math.sin(angle + 0.08) * rearRadius * 0.74,
      Math.cos(angle + halfWidth * 1.35) * geometry.shoulderRadius * 0.82,
      Math.sin(angle + halfWidth * 1.35) * geometry.shoulderRadius * 0.82,
      Math.cos(angle + halfWidth) * rootRadius,
      Math.sin(angle + halfWidth) * rootRadius,
    );
    ctx.closePath();
    ctx.fillStyle = rearFill;
    ctx.fill();
    ctx.strokeStyle = `rgba(213, 203, 198, ${0.13 + motion.ritual * 0.15})`;
    ctx.lineWidth = size * 0.002;
    ctx.stroke();
  }
  ctx.restore();

  const bladeFill = ctx.createRadialGradient(0, 0, geometry.pupilRadius, 0, 0, geometry.outerRadius);
  bladeFill.addColorStop(0, `rgba(61, 3, 23, ${0.94 + active * 0.04})`);
  bladeFill.addColorStop(0.34, "rgba(18, 11, 17, .98)");
  bladeFill.addColorStop(0.7, "rgba(7, 5, 9, .98)");
  bladeFill.addColorStop(1, "rgba(29, 10, 18, .92)");

  const inlayGeometry: ObsessionChoirSealGeometry = {
    outerRadius: geometry.outerRadius * 0.72,
    shoulderRadius: geometry.shoulderRadius * 0.72,
    apertureRadius: geometry.apertureRadius * 1.3,
    pupilRadius: geometry.pupilRadius,
    bladeArc: geometry.bladeArc * 0.58,
  };

  for (let blade = 0; blade < BLADE_COUNT; blade += 1) {
    const angle = (blade / BLADE_COUNT) * TAU;
    const curl = Math.sin(blade * 2.17 + motion.lineTension * 2.4);
    traceBlade(ctx, angle, geometry, curl);
    ctx.fillStyle = bladeFill;
    ctx.shadowColor = `rgba(88, 5, 31, ${0.16 + motion.carmineDepth * 0.16})`;
    ctx.shadowBlur = size * 0.028;
    ctx.fill();
    ctx.shadowBlur = 0;
    ctx.strokeStyle = blade % 3 === 0
      ? `rgba(231, 225, 219, ${0.48 + motion.ritual * 0.22})`
      : `rgba(188, 179, 178, ${0.2 + motion.ritual * 0.12})`;
    ctx.lineWidth = size * (blade % 3 === 0 ? 0.004 : 0.0025);
    ctx.stroke();

    traceBlade(ctx, angle, inlayGeometry, curl * 0.42);
    ctx.fillStyle = blade % 3 === 0
      ? `rgba(112, 8, 42, ${0.08 + motion.carmineDepth * 0.1})`
      : "rgba(198, 188, 185, .025)";
    ctx.fill();
    ctx.strokeStyle = blade % 3 === 0
      ? `rgba(184, 34, 72, ${0.17 + active * 0.08})`
      : "rgba(207, 197, 193, .1)";
    ctx.lineWidth = size * 0.0017;
    ctx.stroke();

    ctx.save();
    ctx.globalCompositeOperation = "screen";
    ctx.strokeStyle = `rgba(126, 8, 43, ${0.18 + motion.carmineDepth * 0.3 + active * 0.1})`;
    ctx.lineWidth = size * 0.0035;
    ctx.beginPath();
    ctx.moveTo(
      Math.cos(angle - geometry.bladeArc * 0.2) * geometry.apertureRadius * 1.08,
      Math.sin(angle - geometry.bladeArc * 0.2) * geometry.apertureRadius * 1.08,
    );
    ctx.quadraticCurveTo(
      Math.cos(angle - 0.1) * geometry.shoulderRadius,
      Math.sin(angle - 0.1) * geometry.shoulderRadius,
      Math.cos(angle) * geometry.outerRadius * 0.91,
      Math.sin(angle) * geometry.outerRadius * 0.91,
    );
    ctx.stroke();

    // Split vein: two faint capillaries make every primary blade read as a
    // laminated optical part instead of one empty petal outline.
    for (const side of [-1, 1]) {
      ctx.globalAlpha = 0.48;
      ctx.lineWidth = size * 0.00145;
      ctx.beginPath();
      ctx.moveTo(
        Math.cos(angle - geometry.bladeArc * 0.1) * geometry.apertureRadius * 1.28,
        Math.sin(angle - geometry.bladeArc * 0.1) * geometry.apertureRadius * 1.28,
      );
      ctx.quadraticCurveTo(
        Math.cos(angle + side * geometry.bladeArc * 0.2) * geometry.shoulderRadius * 0.82,
        Math.sin(angle + side * geometry.bladeArc * 0.2) * geometry.shoulderRadius * 0.82,
        Math.cos(angle + side * 0.035) * geometry.outerRadius * 0.7,
        Math.sin(angle + side * 0.035) * geometry.outerRadius * 0.7,
      );
      ctx.stroke();
    }
    ctx.globalAlpha = 1;
    ctx.restore();
  }

  // Small faceted joints visually lock the primary blades to the middle
  // mechanism and keep the larger petals from feeling hollow.
  ctx.save();
  ctx.globalCompositeOperation = "screen";
  const jointSize = size * 0.011;
  for (let blade = 0; blade < BLADE_COUNT; blade += 1) {
    const angle = (blade / BLADE_COUNT) * TAU;
    const radius = geometry.shoulderRadius * 0.79;
    ctx.save();
    ctx.translate(Math.cos(angle) * radius, Math.sin(angle) * radius);
    ctx.rotate(angle + Math.PI * 0.25);
    ctx.fillStyle = blade % 3 === 0
      ? `rgba(226, 216, 211, ${0.22 + motion.ritual * 0.16})`
      : `rgba(123, 13, 47, ${0.2 + motion.carmineDepth * 0.14})`;
    ctx.fillRect(-jointSize * 0.5, -jointSize * 0.5, jointSize, jointSize);
    ctx.restore();
  }
  ctx.restore();

  // Three interlaced guilloche bands bridge the empty space between the long
  // blades. Their counter-rotation is driven by the already-smoothed motion
  // sample, so activation gains depth without introducing a new jump.
  ctx.save();
  ctx.rotate(0.08 - motion.lineTension * 0.16 - motion.masterGazeX * 0.018 - spin * 1.58);
  ctx.globalCompositeOperation = "screen";
  for (let band = 0; band < 3; band += 1) {
    const radius = geometry.shoulderRadius * (0.54 + band * 0.14);
    const targetOffset = band + 2;
    ctx.beginPath();
    for (let blade = 0; blade < BLADE_COUNT; blade += 1) {
      const angle = (blade / BLADE_COUNT) * TAU;
      const targetAngle = ((blade + targetOffset) / BLADE_COUNT) * TAU;
      const controlAngle = (angle + targetAngle) * 0.5 - 0.22 + band * 0.05;
      const controlRadius = geometry.apertureRadius * (0.66 + band * 0.17);
      ctx.moveTo(Math.cos(angle) * radius, Math.sin(angle) * radius);
      ctx.quadraticCurveTo(
        Math.cos(controlAngle) * controlRadius,
        Math.sin(controlAngle) * controlRadius,
        Math.cos(targetAngle) * radius,
        Math.sin(targetAngle) * radius,
      );
    }
    ctx.strokeStyle = band === 1
      ? `rgba(133, 16, 50, ${0.16 + motion.carmineDepth * 0.16})`
      : `rgba(218, 210, 206, ${0.075 + band * 0.025 + motion.ritual * 0.08})`;
    ctx.lineWidth = size * (band === 1 ? 0.002 : 0.00145);
    ctx.stroke();
  }

  // Eighteen translucent inner petals form a middle depth plane. They are
  // deliberately shorter and softer than the primary nine-lobed crown.
  ctx.rotate(-0.13 + motion.apertureOpen * 0.19);
  const middleInner = geometry.pupilRadius * 1.34;
  const middleOuter = geometry.apertureRadius * 1.58;
  for (let petal = 0; petal < BLADE_COUNT * 2; petal += 1) {
    const angle = (petal / (BLADE_COUNT * 2)) * TAU;
    const halfWidth = TAU / (BLADE_COUNT * 2) * 0.32;
    ctx.beginPath();
    ctx.moveTo(
      Math.cos(angle - halfWidth) * middleInner,
      Math.sin(angle - halfWidth) * middleInner,
    );
    ctx.quadraticCurveTo(
      Math.cos(angle - halfWidth * 1.7) * middleOuter * 0.72,
      Math.sin(angle - halfWidth * 1.7) * middleOuter * 0.72,
      Math.cos(angle) * middleOuter,
      Math.sin(angle) * middleOuter,
    );
    ctx.quadraticCurveTo(
      Math.cos(angle + halfWidth * 1.5) * middleOuter * 0.66,
      Math.sin(angle + halfWidth * 1.5) * middleOuter * 0.66,
      Math.cos(angle + halfWidth) * middleInner,
      Math.sin(angle + halfWidth) * middleInner,
    );
    ctx.closePath();
    ctx.fillStyle = petal % 2 === 0
      ? `rgba(104, 6, 38, ${0.1 + motion.carmineDepth * 0.12})`
      : "rgba(205, 197, 193, .035)";
    ctx.fill();
    ctx.strokeStyle = petal % 3 === 0
      ? "rgba(226, 218, 213, .13)"
      : "rgba(126, 12, 45, .12)";
    ctx.lineWidth = size * 0.00135;
    ctx.stroke();
  }
  ctx.restore();

  // Foreground shutter: nine overlapping filled leaves close around the
  // central void. This is the densest plane and visually anchors the crown.
  ctx.save();
  ctx.rotate(-rotation * 0.45 + motion.lineTension * 0.11);
  const shutterFill = ctx.createRadialGradient(
    0,
    0,
    geometry.pupilRadius,
    0,
    0,
    geometry.apertureRadius * 1.2,
  );
  shutterFill.addColorStop(0, "rgba(2, 1, 3, .99)");
  shutterFill.addColorStop(0.62, `rgba(67, 4, 26, ${0.72 + active * 0.12})`);
  shutterFill.addColorStop(1, "rgba(8, 5, 9, .94)");
  for (let blade = 0; blade < BLADE_COUNT; blade += 1) {
    const angle = (blade / BLADE_COUNT) * TAU;
    const next = angle + TAU / BLADE_COUNT;
    const inner = geometry.pupilRadius * 1.04;
    const outer = geometry.apertureRadius * 1.16;
    ctx.beginPath();
    ctx.moveTo(Math.cos(angle) * inner, Math.sin(angle) * inner);
    ctx.bezierCurveTo(
      Math.cos(angle + 0.17) * outer * 0.62,
      Math.sin(angle + 0.17) * outer * 0.62,
      Math.cos(next - 0.15) * outer,
      Math.sin(next - 0.15) * outer,
      Math.cos(next) * inner,
      Math.sin(next) * inner,
    );
    ctx.quadraticCurveTo(
      Math.cos(angle + 0.31) * geometry.pupilRadius * 0.72,
      Math.sin(angle + 0.31) * geometry.pupilRadius * 0.72,
      Math.cos(angle) * inner,
      Math.sin(angle) * inner,
    );
    ctx.closePath();
    ctx.fillStyle = shutterFill;
    ctx.shadowColor = "rgba(0, 0, 0, .72)";
    ctx.shadowBlur = size * 0.014;
    ctx.fill();
    ctx.shadowBlur = 0;
    ctx.strokeStyle = `rgba(218, 209, 205, ${0.18 + (blade % 3 === 0 ? 0.16 : 0) + motion.ritual * 0.22})`;
    ctx.lineWidth = size * 0.0026;
    ctx.stroke();
  }

  // Faceted collar catches light between the shutter and the black core.
  ctx.beginPath();
  for (let facet = 0; facet < BLADE_COUNT * 2; facet += 1) {
    const angle = (facet / (BLADE_COUNT * 2)) * TAU;
    const radius = geometry.pupilRadius * (facet % 2 === 0 ? 2.12 : 1.66);
    const x = Math.cos(angle) * radius;
    const y = Math.sin(angle) * radius;
    if (facet === 0) ctx.moveTo(x, y);
    else ctx.lineTo(x, y);
  }
  ctx.closePath();
  ctx.fillStyle = `rgba(47, 3, 19, ${0.52 + active * 0.16})`;
  ctx.fill();
  ctx.strokeStyle = `rgba(224, 216, 211, ${0.14 + motion.ritual * 0.2})`;
  ctx.lineWidth = size * 0.0022;
  ctx.stroke();

  const voidGradient = ctx.createRadialGradient(0, 0, 0, 0, 0, geometry.pupilRadius * 1.52);
  voidGradient.addColorStop(0, "#000");
  voidGradient.addColorStop(0.68, "rgba(0, 0, 1, .99)");
  voidGradient.addColorStop(1, `rgba(74, 4, 27, ${0.25 + active * 0.18})`);
  ctx.fillStyle = voidGradient;
  ctx.beginPath();
  ctx.arc(0, 0, geometry.pupilRadius * 1.52, 0, TAU);
  ctx.fill();
  ctx.strokeStyle = `rgba(239, 232, 226, ${0.32 + motion.ritual * 0.42})`;
  ctx.lineWidth = size * 0.0038;
  ctx.beginPath();
  ctx.arc(0, 0, geometry.pupilRadius * 1.22, 0, TAU);
  ctx.stroke();

  ctx.fillStyle = `rgba(246, 240, 234, ${0.28 + motion.ritual * 0.3})`;
  ctx.beginPath();
  ctx.ellipse(
    -geometry.pupilRadius * 0.58,
    -geometry.pupilRadius * 0.72,
    size * 0.009,
    size * 0.004,
    -0.35,
    0,
    TAU,
  );
  ctx.fill();
  ctx.restore();
  ctx.restore();
}
