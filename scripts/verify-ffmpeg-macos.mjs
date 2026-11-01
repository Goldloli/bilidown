#!/usr/bin/env node
/**
 * Verify the macOS arm64 ffmpeg sidecar and, optionally, its packaged .app.
 *
 * Usage:
 *   node scripts/verify-ffmpeg-macos.mjs --binary <path> [--smoke]
 *   node scripts/verify-ffmpeg-macos.mjs --app <bilidown.app> [--smoke]
 *   node scripts/verify-ffmpeg-macos.mjs --self-test
 */
import { spawnSync } from 'node:child_process';
import { mkdtemp, readFile, rm, stat } from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const REQUIRED_PACKAGE_FILES = [
  'NOTICE.ffmpeg.txt',
  'COPYING.LGPLv2.1.ffmpeg',
  'COPYING.LAME',
  'FFMPEG-SOURCE.txt',
];

function log(...args) {
  console.log('[verify-ffmpeg-macos]', ...args);
}

function run(command, args, options = {}) {
  const result = spawnSync(command, args, {
    encoding: 'utf8',
    maxBuffer: 16 * 1024 * 1024,
    ...options,
  });
  if (result.error) throw result.error;
  if (result.status !== 0) {
    const detail = (result.stderr || result.stdout || '').trim();
    throw new Error(`${command} ${args.join(' ')} 失败${detail ? `：${detail}` : ''}`);
  }
  return result.stdout;
}

function parseArgs(argv) {
  const parsed = { smoke: false, selfTest: false };
  for (let index = 0; index < argv.length; index += 1) {
    const arg = argv[index];
    if (arg === '--smoke') {
      parsed.smoke = true;
      continue;
    }
    if (arg === '--self-test') {
      parsed.selfTest = true;
      continue;
    }
    if (arg === '--binary' || arg === '--app') {
      const value = argv[index + 1];
      if (!value) throw new Error(`${arg} 缺少路径`);
      parsed[arg.slice(2)] = path.resolve(value);
      index += 1;
      continue;
    }
    throw new Error(`未知参数: ${arg}`);
  }
  return parsed;
}

function parseCapabilityLines(output) {
  const names = new Set();
  for (const line of output.split(/\r?\n/)) {
    const tokens = line.trim().split(/\s+/);
    for (const token of tokens) {
      if (token && !['D', 'E', 'A', 'S', 'V', 'F', 'X', 'B'].includes(token)) {
        for (const name of token.split(',')) names.add(name);
      }
    }
  }
  return names;
}

function versionAtMost(minos, maximum = '13.0') {
  const left = minos.split('.').map(Number);
  const right = maximum.split('.').map(Number);
  for (let index = 0; index < Math.max(left.length, right.length); index += 1) {
    const a = left[index] ?? 0;
    const b = right[index] ?? 0;
    if (a !== b) return a < b;
  }
  return true;
}

export function validateMetadata(metadata) {
  const errors = [];
  const architectures = metadata.architectures ?? [];
  if (architectures.length !== 1 || architectures[0] !== 'arm64') {
    errors.push(`期望单一 arm64 架构，实际为 ${architectures.join(', ') || '未知'}`);
  }
  if (!metadata.minos || !versionAtMost(metadata.minos, '13.0')) {
    errors.push(`最低系统版本必须不高于 13.0，实际为 ${metadata.minos || '未知'}`);
  }

  const nonSystemDependencies = (metadata.dependencies ?? []).filter(
    (dependency) => !dependency.startsWith('/System/') && !dependency.startsWith('/usr/lib/'),
  );
  if (nonSystemDependencies.length > 0) {
    errors.push(`存在非系统动态依赖：${nonSystemDependencies.join(', ')}`);
  }

  const version = metadata.versionOutput ?? '';
  for (const required of [
    '--disable-gpl',
    '--disable-nonfree',
    '--enable-libmp3lame',
  ]) {
    if (!version.includes(required)) errors.push(`版本配置缺少 ${required}`);
  }
  for (const forbidden of [
    '--enable-gpl',
    '--enable-nonfree',
    '--enable-version3',
  ]) {
    if (version.includes(forbidden)) errors.push(`版本配置含禁用项 ${forbidden}`);
  }
  const encoders = new Set(metadata.encoders ?? []);
  const demuxers = new Set(metadata.demuxers ?? []);
  const muxers = new Set(metadata.muxers ?? []);
  if (!encoders.has('libmp3lame')) errors.push('缺少 libmp3lame 编码器');
  if (!demuxers.has('mov')) errors.push('缺少 mov/mp4 解复用器');
  if (!muxers.has('mp4')) errors.push('缺少 mp4 复用器');
  if (!muxers.has('mp3')) errors.push('缺少 mp3 复用器');
  return errors;
}

