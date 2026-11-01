#!/usr/bin/env node
/**
 * 准备固定版本的 LGPL ffmpeg sidecar。
 *
 * - Windows 下载并校验 BtbN LGPL 归档；
 * - macOS arm64 从固定校验和的 FFmpeg + LAME 源码静态构建；
 * - 版本、SHA256 与构建参数固定在 `scripts/ffmpeg.lock.json`；
 * - 输出放到 `src-tauri/binaries/<os>/ffmpeg-<triple>[.exe]`，
 *   由 Tauri `bundle.externalBin` 打进安装包；
 * - 二进制不入库（见 .gitignore），构建前运行：`node scripts/fetch-ffmpeg.mjs`。
 *
 * 用法：
 *   node scripts/fetch-ffmpeg.mjs                 # 当前平台
 *   node scripts/fetch-ffmpeg.mjs --force         # 重新下载/构建
 *   node scripts/fetch-ffmpeg.mjs --validate-lock # 只校验锁文件
 */
import { createHash } from 'node:crypto';
import { createWriteStream } from 'node:fs';
import { cp, mkdir, readFile, rename, rm, stat, writeFile } from 'node:fs/promises';
import { execFileSync, spawnSync } from 'node:child_process';
import path from 'node:path';
import { pipeline } from 'node:stream/promises';
import { Readable } from 'node:stream';
import { fileURLToPath } from 'node:url';

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const LOCK_PATH = process.env.BILIDOWN_FFMPEG_LOCK
  ? path.resolve(process.env.BILIDOWN_FFMPEG_LOCK)
  : path.join(ROOT, 'scripts', 'ffmpeg.lock.json');
const CACHE_DIR = path.join(ROOT, '.cache', 'ffmpeg');
const RESOURCES = path.join(ROOT, 'src-tauri', 'resources');

const TARGETS = {
  'windows-x64': { os: 'windows', triple: 'x86_64-pc-windows-msvc', binary: 'ffmpeg.exe', unzip: 'powershell' },
  'macos-arm64': { os: 'macos', triple: 'aarch64-apple-darwin', binary: 'ffmpeg' },
};

function log(...args) {
  console.log('[fetch-ffmpeg]', ...args);
}

function hostTarget() {
  if (process.platform === 'win32' && process.arch === 'x64') return 'windows-x64';
  if (process.platform === 'darwin' && process.arch === 'arm64') return 'macos-arm64';
  return null;
}

function sha256Text(value) {
  return createHash('sha256').update(value).digest('hex');
}

function requireString(value, label) {
  if (typeof value !== 'string' || value.trim() === '') {
    throw new Error(`ffmpeg.lock.json 缺少 ${label}`);
  }
  return value;
}

function requireSha256(value, label) {
  const text = requireString(value, label);
  if (!/^[0-9a-f]{64}$/.test(text)) throw new Error(`${label} 不是合法 SHA256`);
  return text;
}

function requirePositiveInteger(value, label) {
  if (!Number.isSafeInteger(value) || value <= 0) {
    throw new Error(`${label} 必须是正整数`);
  }
  return value;
}

