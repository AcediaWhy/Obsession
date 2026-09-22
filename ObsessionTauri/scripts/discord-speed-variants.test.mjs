import test from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import { parseConfig, resourcesRoot } from "./audit-dpi-configs.mjs";
import path from "node:path";

test("Discord speed candidates change only the two TCP repeat counts", () => {
  const read = id => parseConfig(fs.readFileSync(path.join(resourcesRoot, `configs/discord/discord_${id}.conf`), "utf8"));
  for (const [id, repeats] of [[15, "4"], [16, "2"]]) {
    const baseline = read(14);
    const candidate = read(id);
    let changed = 0;
    for (const profile of baseline.profiles) {
      if (profile.has("--filter-tcp")) {
        assert.deepEqual(profile.get("--dpi-desync-repeats"), ["8"]);
        profile.set("--dpi-desync-repeats", [repeats]);
        changed++;
      }
    }
    assert.equal(changed, 2);
    assert.deepEqual(candidate, baseline);
  }
});
