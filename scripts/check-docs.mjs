#!/usr/bin/env node
// 校验 Markdown 中的相对链接，避免文档归档或重命名后留下断链。
import { existsSync, readdirSync, readFileSync, statSync } from "node:fs";
import { dirname, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const skipped = new Set([
  "node_modules",
  ".git",
  "dist",
  "target",
  "gen",
  ".pnpm-store",
]);
const markdown = [];
function walk(directory) {
  for (const name of readdirSync(directory)) {
    if (skipped.has(name)) continue;
    const path = join(directory, name);
    if (statSync(path).isDirectory()) walk(path);
    else if (name.endsWith(".md")) markdown.push(path);
  }
}
walk(root);

const pattern = /\[[^\]]*\]\(([^)\s]+)(?:\s+"[^"]*")?\)/g;
const broken = [];
let checked = 0;
for (const file of markdown) {
  // 代码块中的示例链接不作为文档导航检查。
  const text = readFileSync(file, "utf8")
    .replace(/```[\s\S]*?```/g, "")
    .replace(/`[^`\n]*`/g, "");
  for (const match of text.matchAll(pattern)) {
    let target = match[1];
    if (/^(https?:|mailto:|#)/.test(target)) continue;
    target = target.replace(/^<|>$/g, "");
    target = target.split("#")[0].split("?")[0];
    if (!target) continue;
    const path = target.startsWith("/")
      ? join(root, target)
      : resolve(dirname(file), target);
    checked += 1;
    if (!existsSync(path)) broken.push(`${relative(root, file)} -> ${target}`);
  }
}

if (broken.length > 0) {
  console.error(`文档链接检查失败：${broken.length} 个断链`);
  for (const item of broken) console.error(`  ${item}`);
  process.exit(1);
}
console.log(
  `文档链接检查通过：${markdown.length} 个 Markdown 文件、${checked} 个相对链接`,
);