export function validateLock(lock) {
  if (lock?.schemaVersion !== 1) throw new Error('ffmpeg.lock.json schemaVersion 必须为 1');
  for (const [targetKey, target] of Object.entries(TARGETS)) {
    const entry = lock[targetKey];
    if (!entry) throw new Error(`ffmpeg.lock.json 缺少 ${targetKey}`);
    if (entry.mode === 'download') {
      requireString(entry.url, `${targetKey}.url`);
      requireSha256(entry.sha256, `${targetKey}.sha256`);
      requirePositiveInteger(entry.archiveSize, `${targetKey}.archiveSize`);
      requireString(entry.member, `${targetKey}.member`);
      continue;
    }
    if (entry.mode === 'build') {
      if (entry.triple !== target.triple) {
        throw new Error(`${targetKey}.triple 必须是 ${target.triple}`);
      }
      requireString(entry.binary, `${targetKey}.binary`);
      requireString(entry.builder, `${targetKey}.builder`);
      requirePositiveInteger(entry.builderVersion, `${targetKey}.builderVersion`);
      requireString(entry.deploymentTarget, `${targetKey}.deploymentTarget`);
      if (!Array.isArray(entry.configure) || entry.configure.length === 0) {
        throw new Error(`${targetKey}.configure 必须是非空数组`);
      }
      for (const [dependencyName, dependency] of [
        ['ffmpeg', entry.ffmpeg],
        ['lame', entry.lame],
      ]) {
        requireString(dependency?.version, `${targetKey}.${dependencyName}.version`);
        requireString(dependency?.url, `${targetKey}.${dependencyName}.url`);
        requireSha256(dependency?.sha256, `${targetKey}.${dependencyName}.sha256`);
        requirePositiveInteger(
          dependency?.archiveSize,
          `${targetKey}.${dependencyName}.archiveSize`,
        );
        requireString(dependency?.archive, `${targetKey}.${dependencyName}.archive`);
        requireString(dependency?.directory, `${targetKey}.${dependencyName}.directory`);
      }
      const flags = new Set(entry.configure);
      for (const required of [
        '--disable-gpl',
        '--disable-nonfree',
        '--enable-libmp3lame',
      ]) {
        if (!flags.has(required)) throw new Error(`${targetKey}.configure 缺少 ${required}`);
      }
      for (const forbidden of [
        '--enable-gpl',
        '--enable-nonfree',
        '--enable-version3',
      ]) {
        if (flags.has(forbidden)) throw new Error(`${targetKey}.configure 含禁用项 ${forbidden}`);
      }
      continue;
    }
    throw new Error(`${targetKey}.mode 必须是 build 或 download`);
  }
}

async function digest(file) {
  const { createReadStream } = await import('node:fs');
  const hash = createHash('sha256');
  for await (const chunk of createReadStream(file)) hash.update(chunk);
  return hash.digest('hex');
}

async function download(url, dest) {
  log('download', url);
  const res = await fetch(url, { redirect: 'follow', headers: { 'User-Agent': 'bilidown-build' } });
  if (!res.ok) throw new Error(`下载失败 HTTP ${res.status}`);
  if (!res.body) throw new Error('下载响应为空');
  await mkdir(path.dirname(dest), { recursive: true });
  await pipeline(Readable.fromWeb(res.body), createWriteStream(dest));
  const info = await stat(dest);
  log(`downloaded ${(info.size / 1024 / 1024).toFixed(1)} MB`);
}

async function downloadVerified(url, dest, expectedSha256, expectedSize) {
  const partial = `${dest}.partial-${process.pid}`;
  await mkdir(path.dirname(dest), { recursive: true });
  await rm(partial, { force: true });
  try {
    await download(url, partial);
    const info = await stat(partial);
    if (expectedSize && info.size !== expectedSize) {
      throw new Error(`文件大小不匹配：期望 ${expectedSize}，实际 ${info.size}`);
    }
    if (expectedSha256) {
      const actual = await digest(partial);
      if (actual !== expectedSha256) {
        throw new Error(`SHA256 不匹配：期望 ${expectedSha256}，实际 ${actual}`);
      }
    }
    await rm(dest, { force: true });
    await rename(partial, dest);
  } finally {
    await rm(partial, { force: true });
  }
}

function expectedBuildStamp(targetKey, entry, builderSha256) {
  const config = {
    schemaVersion: 1,
    target: targetKey,
    triple: entry.triple,
    builder: entry.builder,
    builderVersion: entry.builderVersion,
    builderSha256,
    deploymentTarget: entry.deploymentTarget,
    ffmpeg: {
      version: entry.ffmpeg.version,
      url: entry.ffmpeg.url,
      sha256: entry.ffmpeg.sha256,
      archiveSize: entry.ffmpeg.archiveSize,
      archive: entry.ffmpeg.archive,
      directory: entry.ffmpeg.directory,
    },
    lame: {
      version: entry.lame.version,
      url: entry.lame.url,
      sha256: entry.lame.sha256,
      archiveSize: entry.lame.archiveSize,
      archive: entry.lame.archive,
      directory: entry.lame.directory,
    },
    configure: entry.configure,
  };
  return {
    ...config,
    buildConfigSha256: sha256Text(JSON.stringify(config)),
  };
}

