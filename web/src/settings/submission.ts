/**
 * 设置提交队列（design D3/D4）：按用户操作顺序串行提交 patch。
 *
 * · 空闲时连续操作合并为同一个待提交补丁（同字段以后一次为准）；
 * · 有提交在途时，新操作进入下一批，前一批完成后再提交，保证顺序；
 * · 同一批的等待者在批次成功/失败时统一结算；
 * · `idle()` 供关闭设置窗口时等待在途保存。
 */
import type { SettingsPatch } from '../lib/types';

export type SettingsPatchRun = (patch: SettingsPatch) => Promise<boolean>;

function isPlainGroup(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null;
}

/** 两级设置补丁合并：后一次操作同字段覆盖前一次，不同字段全部保留。 */
export function mergePatches(base: SettingsPatch, patch: SettingsPatch): SettingsPatch {
  const merged: Record<string, Record<string, unknown>> = {};
  for (const source of [base, patch]) {
    for (const [group, value] of Object.entries(source)) {
      if (!isPlainGroup(value)) continue;
      merged[group] = { ...merged[group], ...value };
    }
  }
  return merged as SettingsPatch;
}

export class SettingsSubmitQueue {
  private pending: SettingsPatch | null = null;
  private pendingWaiters: Array<(ok: boolean) => void> = [];
  private running = false;
  private idleWaiters: Array<() => void> = [];

  constructor(
    private readonly run: SettingsPatchRun,
    private readonly onBusyChange: (busy: boolean) => void = () => {},
  ) {}

  get busy(): boolean {
    return this.running || this.pending !== null;
  }

  /** 入队一个补丁；返回该批提交是否成功。 */
  enqueue(patch: SettingsPatch): Promise<boolean> {
    this.pending = this.pending ? mergePatches(this.pending, patch) : patch;
    const promise = new Promise<boolean>((resolve) => {
      this.pendingWaiters.push(resolve);
    });
    this.onBusyChange(true);
    this.pump();
    return promise;
  }

  /** 等待当前及所有待提交批次结束。 */
  idle(): Promise<void> {
    if (!this.busy) return Promise.resolve();
    return new Promise<void>((resolve) => {
      this.idleWaiters.push(resolve);
    });
  }

  /** 测试/重新建立会话时清空（不会终止已在执行的 submit）。 */
  clear(): void {
    const waiters = this.pendingWaiters;
    this.pending = null;
    this.pendingWaiters = [];
    for (const waiter of waiters) waiter(false);
    this.onBusyChange(this.running);
    if (!this.running) this.flushIdle();
  }

  private pump(): void {
    if (this.running || !this.pending) return;
    const patch = this.pending;
    const waiters = this.pendingWaiters;
    this.pending = null;
    this.pendingWaiters = [];
    this.running = true;
    this.onBusyChange(true);
    void (async () => {
      let ok = false;
      try {
        ok = await this.run(patch);
      } catch {
        ok = false;
      }
      this.running = false;
      for (const waiter of waiters) waiter(ok);
      if (this.pending) {
        // 仍有下一批：保持 busy，继续按序提交。
        this.pump();
        return;
      }
      this.onBusyChange(false);
      this.flushIdle();
    })();
  }

  private flushIdle(): void {
    const waiters = this.idleWaiters;
    this.idleWaiters = [];
    for (const waiter of waiters) waiter();
  }
}
