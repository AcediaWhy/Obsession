import { test } from "node:test";
import assert from "node:assert/strict";
import { gunzipSync } from "node:zlib";
import { createHash } from "node:crypto";
import { compressMachinePayload } from "./payload-compression.mjs";

test("gzip envelope is deterministic and preserves every original byte", () => {
  const raw = Buffer.concat([Buffer.from("OBSMACH1"), Buffer.alloc(4096, 42)]);
  const packed = compressMachinePayload(raw);
  assert.equal(packed.subarray(0, 8).toString(), "OBSGZIP1");
  assert.equal(packed.readBigUInt64LE(8), BigInt(raw.length));
  assert.deepEqual(packed.subarray(16, 48), createHash("sha256").update(raw).digest());
  assert.deepEqual(gunzipSync(packed.subarray(48)), raw);
  assert.deepEqual(compressMachinePayload(raw), packed);
  assert.ok(packed.length < raw.length);
});

test("encoder refuses invalid or nested packages", () => {
  assert.throws(() => compressMachinePayload(Buffer.alloc(0)));
  assert.throws(() => compressMachinePayload(Buffer.alloc(32)));
  assert.throws(() => compressMachinePayload(compressMachinePayload(Buffer.from("OBSMACH1abcdefgh"))));
});