async function ensureMacBuild(targetKey, entry, force) {
  const dest = path.join(
    ROOT,
    'src-tauri',
    'binaries',
    TARGETS[targetKey].os,
    `${entry.binary}-${entry.triple}`,
  );
  const stampPath = `${dest}.build.json`;
  const builderPath = path.join(ROOT, 'scripts', entry.builder);
  const builderSha256 = await digest(builderPath);
  const expected = expectedBuildStamp(targetKey, entry, builderSha256);
  const expectedText = `${JSON.stringify(expected, null, 2)}\n`;

  if (!force) {
    try {
      const [binaryInfo, actualStamp] = await Promise.all([
        stat(dest),
        readFile(stampPath, 'utf8'),
      ]);
      if (binaryInfo.size > 1024 * 1024 && actualStamp === expectedText) {
        log(`已存在且构建配置匹配，跳过：${path.relative(ROOT, dest)}`);
        log(`build config sha256 = ${expected.buildConfigSha256}`);
        return;
      }
    } catch {
      /* 需要构建 */
    }
  }

  const templateDir = path.join(CACHE_DIR, targetKey);
  const stampTemplate = path.join(templateDir, 'build-stamp.expected.json');
  await mkdir(templateDir, { recursive: true });
  await writeFile(stampTemplate, expectedText, 'utf8');
  log(`build config sha256 = ${expected.buildConfigSha256}`);
  log(`builder sha256 = ${builderSha256}`);

  const result = spawnSync(
    'bash',
    [
      builderPath,
      '--lock',
      LOCK_PATH,
      '--target',
      targetKey,
      '--stamp-template',
      stampTemplate,
      '--output',
      dest,
    ],
    {
      cwd: ROOT,
      env: {
        ...process.env,
        ...(force ? { BILIDOWN_FFMPEG_FORCE: '1' } : {}),
      },
      stdio: 'inherit',
    },
  );
  if (result.error) throw result.error;
  if (result.status !== 0) throw new Error(`macOS ffmpeg 构建失败，退出码 ${result.status}`);

  const [binaryInfo, actualStamp] = await Promise.all([
    stat(dest),
    readFile(stampPath, 'utf8'),
  ]);
  if (binaryInfo.size <= 1024 * 1024 || (binaryInfo.mode & 0o111) === 0) {
    throw new Error('构建产物体积或执行权限异常');
  }
  if (actualStamp !== expectedText) {
    throw new Error('构建后 stamp 与锁定配置不一致');
  }
}

function unzip(archive, destDir, kind) {
  if (kind === 'powershell') {
    execFileSync('powershell', [
      '-NoProfile',
      '-NonInteractive',
      '-Command',
      `Expand-Archive -LiteralPath '${archive.replace(/'/g, "''")}' -DestinationPath '${destDir.replace(/'/g, "''")}' -Force`,
    ]);
  } else {
    execFileSync('unzip', ['-q', '-o', archive, '-d', destDir]);
  }
}

async function ensureLicense() {
  await mkdir(RESOURCES, { recursive: true });
  const target = path.join(RESOURCES, 'COPYING.LGPLv3.ffmpeg');
  try {
    await stat(target);
    return;
  } catch {
    /* 需要下载 */
  }
  const res = await fetch('https://www.gnu.org/licenses/lgpl-3.0.txt');
  if (!res.ok) throw new Error(`下载 LGPL 文本失败 HTTP ${res.status}`);
  const text = await res.text();
  await writeFile(target, text, 'utf8');
  log('license ->', path.relative(ROOT, target));
}

