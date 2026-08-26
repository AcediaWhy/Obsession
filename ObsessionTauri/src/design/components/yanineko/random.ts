/** Stable integer hash; unlike Math.random it is reproducible in tests and previews. */
export function yaniHash(seed: number): number {
  let value = seed | 0;
  value = Math.imul(value ^ (value >>> 16), 0x45d9f3b);
  value = Math.imul(value ^ (value >>> 16), 0x45d9f3b);
  value ^= value >>> 16;
  return (value >>> 0) / 0x1_0000_0000;
}
export function yaniSignedHash(seed: number): number {
  return yaniHash(seed) * 2 - 1;
}
