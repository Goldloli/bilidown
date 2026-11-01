#!/usr/bin/env node
/**
 * 从唯一 SVG 母版生成前端、Windows 与 macOS 图标，不修改移动端资源。
 * node scripts/generate-icons.mjs
 * 使用已有 Playwright 和系统 Chrome；可用 ICON_BROWSER_CHANNEL=msedge 切换。
 */
import { execFileSync } from 'node:child_process';
import fs from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { chromium } from 'playwright';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const source = await fs.readFile(path.join(root, 'assets/app-icon.svg'), 'utf8');
const icoSizes = [16, 24, 32, 48, 64, 128, 256];
const macIconsetFiles = [
  ['icon_16x16.png', 16],
  ['icon_16x16@2x.png', 32],
  ['icon_32x32.png', 32],
  ['icon_32x32@2x.png', 64],
  ['icon_128x128.png', 128],
  ['icon_128x128@2x.png', 256],
  ['icon_256x256.png', 256],
  ['icon_256x256@2x.png', 512],
  ['icon_512x512.png', 512],
  ['icon_512x512@2x.png', 1024],
];
const pngOutputs = new Map([
  [32, ['web/public/favicon-32.png', 'src-tauri/icons/32x32.png']],
  [64, ['web/public/app-icon-64.png', 'src-tauri/icons/64x64.png']],
  [128, ['src-tauri/icons/128x128.png']],
  [256, ['web/public/app-icon-256.png', 'src-tauri/icons/128x128@2x.png']],
  [512, ['src-tauri/icons/icon.png']],
  [1024, ['web/public/app-icon-1024.png']],
]);

// PNG 格式的 ICO 帧直接由相同 SVG 栅格化，不再缩放旧粉色 PNG。
function encodeIco(frames) {
  const header = Buffer.alloc(6 + frames.length * 16);
  header.writeUInt16LE(1, 2);
  header.writeUInt16LE(frames.length, 4);
  let offset = header.length;
  frames.forEach(({ size, png }, index) => {
    const entry = 6 + index * 16;
    header[entry] = size === 256 ? 0 : size;
    header[entry + 1] = size === 256 ? 0 : size;
    header.writeUInt16LE(1, entry + 4);
    header.writeUInt16LE(32, entry + 6);
    header.writeUInt32LE(png.length, entry + 8);
    header.writeUInt32LE(offset, entry + 12);
    offset += png.length;
  });
  return Buffer.concat([header, ...frames.map(({ png }) => png)]);
}

const browser = await chromium.launch({ channel: process.env.ICON_BROWSER_CHANNEL ?? 'chrome' });
const rendered = new Map();
try {
  const page = await browser.newPage({ deviceScaleFactor: 1 });
  await page.setContent(`<!doctype html><html><head><style>
    html, body { margin: 0; background: transparent; }
    svg { display: block; width: 100vw; height: 100vh; }
  </style></head><body>${source}</body></html>`);
  for (const size of new Set([...icoSizes, ...pngOutputs.keys()])) {
    await page.setViewportSize({ width: size, height: size });
    rendered.set(
      size,
      await page.locator('svg').screenshot({ omitBackground: true, type: 'png' }),
    );
  }
} finally {
  await browser.close();
}

// 完成所有渲染之后再写入，浏览器不可用时不会留下半套资源。
for (const [size, outputs] of pngOutputs) {
  for (const output of outputs) await fs.writeFile(path.join(root, output), rendered.get(size));
}
for (const output of ['web/public/app-icon.svg', 'web/public/favicon.svg']) {
  await fs.writeFile(path.join(root, output), source);
}
await fs.writeFile(
  path.join(root, 'src-tauri/icons/icon.ico'),
  encodeIco(icoSizes.map((size) => ({ size, png: rendered.get(size) }))),
);

if (process.platform === 'darwin') {
  const iconset = path.join(root, '.cache/iconset/bilidown.iconset');
  const icns = path.join(root, '.cache/iconset/bilidown.icns');
  await fs.rm(iconset, { recursive: true, force: true });
  await fs.mkdir(iconset, { recursive: true });
  for (const [name, size] of macIconsetFiles) {
    await fs.writeFile(path.join(iconset, name), rendered.get(size));
  }
  await fs.rm(icns, { force: true });
  execFileSync('iconutil', ['-c', 'icns', iconset, '-o', icns]);
  await fs.rename(icns, path.join(root, 'src-tauri/icons/icon.icns'));
  console.log('macOS ICNS 已生成：src-tauri/icons/icon.icns');
}

console.log(`图标已从 assets/app-icon.svg 生成；ICO 尺寸：${icoSizes.join('/')}`);
