// Use scene coordinates and the playback clock, independent of preview zoom.
export function createMeadowInteraction() {
  let pointer = null, lastMotion = -20, travel = 0;
  let attentionAt = -20, attentionSide = 0, attentionCount = 0;
  let gustAt = -20, gustStrength = 0, gustDirection = 1, gustCount = 0;
  return {
    reset() { pointer = null; travel = 0; lastMotion = -20; },
    move(x, y, time, pointerId = 1) {
      const previous = pointer;
      pointer = { x, y, time, pointerId };
      if (!previous || previous.pointerId !== pointerId) return;
      const dx = x - previous.x, dy = y - previous.y;
      const distance = Math.hypot(dx, dy), elapsed = time - previous.time;
      if (distance < 2) { pointer = previous; return; }
      // Discontinuous jumps should not start a gust.
      if (distance > 240 || (elapsed > 0 && distance / elapsed > 3500)) {
        travel = 0; lastMotion = time; return;
      }
      if (time - lastMotion > 2.2 && time - attentionAt > 9) {
        attentionAt = time + .28; attentionSide = x < 349 ? 0 : 1; attentionCount++;
      }
      lastMotion = time;
      travel = travel * Math.exp(-Math.max(0, elapsed) * 3) + Math.abs(dx) + Math.abs(dy) * .4;
      if (travel >= 65 && time - gustAt > 3.2) {
        gustAt = time; gustStrength = Math.min(1, .55 + travel / 260);
        gustDirection = dx < 0 ? -1 : 1; gustCount++; travel = 0;
      }
    },
    sample(time) {
      return { attentionTime: time - attentionAt, attentionSide, attentionCount,
        gustTime: time - gustAt, gustStrength, gustDirection, gustCount };
    },
  };
}
