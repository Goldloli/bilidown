#!/usr/bin/env node
/**
 * 版本一致性检查：package.json / tauri.conf.json / Cargo.toml / 前端构建常量必须一致，
 * 并可选地把新版本同步写入这四处。
 *
 *   node scripts/version.mjs            # 检查，不一致时退出码 1
 *   node scripts/version.mjs --bump 2.4.0   # 同步到 2.4.0（需人工 review 后提交）
 */
import { readFile, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const FILES = {
  packageJson: path.join(ROOT, 'package.json'),
  tauriConf: path.join(ROOT, 'src-tauri', 'tauri.conf.json'),
  cargoToml: path.join(ROOT, 'src-tauri', 'Cargo.toml'),
  constants: path.join(ROOT, 'web', 'src', 'lib', 'constants.ts'),
};

function fail(message) {
  console.error(`[version] ${message}`);
  process.exit(1);
}

function readVersion(kind, text) {
  switch (kind) {
    case 'packageJson': {
      const match = text.match(/"version"\s*:\s*"([^"]+)"/);
      return match?.[1];
    }
    case 'tauriConf': {
      const match = text.match(/"version"\s*:\s*"([^"]+)"/);
      return match?.[1];
    }
    case 'cargoToml': {
      const section = text.split('[dependencies]')[0];
      const match = section.match(/^version\s*=\s*"([^"]+)"/m);
      return match?.[1];
    }
    case 'constants': {
      const match = text.match(/APP_VERSION\s*=\s*'([^']+)'/);
      return match?.[1];
    }
    default:
      return undefined;
  }
}

function replaceVersion(kind, text, version) {
  switch (kind) {
    case 'packageJson':
    case 'tauriConf':
      return text.replace(/("version"\s*:\s*")[^"]+(")/, `$1${version}$2`);
    case 'cargoToml': {
      const [head, ...rest] = text.split('[dependencies]');
      return head.replace(/^(version\s*=\s*")[^"]+(")/m, `$1${version}$2`) + (rest.length ? `[dependencies]${rest.join('[dependencies]')}` : '');
    }
    case 'constants':
      return text.replace(/(APP_VERSION\s*=\s*')[^']+(')/, `$1${version}$2`);
    default:
      return text;
  }
}

async function main() {
  const args = process.argv.slice(2);
  const bumpIndex = args.indexOf('--bump');
  const bumpVersion = bumpIndex >= 0 ? args[bumpIndex + 1] : undefined;

  const entries = await Promise.all(
    Object.entries(FILES).map(async ([kind, file]) => [kind, file, await readFile(file, 'utf8')]),
  );

  if (bumpVersion) {
    if (!/^\d+\.\d+\.\d+([-+].+)?$/.test(bumpVersion)) fail(`版本号不合法: ${bumpVersion}`);
    for (const [kind, file, text] of entries) {
      const next = replaceVersion(kind, text, bumpVersion);
      if (next !== text) {
        await writeFile(file, next, 'utf8');
        console.log(`[version] ${path.relative(ROOT, file)} -> ${bumpVersion}`);
      }
    }
    console.log('[version] 完成：请 review 后提交（Cargo.lock 会在下次 cargo 构建时更新）');
    return;
  }

  const found = entries.map(([kind, file, text]) => [kind, path.relative(ROOT, file), readVersion(kind, text)]);
  const versions = new Set(found.map(([, , version]) => version));
  for (const [kind, file, version] of found) {
    console.log(`[version] ${file.padEnd(34)} ${version ?? '<missing>'}   (${kind})`);
  }
  if (versions.size !== 1 || [...versions][0] === undefined) {
    fail(`版本不一致：${[...versions].join(' / ')}`);
  }
  console.log(`[version] OK ${[...versions][0]}`);
}

main().catch((error) => fail(error.message));
