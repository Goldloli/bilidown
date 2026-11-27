#!/usr/bin/env node
import { spawn } from 'node:child_process';
import fs from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { chromium } from 'playwright';

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
const PORT = Number(process.env.QA_INTERACTIONS_PORT ?? 1421);
const BASE = `http://127.0.0.1:${PORT}`;
const VITE_BIN = path.join(ROOT, 'node_modules/vite/bin/vite.js');
const ARTIFACT_OUT = path.join(ROOT, '.agents/qa/artifacts');

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

async function ready() {
  try {
    const response = await fetch(`${BASE}/`, { signal: AbortSignal.timeout(800) });
    return response.ok;
  } catch {
    return false;
  }
}

async function ensureServer() {
  if (await ready()) return null;
  const proc = spawn(
    process.execPath,
    [VITE_BIN, '--config', 'web/vite.config.ts', '--port', String(PORT), '--strictPort'],
    { cwd: ROOT, stdio: 'ignore' },
  );
  const deadline = Date.now() + 30_000;
  while (Date.now() < deadline) {
    if (await ready()) return proc;
    await sleep(300);
  }
  proc.kill('SIGTERM');
  throw new Error('Vite 测试服务启动超时');
}

async function openPanel(page, scenario = 'add-panel') {
  await page.goto(`${BASE}/?mock=1&qa=1&scenario=${scenario}`, {
    waitUntil: 'domcontentloaded',
  });
  await page.waitForFunction(() => document.documentElement.dataset.ready === '1');
  const dialog = page.locator('[role="dialog"]');
  await dialog.waitFor();
  return dialog;
}

async function selectTab(dialog, label) {
  await dialog.getByRole('button', { name: new RegExp(label) }).first().click();
}

async function toggleAll(dialog) {
  await dialog.getByRole('checkbox', { name: '全选' }).click();
}

async function captureArtifactViews(browser) {
  await fs.mkdir(ARTIFACT_OUT, { recursive: true });
  const manifest = [];
  for (const theme of ['light', 'dark']) {
    const context = await browser.newContext({
      viewport: { width: 1100, height: 800 },
      deviceScaleFactor: 1,
      colorScheme: theme,
      reducedMotion: 'reduce',
    });
    const page = await context.newPage();
    const shots = [
      { name: 'task-detail-artifacts', scenario: 'task-detail-artifacts', expand: false },
      { name: 'history-artifacts', scenario: 'history-artifacts', expand: true },
      { name: 'history-legacy', scenario: 'history-legacy', expand: true },
    ];
    for (const shot of shots) {
      await page.goto(
        `${BASE}/?mock=1&qa=1&scenario=${shot.scenario}&theme=${theme}`,
        { waitUntil: 'domcontentloaded' },
      );
      await page.waitForFunction(() => document.documentElement.dataset.ready === '1');
      if (shot.expand) {
        const toggle = page.getByRole('button', { name: /个文件 · 展开/ }).first();
        await toggle.waitFor();
        await toggle.click();
      }
      const output = path.join(ARTIFACT_OUT, `${shot.name}-${theme}.png`);
      await page.screenshot({ path: output, fullPage: true });
      manifest.push({ theme, scenario: shot.scenario, output });
    }
    await context.close();
  }

  const narrow = await browser.newContext({
    viewport: { width: 760, height: 480 },
    deviceScaleFactor: 1,
    colorScheme: 'light',
    reducedMotion: 'reduce',
  });
  const narrowPage = await narrow.newPage();
  await narrowPage.goto(
    `${BASE}/?mock=1&qa=1&scenario=history-artifacts&theme=light`,
    { waitUntil: 'domcontentloaded' },
  );
  await narrowPage.waitForFunction(() => document.documentElement.dataset.ready === '1');
  await narrowPage.getByRole('button', { name: /个文件 · 展开/ }).first().click();
  const narrowOutput = path.join(ARTIFACT_OUT, 'history-artifacts-narrow.png');
  await narrowPage.screenshot({ path: narrowOutput, fullPage: true });
  manifest.push({ theme: 'light', scenario: 'history-artifacts-narrow', output: narrowOutput });
  await narrow.close();
  await fs.writeFile(
    path.join(ARTIFACT_OUT, 'manifest.json'),
    JSON.stringify(manifest, null, 2),
  );
  return manifest;
}

