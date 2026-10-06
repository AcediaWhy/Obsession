#!/usr/bin/env node
import { generateKeyPairSync, createPublicKey } from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const keyPath = path.join(root, ".secrets", "update-private.pem");
const publicPath = path.join(root, "src-tauri", "src", "update_public_key.hex");

if (fs.existsSync(keyPath) || fs.existsSync(publicPath)) {
  throw new Error("Update key already exists. Never replace it: old clients trust the pinned public key.");
}

const { privateKey } = generateKeyPairSync("ed25519");
const publicDer = createPublicKey(privateKey).export({ type: "spki", format: "der" });
// Ed25519 SubjectPublicKeyInfo ends with the raw 32-byte public key.
const rawPublic = publicDer.subarray(-32);
fs.mkdirSync(path.dirname(keyPath), { recursive: true });
fs.writeFileSync(keyPath, privateKey.export({ type: "pkcs8", format: "pem" }), { flag: "wx", mode: 0o600 });
fs.writeFileSync(publicPath, `${rawPublic.toString("hex")}\n`, { flag: "wx" });
console.log(`Pinned public key: ${path.relative(root, publicPath)}`);
console.log(`Private key (ignored by Git): ${path.relative(root, keyPath)}`);
console.log("Back up the private key securely. Without it, existing app versions cannot accept new in-app updates.");
