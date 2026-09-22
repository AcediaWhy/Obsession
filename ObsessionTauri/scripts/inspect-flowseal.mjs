// Read-only upstream inspection. Does not execute downloaded BAT files or write
// into the bundle. The caller reviews output before applying local patches.
import { createHash } from "node:crypto";
const sources = ["general.bat", "general (ALT6).bat", "general (ALT2).bat",
  "general (FAKE TLS AUTO ALT).bat", "general (FAKE TLS AUTO ALT2).bat",
  "general (FAKE TLS AUTO ALT3).bat", "general (ALT3).bat", "general (ALT4).bat",
  "general (ALT8).bat", "general (SIMPLE FAKE).bat", "lists/list-google.txt"];
const result = await Promise.all(sources.map(async (path) => {
  const response = await fetch(`https://raw.githubusercontent.com/Flowseal/zapret-discord-youtube/1.10.3/${encodeURI(path)}`,
    { signal: AbortSignal.timeout(15000) });
  if (!response.ok) throw new Error(`${path}: ${response.status}`);
  const content = await response.text();
  return { path, sha256: createHash("sha256").update(content).digest("hex"), content };
}));
console.log(JSON.stringify(result));
