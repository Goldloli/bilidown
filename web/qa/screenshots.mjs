#!/usr/bin/env node
/**
 * bilidown 视觉验收截图流水线（design D11 / tasks 5.7、6.7）
 *
 * 用法：pnpm qa:screenshots
 * · 使用系统 Chrome（channel: 'chrome'），不下载 Playwright 浏览器
 * · 1440×900 @2x，输出到 .agents/qa/current/
 * · 自动启动/复用 Vite dev server（127.0.0.1:5183）
 * · 全部数据来自 mock（?mock=1），无需 Tauri / 网络
 */
import { spawn } from 'node:child_process';
import path from 'node:path';
import fs from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
import { chromium } from 'playwright';

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const ROOT = path.resolve(__dirname, '../..');
const OUT_DIR = path.join(ROOT, '.agents/qa/current');
const PORT = Number(process.env.QA_PORT ?? 1420);
const BASE = `http://127.0.0.1:${PORT}`;
/** QA_OS=windows 时给所有 mock 页面追加 &os=windows，用于 Windows chrome/文案截图 */
const OS_PARAM = process.env.QA_OS === 'windows' ? '&os=windows' : '';
const VITE_BIN = path.join(ROOT, 'node_modules/vite/bin/vite.js');

const PREFS_CATEGORIES = ['general', 'interface', 'download', 'quality', 'network', 'account', 'about'];

/** 截图场景矩阵（每个场景截 light / dark 两张） */
const SCENARIOS = [
  { name: 'main-empty', path: '/?mock=1&qa=1&scenario=empty' },
  { name: 'main-queue', path: '/?mock=1&qa=1&scenario=queue' },
  { name: 'main-states', path: '/?mock=1&qa=1&scenario=states', height: 1200 },
  { name: 'main-bulk', path: '/?mock=1&qa=1&scenario=bulk', audit: 'virtual-list' },
  { name: 'main-add-link', path: '/?mock=1&qa=1&scenario=add-link', panel: true },
  { name: 'main-add-panel', path: '/?mock=1&qa=1&scenario=add-panel', panel: true },
  { name: 'main-task-detail', path: '/?mock=1&qa=1&scenario=task-detail', panel: true },
  { name: 'main-task-detail-failed', path: '/?mock=1&qa=1&scenario=task-detail-failed', panel: true },
  { name: 'main-task-detail-done', path: '/?mock=1&qa=1&scenario=task-detail-done', panel: true },
  { name: 'main-login', path: '/?mock=1&qa=1&scenario=login', panel: true },
  { name: 'main-login-scanned', path: '/?mock=1&qa=1&scenario=login-scanned', panel: true },
  { name: 'main-login-expired', path: '/?mock=1&qa=1&scenario=login-expired', panel: true },
  { name: 'main-login-failed', path: '/?mock=1&qa=1&scenario=login-failed', panel: true },
  { name: 'main-login-success', path: '/?mock=1&qa=1&scenario=login-success', panel: true },
  { name: 'main-history', path: '/?mock=1&qa=1&scenario=history' },
  { name: 'main-history-empty', path: '/?mock=1&qa=1&scenario=history-empty' },
  { name: 'main-errors', path: '/?mock=1&qa=1&scenario=errors' },
  {
    name: 'main-season-confirm',
    path: '/?mock=1&qa=1&scenario=season-confirm',
    awaitSelector: '[role="alertdialog"]',
  },
  { name: 'main-empty-narrow', path: '/?mock=1&qa=1&scenario=empty', width: 760, height: 480 },
  { name: 'main-queue-narrow', path: '/?mock=1&qa=1&scenario=queue', width: 760, height: 480 },
  { name: 'gallery', path: '/?mock=1&qa=1#/gallery' },
  ...PREFS_CATEGORIES.map((category) => ({
    name: `prefs-${category}`,
    path: `/?mock=1&qa=1&category=${category}#/preferences`,
    panel: false,
  })),
  {
    name: 'prefs-interface-accent-blue',
    path: '/?mock=1&qa=1&accent=blue&category=interface#/preferences',
  },
  {
    name: 'prefs-interface-accent-purple',
    path: '/?mock=1&qa=1&accent=purple&category=interface#/preferences',
  },
  {
    name: 'prefs-interface-accent-yellow',
    path: '/?mock=1&qa=1&accent=yellow&category=interface#/preferences',
  },
  {
    name: 'main-accent-blue',
    path: '/?mock=1&qa=1&scenario=queue&accent=blue',
  },
  {
    name: 'main-accent-purple',
    path: '/?mock=1&qa=1&scenario=queue&accent=purple',
  },
  {
    name: 'main-accent-yellow',
    path: '/?mock=1&qa=1&scenario=queue&accent=yellow',
  },
  {
    name: 'prefs-general-storage',
    path: '/?mock=1&qa=1&category=general#/preferences',
    scrollBottomSelector: '[data-prefs-scroll]',
  },
  {
    name: 'prefs-general-storage-low',
    path: '/?mock=1&qa=1&storage=low&category=general#/preferences',
    scrollBottomSelector: '[data-prefs-scroll]',
  },
  {
    name: 'prefs-general-storage-error',
    path: '/?mock=1&qa=1&storage=error&category=general#/preferences',
    scrollBottomSelector: '[data-prefs-scroll]',
  },
  {
    name: 'prefs-about-update-available',
    path: '/?mock=1&qa=1&updates=available&category=about#/preferences',
  },
  {
    name: 'prefs-about-update-downloading',
    path: '/?mock=1&qa=1&updates=downloading&category=about#/preferences',
  },
  {
    name: 'prefs-about-update-downloaded',
    path: '/?mock=1&qa=1&updates=downloaded&category=about#/preferences',
  },
  {
    name: 'prefs-about-update-error',
    path: '/?mock=1&qa=1&updates=error&category=about#/preferences',
  },
];