function inspectBinary(binary) {
  const resolved = path.resolve(binary);
  const lipoOutput = run('lipo', ['-archs', resolved]).trim();
  const architectures = lipoOutput.split(/\s+/).filter(Boolean);
  const buildOutput = run('vtool', ['-show-build', resolved]);
  const minos = buildOutput.match(/\bminos\s+([0-9.]+)/)?.[1] ?? '';
  const linkage = run('otool', ['-L', resolved]);
  const dependencies = linkage
    .split(/\r?\n/)
    .slice(1)
    .map((line) => line.trim().split(/\s+/)[0])
    .filter(Boolean);
  const versionOutput = run(resolved, ['-hide_banner', '-version']);
  const encoders = parseCapabilityLines(
    run(resolved, ['-hide_banner', '-encoders']),
  );
  const demuxers = parseCapabilityLines(
    run(resolved, ['-hide_banner', '-demuxers']),
  );
  const muxers = parseCapabilityLines(
    run(resolved, ['-hide_banner', '-muxers']),
  );
  return {
    binary: resolved,
    architectures,
    minos,
    dependencies,
    versionOutput,
    encoders: [...encoders],
    demuxers: [...demuxers],
    muxers: [...muxers],
  };
}

async function assertNonEmptyFile(file) {
  const info = await stat(file);
  if (!info.isFile() || info.size <= 0) throw new Error(`产物为空或不是文件：${file}`);
}

async function runSmoke(binary) {
  const tempDir = await mkdtemp(path.join(os.tmpdir(), 'bilidown-ffmpeg-smoke-'));
  const video = path.join(tempDir, 'video.mp4');
  const audio = path.join(tempDir, 'audio.m4a');
  const merged = path.join(tempDir, 'merged.mp4');
  const mp3 = path.join(tempDir, 'audio.mp3');
  try {
    run(binary, [
      '-y',
      '-hide_banner',
      '-loglevel',
      'error',
      '-f',
      'lavfi',
      '-i',
      'testsrc2=size=160x90:rate=10',
      '-t',
      '1',
      '-c:v',
      'mpeg4',
      '-an',
      video,
    ]);
    run(binary, [
      '-y',
      '-hide_banner',
      '-loglevel',
      'error',
      '-f',
      'lavfi',
      '-i',
      'sine=frequency=1000:sample_rate=44100',
      '-t',
      '1',
      '-c:a',
      'aac',
      '-vn',
      audio,
    ]);
    run(binary, [
      '-y',
      '-hide_banner',
      '-loglevel',
      'error',
      '-i',
      video,
      '-i',
      audio,
      '-c',
      'copy',
      '-movflags',
      '+faststart',
      merged,
    ]);
    run(binary, [
      '-y',
      '-hide_banner',
      '-loglevel',
      'error',
      '-i',
      audio,
      '-vn',
      '-c:a',
      'libmp3lame',
      '-q:a',
      '0',
      mp3,
    ]);
    for (const output of [merged, mp3]) {
      await assertNonEmptyFile(output);
      run(binary, [
        '-hide_banner',
        '-loglevel',
        'error',
        '-i',
        output,
        '-f',
        'null',
        '-',
      ]);
    }
    log('MP4 合流与 MP3 转码冒烟通过');
  } finally {
    await rm(tempDir, { recursive: true, force: true });
  }
}

