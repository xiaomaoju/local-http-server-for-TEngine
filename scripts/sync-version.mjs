#!/usr/bin/env node
// package.json is the release version source. --check validates without writing.
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const args = process.argv.slice(2);
if (args.some((arg) => arg !== "--check")) throw new Error("Usage: node scripts/sync-version.mjs [--check]");
const checkOnly = args.includes("--check");
const version = JSON.parse(fs.readFileSync(path.join(ROOT, "package.json"), "utf8")).version;
if (!/^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?$/.test(version)) throw new Error(`Invalid release version: ${version}`);
const changes = [];

function update(file, transform) {
  const before = fs.readFileSync(path.join(ROOT, file), "utf8");
  const after = transform(before);
  if (before !== after) changes.push({ file, after });
}

function replaceVersion(file, pattern, value = version) {
  update(file, (text) => {
    if (!pattern.test(text)) throw new Error(`Version field missing in ${file}`);
    return text.replace(pattern, (_, prefix, suffix) => `${prefix}${value}${suffix}`);
  });
}

update("package-lock.json", (text) => {
  const lock = JSON.parse(text);
  lock.version = version;
  lock.packages[""].version = version;
  return JSON.stringify(lock, null, 2) + "\n";
});
for (const [dir, name] of [["server", "tengine-server"], ["src-tauri", "tengine-http-server"]]) {
  replaceVersion(`${dir}/Cargo.toml`, /(\[package\][\s\S]*?\nversion\s*=\s*")[^"]+(")/);
  replaceVersion(`${dir}/Cargo.lock`, new RegExp(`(\\[\\[package\\]\\]\\r?\\nname = "${name}"\\r?\\nversion = ")[^"]+(")`));
}
update("src-tauri/tauri.conf.json", (text) => {
  const config = JSON.parse(text);
  config.version = version;
  return JSON.stringify(config, null, 2) + "\n";
});
replaceVersion("server/offline/docker-compose.yml", /(^    image: tengine-server:)[^\r\n]+($)/m, `${version}-amd64`);

if (checkOnly && changes.length) throw new Error(`Run npm run sync-version; out of sync: ${changes.map(({ file }) => file).join(", ")}`);
for (const { file, after } of changes) {
  fs.writeFileSync(path.join(ROOT, file), after);
  console.log(`Updated ${file}`);
}
console.log(`Version ${version}: ${checkOnly ? "checked" : "synchronized"}`);
