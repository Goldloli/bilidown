#!/usr/bin/env node
/**
 * 进程 CPU/内存采样（tasks 1.3 / 8.4）：macOS/Linux 下对指定 PID 按固定间隔采样，
 * 输出平均值/最大值与原始序列，供「同一构建、同设置」的空闲/活跃对照使用。
 *
 * 用法：
 *   node scripts/perf-sample.mjs <pid> --seconds 60 --interval 1 \
 *     --label idle --json docs/qa/improve-client-reliability-and-performance/cpu-idle.json
 *
 * 说明：CPU 为 `ps -o %cpu` 的瞬时百分比（多核可 >100）；应用自身唤醒次数在 macOS 需
 * `powermetrics --show-process-wakeups`（需权限），本脚本不采集，可由调用方补充。
 */
import { execFileSync } from 'node:child_process';
import fs from 'node:fs/promises';
import path from 'node:path';

function arg(name, fallback) {
  const index = process.argv.indexOf(`--${name}`);
  return index >= 0 ? process.argv[index + 1] : fallback;
}

const pid = Number(process.argv[2]);
if (!Number.isInteger(pid) || pid <= 0) {
  console.error('用法: node scripts/perf-sample.mjs <pid> [--seconds 60] [--interval 1] [--label idle] [--json out.json]');
  process.exit(1);
}
const seconds = Number(arg('seconds', '60'));
const interval = Number(arg('interval', '1'));
const label = arg('label', 'sample');
const outPath = arg('json', null);

function sample() {
  try {
    // macOS/Linux 通用关键字：%cpu 与 rss（线程数不是所有 ps 都支持，省略）。
    const output = execFileSync('ps', ['-p', String(pid), '-o', '%cpu=,rss='], {
      encoding: 'utf8',
    }).trim();
    if (!output) return null;
    const parts = output.split(/\s+/);
    return {
      cpuPercent: Number(parts[0]),
      rssKiB: Number(parts[1]),
    };
  } catch {
    return null;
  }
}

const samples = [];
const startedAt = new Date();
const iterations = Math.max(1, Math.round(seconds / interval));
for (let index = 0; index < iterations; index += 1) {
  const samplePoint = sample();
  if (!samplePoint) {
    console.error(`进程 ${pid} 已退出或不可采样`);
    break;
  }
  samples.push({ at: new Date().toISOString(), ...samplePoint });
  await new Promise((resolve) => setTimeout(resolve, interval * 1000));
}

if (samples.length === 0) {
  console.error('没有采集到样本');
  process.exit(2);
}
const cpuValues = samples.map((samplePoint) => samplePoint.cpuPercent);
const rssValues = samples.map((samplePoint) => samplePoint.rssKiB);
const summary = {
  label,
  pid,
  seconds: numbers((samples.length * interval).toFixed(1)),
  samples: samples.length,
  cpuPercent: {
    avg: numbers(average(cpuValues).toFixed(2)),
    max: numbers(Math.max(...cpuValues).toFixed(2)),
    min: numbers(Math.min(...cpuValues).toFixed(2)),
  },
  rssMiB: {
    avg: numbers((average(rssValues) / 1024).toFixed(1)),
    max: numbers((Math.max(...rssValues) / 1024).toFixed(1)),
  },
  startedAt: startedAt.toISOString(),
};

function average(values) {
  return values.reduce((sum, value) => sum + value, 0) / values.length;
}
function numbers(value) {
  return typeof value === 'string' ? Number(value) : value;
}

console.log(`[${label}] pid=${pid} 采样 ${summary.samples} 次 / ${summary.seconds}s`);
console.log(
  `  CPU: avg ${summary.cpuPercent.avg}%  max ${summary.cpuPercent.max}%  min ${summary.cpuPercent.min}%`,
);
console.log(`  RSS: avg ${summary.rssMiB.avg} MiB  max ${summary.rssMiB.max} MiB`);
if (outPath) {
  await fs.mkdir(path.dirname(outPath), { recursive: true });
  await fs.writeFile(outPath, `${JSON.stringify({ summary, samples }, null, 2)}\n`);
  console.log(`  原始数据: ${outPath}`);
}
