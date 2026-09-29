// 校验三处版本号一致：package.json / tauri.conf.json / Cargo.toml（CI 门禁用）
import { readFileSync } from "node:fs";

const pkg = JSON.parse(readFileSync("package.json", "utf8"));
const conf = JSON.parse(readFileSync("src-tauri/tauri.conf.json", "utf8"));
const cargo = readFileSync("src-tauri/Cargo.toml", "utf8");

const version = pkg.version;
const errors = [];

if (conf.version !== version) {
  errors.push(`src-tauri/tauri.conf.json: ${conf.version}`);
}
const cargoMatch = cargo.match(/^version\s*=\s*"([^"]+)"/m);
if (cargoMatch && cargoMatch[1] !== version) {
  errors.push(`src-tauri/Cargo.toml: ${cargoMatch[1]}`);
}

if (errors.length > 0) {
  console.error(`版本号不一致（package.json = ${version}）：`);
  for (const e of errors) console.error(`  - ${e}`);
  process.exit(1);
}
console.log(`版本号一致：${version}`);
