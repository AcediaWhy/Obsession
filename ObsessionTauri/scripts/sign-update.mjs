#!/usr/bin/env node
import { createHash, createPrivateKey, createPublicKey, sign } from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const [setupPath, version] = process.argv.slice(2);
if (!setupPath || !/^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?$/.test(version ?? "")) {
  throw new Error("Usage: node scripts/sign-update.mjs <setup.exe> <version>");
}
const privatePath = process.env.OBSESSION_UPDATE_SIGNING_KEY || path.join(root, ".secrets", "update-private.pem");
const publicPath = path.join(root, "src-tauri", "src", "update_public_key.hex");
const privateKey = createPrivateKey(fs.readFileSync(privatePath));
const actualPublic = createPublicKey(privateKey).export({ type: "spki", format: "der" }).subarray(-32).toString("hex");
if (actualPublic !== fs.readFileSync(publicPath, "utf8").trim()) {
  throw new Error("Signing key does not match the public key pinned in the application");
}
const bytes = fs.readFileSync(setupPath);
const manifest = Buffer.from(`${JSON.stringify({
  schema_version: 1,
  product_id: "com.vlarpsu.obsession",
  version,
  setup_asset: path.basename(setupPath),
  size: bytes.length,
  sha256: createHash("sha256").update(bytes).digest("hex"),
})}\n`, "utf8");
const dir = path.dirname(setupPath);
fs.writeFileSync(path.join(dir, "update.json"), manifest);
fs.writeFileSync(path.join(dir, "update.json.sig"), `${sign(null, manifest, privateKey).toString("hex")}\n`);
console.log("Signed update.json and update.json.sig. Upload both beside the Setup executable.");
