export type ChoirPoint = { x: number; y: number };

export type ChoirCurve = {
  start: ChoirPoint;
  controlA: ChoirPoint;
  controlB: ChoirPoint;
  end: ChoirPoint;
  depth: 0 | 1 | 2;
  seed: number;
};

export type ChoirEye = {
  x: number;
  y: number;
  rx: number;
  ry: number;
  tilt: number;
};

export type ChoirRibbonGeometry = {
  vertices: Float32Array;
  curveVertexOffsets: readonly number[];
  vertexStride: 8;
};

const fract = (value: number) => value - Math.floor(value);
const hash = (index: number, salt: number) =>
  fract(Math.sin(index * 127.1 + salt * 311.7) * 43758.5453123);

function pointOnCurve(curve: ChoirCurve, t: number): ChoirPoint {
  const oneMinus = 1 - t;
  const a = oneMinus * oneMinus * oneMinus;
  const b = 3 * oneMinus * oneMinus * t;
  const c = 3 * oneMinus * t * t;
  const d = t * t * t;
  return {
    x: curve.start.x * a + curve.controlA.x * b + curve.controlB.x * c + curve.end.x * d,
    y: curve.start.y * a + curve.controlA.y * b + curve.controlB.y * c + curve.end.y * d,
  };
}

function tangentOnCurve(curve: ChoirCurve, t: number): ChoirPoint {
  const oneMinus = 1 - t;
  return {
    x:
      3 * oneMinus * oneMinus * (curve.controlA.x - curve.start.x) +
      6 * oneMinus * t * (curve.controlB.x - curve.controlA.x) +
      3 * t * t * (curve.end.x - curve.controlB.x),
    y:
      3 * oneMinus * oneMinus * (curve.controlA.y - curve.start.y) +
      6 * oneMinus * t * (curve.controlB.y - curve.controlA.y) +
      3 * t * t * (curve.end.y - curve.controlB.y),
  };
}

function makeCurve(index: number): ChoirCurve {
  // Interleave families so every quality tier retains the full compositional
  // spread instead of truncating the lower and vertical layers first.
  const family = index % 6;
  const lane = Math.floor(index / 6);
  const wave = (hash(index, 2.17) - 0.5) * 0.06;
  let start: ChoirPoint;
  let controlA: ChoirPoint;
  let controlB: ChoirPoint;
  let end: ChoirPoint;
  let depth: 0 | 1 | 2;

  if (family === 0) {
    // Quiet horizon threads. Their lanes never cross, so they read as a
    // deliberate engraving rather than a random wireframe.
    const y = 0.075 + lane * 0.122;
    start = { x: -0.08, y };
    controlA = { x: 0.2, y: y + 0.035 + wave };
    controlB = { x: 0.66, y: y - 0.026 - wave * 0.7 };
    end = { x: 1.08, y: y + 0.012 };
    depth = lane === 3 ? 2 : lane === 1 || lane === 6 ? 1 : 0;
  } else if (family === 1) {
    // Counter-flowing ribbons cross the rising fan over the whole plate. The
    // shared offsets keep the family parallel while the long cubic bow gives
    // the guilloche a deliberate, woven silhouette.
    const offset = (lane - 3.5) * 0.043;
    start = { x: -0.08, y: 0.94 + offset };
    controlA = { x: 0.24, y: 0.84 + offset * 0.82 + wave * 0.45 };
    controlB = { x: 0.66, y: 0.28 + offset * 0.58 - wave * 0.3 };
    end = { x: 1.08, y: 0.06 + offset * 0.42 };
    depth = lane % 4 === 2 ? 2 : lane % 2 === 0 ? 1 : 0;
  } else if (family === 2) {
    // A disciplined diagonal fan restores the vector character without the
    // old random crossings: every lane follows the same rising perspective.
    const y = 0.06 + lane * 0.07;
    start = { x: -0.08, y };
    controlA = { x: 0.18, y: y + 0.12 + wave * 0.35 };
    controlB = { x: 0.48, y: 0.35 + lane * 0.065 - wave * 0.3 };
    end = { x: 1.08, y: 0.56 + lane * 0.052 };
    depth = (lane % 3) as 0 | 1 | 2;
  } else if (family === 3) {
    // Four nested eyelid echoes bind the engraving to the focal eye. These
    // are the only short curves and form a recognisable local hierarchy.
    const sign = lane % 2 === 0 ? -1 : 1;
    const ringIndex = Math.floor(lane / 2);
    const spread = 0.07 + ringIndex * 0.035;
    start = { x: 0.46 - ringIndex * 0.025, y: 0.66 + sign * spread * 0.62 };
    controlA = { x: 0.61, y: 0.66 + sign * (spread * 1.5 + wave * 0.4) };
    controlB = { x: 0.9, y: 0.66 + sign * (spread * 1.34 - wave * 0.25) };
    end = { x: 1.07, y: 0.66 + sign * spread * 0.5 };
    depth = ringIndex === 0 ? 2 : ringIndex % 2 === 0 ? 0 : 1;
  } else if (family === 4) {
    // Low bowls carry detail through the formerly empty lower third without
    // aiming at the master eye or competing with the content cards.
    const y = 0.035 + lane * 0.029;
    start = { x: -0.08, y: y + 0.065 };
    controlA = { x: 0.22, y: y - 0.052 + wave * 0.35 };
    controlB = { x: 0.74, y: y - 0.034 - wave * 0.25 };
    end = { x: 1.08, y: y + 0.07 };
    depth = lane % 3 === 0 ? 2 : lane % 3 === 1 ? 1 : 0;
  } else {
    // Near-vertical optical threads provide the missing counterweight on the
    // left and centre. A gentle bow keeps them organic rather than grid-like.
    const x = 0.035 + lane * 0.085;
    start = { x, y: -0.08 };
    controlA = { x: x + 0.032 + wave * 0.22, y: 0.24 };
    controlB = { x: x + 0.082 - wave * 0.16, y: 0.73 };
    end = { x: x + 0.12, y: 1.08 };
    depth = lane % 3 === 1 ? 2 : lane % 3 === 2 ? 1 : 0;
  }
  return {
    start,
    controlA,
    controlB,
    end,
    depth,
    seed: hash(index, 14.2),
  };
}