async function verifyBinary(binary, smoke) {
  const metadata = inspectBinary(binary);
  const errors = validateMetadata(metadata);
  if (errors.length > 0) throw new Error(errors.join('\n'));
  const versionLine =
    metadata.versionOutput.split(/\r?\n/).find((line) => line.startsWith('ffmpeg version')) ??
    'ffmpeg version 未知';
  log(versionLine);
  log(`架构=${metadata.architectures.join(',')} minos=${metadata.minos}`);
  log('动态依赖：');
  for (const dependency of metadata.dependencies) log(`  ${dependency}`);
  if (smoke) await runSmoke(binary);
}

async function verifyApp(app, smoke) {
  const appInfo = await stat(app);
  if (!appInfo.isDirectory()) throw new Error(`不是 .app 目录：${app}`);
  const binary = path.join(app, 'Contents', 'MacOS', 'ffmpeg');
  await assertNonEmptyFile(binary);
  await verifyBinary(binary, smoke);

  for (const name of REQUIRED_PACKAGE_FILES) {
    const file = path.join(app, 'Contents', 'Resources', name);
    await assertNonEmptyFile(file);
    const content = await readFile(file, 'utf8');
    if (content.trim() === '') throw new Error(`资源内容为空：${name}`);
  }
  run('codesign', ['--verify', '--deep', '--strict', '--verbose=2', app]);
  log('包内 ffmpeg、许可资源与 codesign 验证通过');
}

function selfTest() {
  const good = {
    architectures: ['arm64'],
    minos: '13.0',
    dependencies: ['/usr/lib/libSystem.B.dylib'],
    versionOutput:
      'configuration: --disable-gpl --disable-nonfree --enable-libmp3lame',
    encoders: ['libmp3lame'],
    demuxers: ['mov'],
    muxers: ['mp4', 'mp3'],
  };
  const cases = [
    ['基线', good, 0],
    ['非 arm64', { ...good, architectures: ['x86_64'] }, 'arm64'],
    [
      'Homebrew 动态依赖',
      { ...good, dependencies: ['/opt/homebrew/opt/libxcb/lib/libxcb.1.dylib'] },
      '非系统动态依赖',
    ],
    [
      'GPL 配置',
      { ...good, versionOutput: good.versionOutput.replace('--disable-gpl', '--enable-gpl') },
      '--enable-gpl',
    ],
    [
      '缺失 libmp3lame',
      { ...good, encoders: [] },
      'libmp3lame',
    ],
  ];

  for (const [name, metadata, expectation] of cases) {
    const errors = validateMetadata(metadata);
    if (expectation === 0) {
      if (errors.length > 0) throw new Error(`${name} 自测失败：${errors.join('; ')}`);
    } else if (!errors.some((error) => error.includes(expectation))) {
      throw new Error(`${name} 自测未捕获预期错误 ${expectation}: ${errors.join('; ')}`);
    }
    log(`自测通过：${name}`);
  }
}

async function main() {
  const args = parseArgs(process.argv.slice(2));
  if (args.selfTest) {
    selfTest();
    if (!args.binary && !args.app) return;
  }
  if (args.binary) await verifyBinary(args.binary, args.smoke);
  if (args.app) await verifyApp(args.app, args.smoke);
  if (!args.selfTest && !args.binary && !args.app) {
    throw new Error('请指定 --binary、--app 或 --self-test');
  }
}

const isMain =
  process.argv[1] &&
  path.resolve(process.argv[1]) === fileURLToPath(import.meta.url);
if (isMain) {
  main().catch((error) => {
    console.error('[verify-ffmpeg-macos] 失败:', error.message);
    process.exit(1);
  });
}