async function checkPopoverInteractions(page, browser) {
  const panelRequests = [];
  const onRequest = (request) => {
    if (request.url().includes('/src/components/PopoverMotionPanel.tsx')) {
      panelRequests.push(request.url());
    }
  };
  page.on('request', onRequest);
  await page.goto(`${BASE}/?mock=1&qa=1&scenario=empty&os=macos`, {
    waitUntil: 'domcontentloaded',
  });
  await page.waitForFunction(() => document.documentElement.dataset.ready === '1');
  if (panelRequests.length !== 0) throw new Error('关闭状态不应阻塞首屏加载动画面板');

  await page.getByRole('button', { name: '搜索', exact: true }).click();
  const search = page.getByRole('textbox', { name: '搜索' });
  await search.waitFor();
  if (panelRequests.length === 0) throw new Error('首次打开应按需加载动画面板');
  const searchBox = await search.boundingBox();
  const viewport = page.viewportSize();
  if (!searchBox || !viewport || searchBox.x < 0 || searchBox.y < 0 || searchBox.x + searchBox.width > viewport.width || searchBox.y + searchBox.height > viewport.height) {
    throw new Error('搜索弹层超出视口');
  }
  await page.locator('body').click({ position: { x: 4, y: 4 } });
  await search.waitFor({ state: 'detached' });
  await page.getByRole('button', { name: '搜索', exact: true }).click();
  await search.waitFor();
  await page.keyboard.press('Escape');
  await search.waitFor({ state: 'detached' });

  await page.emulateMedia({ reducedMotion: 'reduce' });
  await page.getByRole('button', { name: '帮助', exact: true }).click();
  await page.getByText('快捷键', { exact: true }).waitFor();
  await page.keyboard.press('Escape');
  await page.getByText('快捷键', { exact: true }).waitFor({ state: 'detached' });
  page.off('request', onRequest);

  const failureContext = await browser.newContext();
  const failurePage = await failureContext.newPage();
  let blockPanel = true;
  await failurePage.route('**/src/components/PopoverMotionPanel.tsx*', (route) =>
    blockPanel ? route.abort() : route.continue(),
  );
  await failurePage.goto(`${BASE}/?mock=1&qa=1&scenario=empty&os=macos`, {
    waitUntil: 'domcontentloaded',
  });
  await failurePage.waitForFunction(() => document.documentElement.dataset.ready === '1');
  await failurePage.getByRole('button', { name: '搜索', exact: true }).click();
  await failurePage.getByText('浮层加载失败', { exact: true }).waitFor();
  blockPanel = false;
  await failurePage.getByRole('button', { name: '重试', exact: true }).click();
  await failurePage.getByRole('textbox', { name: '搜索' }).waitFor();
  await failureContext.close();
}