export const CHOIR_CURVES: readonly ChoirCurve[] = Array.from(
  { length: 48 },
  (_, index) => makeCurve(index),
);

export const CHOIR_EYES: readonly ChoirEye[] = [
  { x: 0.16, y: 0.22, rx: 0.082, ry: 0.026, tilt: -0.16 },
  { x: 0.36, y: 0.16, rx: 0.064, ry: 0.022, tilt: 0.1 },
  { x: 0.58, y: 0.25, rx: 0.072, ry: 0.024, tilt: -0.08 },
  { x: 0.25, y: 0.43, rx: 0.07, ry: 0.023, tilt: 0.15 },
  { x: 0.48, y: 0.48, rx: 0.058, ry: 0.019, tilt: -0.2 },
  { x: 0.7, y: 0.51, rx: 0.068, ry: 0.021, tilt: 0.12 },
  { x: 0.13, y: 0.72, rx: 0.074, ry: 0.023, tilt: 0.08 },
  { x: 0.4, y: 0.76, rx: 0.062, ry: 0.02, tilt: -0.12 },
  { x: 0.67, y: 0.79, rx: 0.078, ry: 0.024, tilt: 0.18 },
];

export function createChoirRibbonGeometry(segments = 48): ChoirRibbonGeometry {
  const safeSegments = Math.max(4, Math.round(segments));
  const values: number[] = [];
  const offsets: number[] = [0];
  const pushVertex = (
    position: ChoirPoint,
    normal: ChoirPoint,
    side: number,
    along: number,
    curve: ChoirCurve,
  ) => {
    values.push(
      position.x,
      position.y,
      normal.x,
      normal.y,
      side,
      along,
      curve.depth,
      curve.seed,
    );
  };

  for (const curve of CHOIR_CURVES) {
    for (let segment = 0; segment < safeSegments; segment += 1) {
      const t0 = segment / safeSegments;
      const t1 = (segment + 1) / safeSegments;
      const p0 = pointOnCurve(curve, t0);
      const p1 = pointOnCurve(curve, t1);
      const tangent0 = tangentOnCurve(curve, t0);
      const tangent1 = tangentOnCurve(curve, t1);
      const length0 = Math.max(0.00001, Math.hypot(tangent0.x, tangent0.y));
      const length1 = Math.max(0.00001, Math.hypot(tangent1.x, tangent1.y));
      const n0 = { x: -tangent0.y / length0, y: tangent0.x / length0 };
      const n1 = { x: -tangent1.y / length1, y: tangent1.x / length1 };
      pushVertex(p0, n0, -1, t0, curve);
      pushVertex(p0, n0, 1, t0, curve);
      pushVertex(p1, n1, -1, t1, curve);
      pushVertex(p1, n1, -1, t1, curve);
      pushVertex(p0, n0, 1, t0, curve);
      pushVertex(p1, n1, 1, t1, curve);
    }
    offsets.push(values.length / 8);
  }
  return { vertices: new Float32Array(values), curveVertexOffsets: offsets, vertexStride: 8 };
}

export function choirCurveToSvgPath(curve: ChoirCurve, width = 1000, height = 680): string {
  const p = (point: ChoirPoint) => `${(point.x * width).toFixed(2)} ${((1 - point.y) * height).toFixed(2)}`;
  return `M ${p(curve.start)} C ${p(curve.controlA)}, ${p(curve.controlB)}, ${p(curve.end)}`;
}
