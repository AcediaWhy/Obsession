export type YaniCoreGeometry = {
  size: number;
  centerX: number;
  headTop: number;
  headBottom: number;
  headLeft: number;
  headRight: number;
  leftEarTip: { x: number; y: number };
  rightEarTip: { x: number; y: number };
  leftEye: { x: number; y: number };
  rightEye: { x: number; y: number };
  cigaretteEnd: { x: number; y: number };
};

export function yaniCoreGeometry(size: number): YaniCoreGeometry {
  const safe = Math.max(1, Number.isFinite(size) ? size : 1);
  return {
    size: safe,
    centerX: safe * 0.5,
    headTop: safe * 0.17,
    headBottom: safe * 0.86,
    headLeft: safe * 0.13,
    headRight: safe * 0.87,
    leftEarTip: { x: safe * 0.2, y: safe * 0.045 },
    rightEarTip: { x: safe * 0.82, y: safe * 0.035 },
    leftEye: { x: safe * 0.36, y: safe * 0.49 },
    rightEye: { x: safe * 0.64, y: safe * 0.485 },
    cigaretteEnd: { x: safe * 0.77, y: safe * 0.76 },
  };
}
export function yaniGeometryFits(size: number): boolean {
  const g = yaniCoreGeometry(size);
  const points = [
    { x: g.headLeft, y: g.headTop },
    { x: g.headRight, y: g.headBottom },
    g.leftEarTip,
    g.rightEarTip,
    g.leftEye,
    g.rightEye,
    g.cigaretteEnd,
  ];
  return points.every((point) => point.x >= 0 && point.y >= 0 && point.x <= g.size && point.y <= g.size);
}