async function main() {
  const server = await ensureServer();
  const browser = await chromium.launch({ channel: 'chrome', headless: true });
  const failures = [];
  try {
    const page = await browser.newPage({ viewport: { width: 1100, height: 800 } });

    await page.goto(`${BASE}/?mock=1&qa=1&scenario=empty&os=macos`, {
      waitUntil: 'domcontentloaded',
    });
    await page.waitForFunction(() => document.documentElement.dataset.ready === '1');
    const macControls = await page
      .locator('.win-controls')
      .evaluate((element) => getComputedStyle(element).display);
    if (macControls !== 'none') throw new Error('macOS 不应显示 Windows 窗口按钮');

    await page.goto(`${BASE}/?mock=1&qa=1&scenario=empty&os=windows`, {
      waitUntil: 'domcontentloaded',
    });
    await page.waitForFunction(() => document.documentElement.dataset.ready === '1');
    const windowsControls = await page
      .locator('.win-controls')
      .evaluate((element) => getComputedStyle(element).display);
    if (windowsControls !== 'flex') throw new Error('Windows 应显示自绘窗口按钮');

    await checkPopoverInteractions(page, browser);

    const dialog = await openPanel(page);

    await selectTab(dialog, '夜航西飞');
    await toggleAll(dialog);
    const submitOne = dialog.getByRole('button', { name: '下载 12' });
    if (!(await submitOne.isEnabled())) throw new Error('部分条目明确清空后确认按钮不应禁用');
    await submitOne.click();
    await page.getByText('已添加 12 个任务').first().waitFor({ timeout: 4_000 });

    const cleared = await openPanel(page);
    await toggleAll(cleared);
    await selectTab(cleared, '夜航西飞');
    await toggleAll(cleared);
    const submitZero = cleared.getByRole('button', { name: '下载 0' });
    if (!(await submitZero.isDisabled())) throw new Error('全部条目清空后确认按钮必须禁用');

    const preferred = await openPanel(page, 'add-panel-preferences');
    const quality = preferred.getByRole('button', { name: '720P', exact: true });
    if (!(await quality.evaluate((element) => element.className.includes('bg-accent')))) {
      throw new Error('固定 720P 未应用到确认面板');
    }
    if ((await preferred.getByRole('radio', { name: 'HEVC' }).getAttribute('aria-checked')) !== 'true') {
      throw new Error('HEVC 偏好未应用到确认面板');
    }

    const noFallback = await openPanel(page, 'add-panel-no-fallback');
    await noFallback.getByText(/设置的清晰度当前不可用/).first().waitFor();
    const blockedSubmit = noFallback.getByRole('button', { name: /^下载 \d+$/ });
    if (!(await blockedSubmit.isDisabled())) throw new Error('关闭回退时不可用固定档位必须阻止提交');

    const detail = await openPanel(page, 'task-detail-artifacts');
    await detail.getByRole('button', { name: '打开第 2 个文件' }).click();
    await page
      .getByText('打开文件：~/Downloads/bilidown/城市夜景[1080P] P2.mp4')
      .first()
      .waitFor({ timeout: 4_000 });

    await page.goto(`${BASE}/?mock=1&qa=1&scenario=history-artifacts`, {
      waitUntil: 'domcontentloaded',
    });
    await page.waitForFunction(() => document.documentElement.dataset.ready === '1');
    await page.getByRole('button', { name: '2 个文件 · 展开' }).first().click();
    await page.getByRole('button', { name: '打开第 2 个文件' }).first().click();
    await page
      .getByText('打开文件：~/Downloads/bilidown/历史 P2.mp4')
      .first()
      .waitFor({ timeout: 4_000 });

    await page.goto(`${BASE}/?mock=1&qa=1&scenario=history-legacy`, {
      waitUntil: 'domcontentloaded',
    });
    await page.waitForFunction(() => document.documentElement.dataset.ready === '1');
    await page.getByRole('button', { name: '1 个文件 · 展开' }).first().click();
    await page.getByText(/旧记录：仅保留单文件信息/).first().waitFor();

    await page.goto(`${BASE}/?mock=1&qa=1&scenario=history`, {
      waitUntil: 'domcontentloaded',
    });
    await page.waitForFunction(() => document.documentElement.dataset.ready === '1');
    await page.locator('[role="listitem"]').first().dblclick();
    await page.getByText(/打开文件：/).first().waitFor({ timeout: 4_000 });
    if ((await page.getByText(/打开文件 · 待接线/).count()) > 0) {
      throw new Error('真实模式双击仍出现待接线提示');
    }
    const screenshots = await captureArtifactViews(browser);
  } catch (error) {
    failures.push(error instanceof Error ? error.message : String(error));
  } finally {
    await browser.close();
    server?.kill('SIGTERM');
  }

  if (failures.length > 0) {
    console.error(`交互检查失败：${failures.join('; ')}`);
    process.exitCode = 1;
  } else {
    console.log(
      `交互检查通过：空选择、固定偏好、多产物、弹层交互与加载失败重试符合规格；截图 ${path.relative(ROOT, ARTIFACT_OUT)}`,
    );
  }
}

await main();
