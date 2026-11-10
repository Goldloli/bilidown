#!/usr/bin/env node
/**
 * 统计 Web 生产构建的首屏必载 JS（tasks 1.3 / 8.3）。
 *
 * 口径：从 dist/index.html 的 module 入口开始，递归解析**静态** ESM import
 * （`from "./x.js"` 与 `import "./x.js"`），动态 `import("./x.js")` 不计入首屏。
 * 输出原始字节与 gzip 字节，供改动前后同条件比较。
 *
 * 用法：node scripts/measure-first-load.mjs [distDir] [--json out.json]
 */
import fs from 'node:fs/promises';
import path from 'node:path';
import zlib from 'node:zlib';

const args = process.argv.slice(2);
const jsonFlag = args.indexOf('--json');
const outPath = jsonFlag >= 0 ? args[jsonFlag + 1] : null;
const distDir = args.find((arg, index) => !arg.startsWith('--') && index !== jsonFlag + 1) ?? 'web/dist';

const html = await fs.readFile(path.join(distDir, 'index.html'), 'utf8');
const entryMatches = [...html.matchAll(/<script[^>]*type="module"[^>]*src="([^"]+)"/g)];
if (entryMatches.length === 0) {
  console.error('未在 index.html 中找到 module 入口');
  process.exit(1);
}

const seen = new Set();
const files = [];

function relativeImports(source) {
  const found = new Set();
  // 动态 import 不计入首屏
  const dynamic = new Set();
  for (const match of source.matchAll(/import\s*\(\s*["'](\.\/[^"']+)["']\s*\)/g)) {
    dynamic.add(match[1]);
  }
  for (const match of source.matchAll(/from\s*["'](\.\/[^"']+)["']/g)) {
    found.add(match[1]);
  }
  for (const match of source.matchAll(/import\s*["'](\.\/[^"']+)["']/g)) {
    found.add(match[1]);
  }
  for (const item of dynamic) found.delete(item);
  return [...found];
}

async function walk(file) {
  const resolved = path.normalize(file);
  if (seen.has(resolved)) return;
  seen.add(resolved);
  let source;
  try {
    source = await fs.readFile(resolved, 'utf8');
  } catch {
    console.error(`无法读取 chunk: ${resolved}`);
    return;
  }
  const bytes = Buffer.byteLength(source);
  const gzipBytes = zlib.gzipSync(Buffer.from(source), { level: 9 }).length;
  files.push({ path: path.relative(distDir, resolved), bytes, gzipBytes });
  if (!resolved.endsWith('.js')) return;
  for (const specifier of relativeImports(source)) {
    await walk(path.join(path.dirname(resolved), specifier));
  }
}

for (const match of entryMatches) {
  const src = match[1].replace(/^\.\//, '');
  await walk(path.join(distDir, src));
}

const totalBytes = files.reduce((sum, file) => sum + file.bytes, 0);
const totalGzipBytes = files.reduce((sum, file) => sum + file.gzipBytes, 0);
const report = {
  dist: distDir,
  entry: entryMatches.map((match) => match[1]),
  files: files.sort((a, b) => b.bytes - a.bytes),
  totalBytes,
  totalGzipBytes,
  totalKiB: Number((totalBytes / 1024).toFixed(2)),
  totalGzipKiB: Number((totalGzipBytes / 1024).toFixed(2)),
};
if (outPath) {
  await fs.mkdir(path.dirname(outPath), { recursive: true });
  await fs.writeFile(outPath, `${JSON.stringify(report, null, 2)}\n`);
}
console.log(
  `首屏必载 JS：${report.files.length} 个文件，${report.totalKiB} KiB（gzip ${report.totalGzipKiB} KiB）`,
);
for (const file of report.files) {
  console.log(`  ${String(file.bytes).padStart(8)} B  ${file.path}`);
}