async function findMember(root, suffix) {
  const { readdir } = await import('node:fs/promises');
  const wanted = suffix.replace(/^\/+/, '').replace(/\//g, path.sep);
  const walk = async (dir) => {
    const entries = await readdir(dir, { withFileTypes: true });
    for (const entry of entries) {
      const full = path.join(dir, entry.name);
      if (entry.isDirectory()) {
        const found = await walk(full);
        if (found) return found;
      } else if (full.endsWith(path.sep + wanted) || full.endsWith(path.sep + path.basename(wanted))) {
        return full;
      }
    }
    return null;
  };
  const found = await walk(root);
  if (!found) throw new Error(`压缩包中未找到 ${suffix}`);
  return found;
}

async function main() {
  const args = process.argv.slice(2);
  const force = args.includes('--force');
  const validateOnly = args.includes('--validate-lock');
  const lock = JSON.parse(await readFile(LOCK_PATH, 'utf8'));
  validateLock(lock);
  if (validateOnly) {
    log('ffmpeg.lock.json 校验通过');
    return;
  }

  const targetKey = args.find((a) => !a.startsWith('-')) ?? hostTarget();
  if (!targetKey) {
    log('当前平台未配置 ffmpeg sidecar（仅支持 windows-x64 / macos-arm64）：跳过');
    process.exit(0);
  }
  const target = TARGETS[targetKey];
  if (!target) throw new Error(`未知目标: ${targetKey}`);

  const entry = lock[targetKey];
  if (!entry) throw new Error(`ffmpeg.lock.json 缺少 ${targetKey} 配置`);
  if (entry.mode === 'build') {
    await ensureMacBuild(targetKey, entry, force);
    log('完成');
    return;
  }

  const ext = target.os === 'windows' ? '.exe' : '';
  const dest = path.join(
    ROOT,
    'src-tauri',
    'binaries',
    target.os,
    `ffmpeg-${target.triple}${ext}`,
  );
  const archive = path.join(CACHE_DIR, `${targetKey}.zip`);
  const unpackDir = path.join(CACHE_DIR, `${targetKey}-unpacked`);

  if (!force) {
    try {
      const info = await stat(dest);
      if (info.size > 0) {
        log('已存在，跳过（--force 可重新下载）:', path.relative(ROOT, dest));
        await ensureLicense();
        return;
      }
    } catch {
      /* 继续下载 */
    }
  }

  let archiveReady = false;
  if (!force) {
    try {
      const info = await stat(archive);
      if (entry.sha256 && info.size === entry.archiveSize) {
        const actual = await digest(archive);
        if (actual === entry.sha256) {
          archiveReady = true;
          log('复用已缓存压缩包（SHA256 匹配）');
        }
      }
    } catch {
      /* 重新下载 */
    }
  }

  if (!archiveReady) {
    await downloadVerified(entry.url, archive, entry.sha256, entry.archiveSize);
    log('archive sha256 =', entry.sha256);
  }

  await rm(unpackDir, { recursive: true, force: true });
  unzip(archive, unpackDir, target.unzip);
  // BtbN 压缩包内含顶层版本目录（如 ffmpeg-master-latest-win64-lgpl/bin/ffmpeg.exe），
  // 因此按后缀匹配而不是固定顶层路径。
  const member = await findMember(unpackDir, entry.member ?? `bin/${target.binary}`);
  const info = await stat(member);
  if (info.size < 1024 * 1024) throw new Error('解包出的 ffmpeg 体积异常');
  await mkdir(path.dirname(dest), { recursive: true });
  const partialDest = `${dest}.partial-${process.pid}`;
  await rm(partialDest, { force: true });
  try {
    await cp(member, partialDest);
    if (target.os !== 'windows') await execFileSync('chmod', ['+x', partialDest]);
    await rm(dest, { force: true });
    await rename(partialDest, dest);
  } finally {
    await rm(partialDest, { force: true });
  }
  log('ffmpeg ->', path.relative(ROOT, dest), `${(info.size / 1024 / 1024).toFixed(1)} MB`);

  await ensureLicense();
  log('完成');
}

const entryPath = fileURLToPath(import.meta.url);
if (process.argv[1] && path.resolve(process.argv[1]) === entryPath) {
  main().catch((err) => {
    console.error('[fetch-ffmpeg] 失败:', err.message);
    process.exit(1);
  });
}
