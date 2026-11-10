/**
 * 设置成功提示（design D4 / tasks 4.7）：一次性到期计时。
 *
 * 不轮询、不重复提示：每次保存成功只安排一个 timeout；窗口隐藏或组件卸载时释放；
 * 隐藏期间到达的成功结果不再补显示已结束的提示。
 */
export const SAVED_FEEDBACK_MS = 1600;

export interface SaveFeedbackOptions {
  durationMs?: number;
  onChange: (visible: boolean) => void;
  /** 当前是否隐藏（窗口最小化/切走时不展示提示，也不保留计时器） */
  isHidden?: () => boolean;
}

export class SaveFeedbackController {
  private timer: ReturnType<typeof setTimeout> | null = null;
  private visible = false;
  private suspended = false;
  private readonly durationMs: number;

  constructor(private readonly options: SaveFeedbackOptions) {
    this.durationMs = options.durationMs ?? SAVED_FEEDBACK_MS;
  }

  get isVisible(): boolean {
    return this.visible;
  }

  get hasTimer(): boolean {
    return this.timer !== null;
  }

  /** 本次保存成功：显示「已保存」并在一次性的到期计时结束后隐藏。 */
  markSaved(): void {
    this.clearTimer();
    if (this.suspended || this.options.isHidden?.()) {
      this.setVisible(false);
      return;
    }
    this.setVisible(true);
    this.timer = setTimeout(() => {
      this.timer = null;
      this.setVisible(false);
    }, this.durationMs);
  }

  /** 窗口隐藏/恢复：隐藏时释放计时器，恢复后不补显示已结束的提示。 */
  setSuspended(suspended: boolean): void {
    this.suspended = suspended;
    if (suspended) {
      this.clearTimer();
      this.setVisible(false);
    }
  }

  hide(): void {
    this.clearTimer();
    this.setVisible(false);
  }

  dispose(): void {
    this.clearTimer();
    this.visible = false;
  }

  private clearTimer(): void {
    if (this.timer !== null) {
      clearTimeout(this.timer);
      this.timer = null;
    }
  }

  private setVisible(visible: boolean): void {
    if (this.visible === visible) return;
    this.visible = visible;
    this.options.onChange(visible);
  }
}