const THEMES = ['light', 'dark'];

function sleep(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

async function serverReady() {
  try {
    const response = await fetch(`${BASE}/`, { signal: AbortSignal.timeout(1200) });
    return response.ok;
  } catch {
    return false;
  }
}

async function ensureServer() {
  if (await serverReady()) return { proc: null, reused: true };
  const proc = spawn(
    process.execPath,
    [VITE_BIN, '--config', 'web/vite.config.ts', '--port', String(PORT), '--strictPort'],
    { cwd: ROOT, stdio: 'ignore' },
  );
  const deadline = Date.now() + 60_000;
  while (Date.now() < deadline) {
    if (await serverReady()) return { proc, reused: false };
    await sleep(400);
  }
  proc.kill('SIGTERM');
  throw new Error('Vite dev server 启动超时');
}

async function main() {
  await fs.mkdir(OUT_DIR, { recursive: true });
  for (const file of await fs.readdir(OUT_DIR).catch(() => [])) {
    if (file.endsWith('.png') || file === 'manifest.json') {
      await fs.rm(path.join(OUT_DIR, file), { force: true });
    }
  }

  const { proc, reused } = await ensureServer();
  console.log(reused ? '复用已运行的 Vite dev server' : '已启动 Vite dev server');

  const browser = await chromium.launch({ channel: 'chrome', headless: true });
  const context = await browser.newContext({
    viewport: { width: 1440, height: 900 },
    deviceScaleFactor: 2,
    reducedMotion: 'reduce',
    colorScheme: 'light',
  });

  /** @type {Array<Record<string, unknown>>} */
  const manifest = [];
  let errorCount = 0;
  const startedAt = Date.now();

  for (const scenario of SCENARIOS) {
    for (const theme of THEMES) {
      const page = await context.newPage();
      const pageErrors = [];
      page.on('pageerror', (error) => pageErrors.push(`pageerror: ${error.message}`));
      page.on('console', (message) => {
        if (message.type() === 'error') pageErrors.push(`console: ${message.text()}`);
      });
      const width = scenario.width ?? 1440;
      const height = scenario.height ?? 900;
      await page.setViewportSize({ width, height });
      const [pathPart, hashPart] = scenario.path.split('#');
      const search = `${pathPart}${pathPart.includes('?') ? '&' : '?'}theme=${theme}${OS_PARAM}`;
      const url = `${BASE}${search}${hashPart ? `#${hashPart}` : ''}`;
      let ok = true;
      try {
        await page.goto(url, { waitUntil: 'domcontentloaded', timeout: 20_000 });
        await page.waitForFunction(
          () => document.documentElement.dataset.ready === '1',
          undefined,
          { timeout: 20_000 },
        );
        if (scenario.panel) {
          await page
            .waitForSelector('[role="dialog"]', { timeout: 5_000 })
            .catch(() => pageErrors.push('缺少覆盖层面板 [role="dialog"]'));
        }
        // 等字体/封面/布局稳定（动画已通过 reducedMotion + qa 样式关闭）
        await sleep(450);
        if (scenario.awaitSelector) {
          await page
            .waitForSelector(scenario.awaitSelector, { timeout: 5_000 })
            .catch(() => pageErrors.push(`缺少 ${scenario.awaitSelector}`));
        }
        if (scenario.scrollBottomSelector) {
          await page.evaluate((selector) => {
            const el = document.querySelector(selector);
            if (el) el.scrollTop = el.scrollHeight;
          }, scenario.scrollBottomSelector);
          await sleep(150);
        }

        const audit = await page.evaluate(() => {
          const doc = document.documentElement;
          const rows = document.querySelectorAll('[data-task-id]');
          const brokenImages = [...document.images]
            .filter((image) => image.complete && image.naturalWidth === 0)
            .map((image) => image.getAttribute('src') || '(missing src)');
          const remoteAvatarImages = [...document.querySelectorAll('[data-avatar-state] img')]
            .filter((image) => /^https?:/i.test(image.getAttribute('src') || '')).length;
          return {
            rows: rows.length,
            hOverflow: doc.scrollWidth - window.innerWidth,
            vOverflow: doc.scrollHeight - window.innerHeight,
            theme: doc.dataset.theme,
            dialog: Boolean(document.querySelector('[role="dialog"]')),
            brokenImages,
            remoteAvatarImages,
          };
        });
        if (audit.theme !== theme) {
          pageErrors.push(`主题未生效：期望 ${theme}，实际 ${audit.theme}`);
        }
        if (audit.hOverflow > 1) {
          pageErrors.push(`水平溢出 ${audit.hOverflow}px`);
        }
        if (audit.brokenImages.length > 0) {
          pageErrors.push(`存在破图：${audit.brokenImages.join(', ')}`);
        }
        if (audit.remoteAvatarImages > 0) {
          pageErrors.push(`头像仍在直连远程 URL：${audit.remoteAvatarImages}`);
        }

        const file = `${scenario.name}-${theme}.png`;
        await page.screenshot({ path: path.join(OUT_DIR, file), animations: 'disabled' });
        manifest.push({
          ...scenario,
          theme,
          file,
          audit,
          errors: pageErrors,
        });
        if (pageErrors.length) {
          ok = false;
          errorCount += 1;
          console.error(`✗ ${scenario.name}-${theme}: ${pageErrors.join(' | ')}`);
        } else {
          console.log(`✓ ${scenario.name}-${theme}（行数 ${audit.rows}）`);
        }
      } catch (error) {
        ok = false;
        errorCount += 1;
        console.error(`✗ ${scenario.name}-${theme}: ${error.message}`);
        manifest.push({ ...scenario, theme, error: error.message, errors: pageErrors });
      }
      void ok;
      await page.close();
    }
  }

  const bulk = manifest.find((item) => item.name === 'main-bulk' && item.theme === 'light');
  const virtualNote = bulk?.audit
    ? `虚拟列表：bulk 场景 187 条数据，仅渲染 ${bulk.audit.rows} 个行卡`
    : '虚拟列表：未取得审计数据';

  await fs.writeFile(
    path.join(OUT_DIR, 'manifest.json'),
    JSON.stringify(
      {
        generatedAt: new Date().toISOString(),
        viewport: { width: 1440, height: 900, deviceScaleFactor: 2 },
        browser: 'chrome',
        virtualList: virtualNote,
        screenshotCount: manifest.filter((item) => item.file).length,
        errorCount,
        durationMs: Date.now() - startedAt,
        screenshots: manifest,
      },
      null,
      2,
    ),
    'utf8',
  );

  await browser.close();
  if (proc) proc.kill('SIGTERM');

  console.log(`\n共 ${manifest.filter((item) => item.file).length} 张截图 → ${path.relative(ROOT, OUT_DIR)}`);
  console.log(virtualNote);
  if (errorCount > 0) {
    console.error(`\n存在 ${errorCount} 个失败的截图场景，请检查 manifest.json`);
    process.exit(1);
  }
}

main().catch((error) => {
  console.error(error);
  process.exit(1);
});
