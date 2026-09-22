import { gzipSync } from "node:zlib";
import { createHash } from "node:crypto";

export const MAX_PAYLOAD_BYTES = 512 * 1024 * 1024;

// Envelope: magic[8], decompressed size u64 LE, SHA-256[32], one gzip member.
// The inner OBSMACH1 manifest, file hashes and path rules remain unchanged.
export function compressMachinePayload(raw) {
  if (raw.length < 16 || raw.length > MAX_PAYLOAD_BYTES || raw.subarray(0, 8).toString("ascii") !== "OBSMACH1") {
    throw new Error("Invalid raw machine payload or size bound exceeded");
  }
  const header = Buffer.alloc(48);
  header.write("OBSGZIP1", 0, "ascii");
  header.writeBigUInt64LE(BigInt(raw.length), 8);
  createHash("sha256").update(raw).digest().copy(header, 16);
  const packed = Buffer.concat([header, gzipSync(raw, { level: 9 })]);
  if (packed.length > MAX_PAYLOAD_BYTES) throw new Error("Compressed payload exceeds size bound");
  return packed;
}
