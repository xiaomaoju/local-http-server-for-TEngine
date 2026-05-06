#!/usr/bin/env node
// 把 package.json 的 version 同步到所有需要的地方:
//   server/Cargo.toml, src-tauri/Cargo.toml, src-tauri/tauri.conf.json
// 用法: node scripts/sync-version.mjs

import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const pkg = JSON.parse(fs.readFileSync(path.join(ROOT, "package.json"), "utf8"));
const version = pkg.version;

console.log(`Sync version: ${version}`);

// Cargo.toml: 仅替换顶层 [package] 段第一个 version = "..."
function bumpCargoToml(file) {
  const full = path.join(ROOT, file);
  let text = fs.readFileSync(full, "utf8");
  const before = text;
  text = text.replace(
    /(\[package\][\s\S]*?\nversion\s*=\s*")[^"]+(")/,
    `$1${version}$2`,
  );
  if (text === before) {
    throw new Error(`Failed to update version in ${file}`);
  }
  fs.writeFileSync(full, text);
  console.log(`  ✓ ${file}`);
}

// tauri.conf.json: 顶层 "version" 字段
function bumpTauriConf(file) {
  const full = path.join(ROOT, file);
  const json = JSON.parse(fs.readFileSync(full, "utf8"));
  json.version = version;
  fs.writeFileSync(full, JSON.stringify(json, null, 2) + "\n");
  console.log(`  ✓ ${file}`);
}

bumpCargoToml("server/Cargo.toml");
bumpCargoToml("src-tauri/Cargo.toml");
bumpTauriConf("src-tauri/tauri.conf.json");

console.log("Done. Cargo.lock files will be regenerated on next build.");
