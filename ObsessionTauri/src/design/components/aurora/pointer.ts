import type { AuroraPointer } from "./scene";

type Spring = { value: number; velocity: number };
function advance(spring: Spring, target: number, dt: number, speed: number) {
  // Exact critically damped response: stable even at lower quality frame rates.
  const offset = spring.value - target;
  const carry = spring.velocity + speed * offset;
  const decay = Math.exp(-speed * dt);
  spring.value = target + (offset + carry * dt) * decay;
  spring.velocity = (spring.velocity - speed * carry * dt) * decay;
}
const clamp = (value: number) => Math.max(0, Math.min(1, value));

export class AuroraPointerMotion {
  private readonly x: Spring = { value: 0.5, velocity: 0 };
  private readonly y: Spring = { value: 0.5, velocity: 0 };
  private readonly strength: Spring = { value: 0, velocity: 0 };
  private targetX = 0.5;
  private targetY = 0.5;
  private inside = false;
  private initialized = false;
  private idle = 0;

  move(x: number, y: number) {
    if (!Number.isFinite(x) || !Number.isFinite(y)) return;
    this.targetX = clamp(x); this.targetY = clamp(y);
    if (!this.initialized) {
      this.x.value = this.targetX; this.y.value = this.targetY;
      this.initialized = true;
    }
    this.inside = true; this.idle = 0;
  }
  leave() { this.inside = false; }
  reset() {
    this.inside = false; this.initialized = false; this.idle = 0;
    this.x.value = this.targetX = 0.5; this.x.velocity = 0;
    this.y.value = this.targetY = 0.5; this.y.velocity = 0;
    this.strength.value = 0; this.strength.velocity = 0;
  }
  update(dt: number): AuroraPointer {
    const elapsed = Math.max(0, dt);
    this.idle += elapsed;
    const targetStrength = this.inside ? Math.exp(-Math.max(0, this.idle - 4.5) * 0.55) : 0;
    advance(this.x, this.targetX, elapsed, 6);
    advance(this.y, this.targetY, elapsed, 6);
    advance(this.strength, targetStrength, elapsed, targetStrength > this.strength.value ? 4.6 : 2.8);
    return { x: clamp(this.x.value), y: clamp(this.y.value), strength: clamp(this.strength.value) };
  }
}
