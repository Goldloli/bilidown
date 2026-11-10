import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import {
  SAVED_FEEDBACK_MS,
  SaveFeedbackController,
} from '../src/settings/saveFeedback';

beforeEach(() => {
  vi.useFakeTimers();
});

afterEach(() => {
  vi.useRealTimers();
});

function tracker() {
  const visible: boolean[] = [];
  return {
    visible,
    onChange: (value: boolean) => visible.push(value),
  };
}

describe('保存成功提示一次性到期（design D4 / tasks 4.7）', () => {
  it('保存成功显示「已保存」，到期后自然消失且不残留计时器', () => {
    const state = tracker();
    const controller = new SaveFeedbackController({ onChange: state.onChange });

    controller.markSaved();
    expect(controller.isVisible).toBe(true);
    expect(state.visible).toEqual([true]);
    expect(vi.getTimerCount()).toBe(1);

    vi.advanceTimersByTime(SAVED_FEEDBACK_MS - 1);
    expect(controller.isVisible).toBe(true);

    vi.advanceTimersByTime(1);
    expect(controller.isVisible).toBe(false);
    expect(state.visible).toEqual([true, false]);
    expect(vi.getTimerCount()).toBe(0);
  });

  it('不创建永久 interval：提示结束后没有任何定时器持续刷新', () => {
    const state = tracker();
    const controller = new SaveFeedbackController({ onChange: state.onChange });

    controller.markSaved();
    vi.advanceTimersByTime(SAVED_FEEDBACK_MS);
    expect(vi.getTimerCount()).toBe(0);

    // 再等待更久也不会有新的状态变化（无轮询）。
    vi.advanceTimersByTime(10_000);
    expect(state.visible).toEqual([true, false]);
    expect(vi.getTimerCount()).toBe(0);
  });

  it('重复保存只替换一次到期计时，不叠加提示', () => {
    const controller = new SaveFeedbackController({ onChange: () => {} });
    controller.markSaved();
    vi.advanceTimersByTime(1000);
    controller.markSaved();
    expect(vi.getTimerCount()).toBe(1);
    vi.advanceTimersByTime(SAVED_FEEDBACK_MS - 1);
    expect(controller.isVisible).toBe(true);
    vi.advanceTimersByTime(1);
    expect(controller.isVisible).toBe(false);
    expect(vi.getTimerCount()).toBe(0);
  });

  it('窗口隐藏时释放计时器，恢复后不补显示已结束的提示', () => {
    const state = tracker();
    const controller = new SaveFeedbackController({ onChange: state.onChange });
    controller.markSaved();
    expect(vi.getTimerCount()).toBe(1);

    controller.setSuspended(true);
    expect(controller.isVisible).toBe(false);
    expect(vi.getTimerCount()).toBe(0);

    // 隐藏期间保存成功：不排队补显示。
    controller.markSaved();
    expect(controller.isVisible).toBe(false);
    expect(vi.getTimerCount()).toBe(0);
    controller.setSuspended(false);
    vi.advanceTimersByTime(SAVED_FEEDBACK_MS);
    expect(controller.isVisible).toBe(false);
    expect(state.visible).toEqual([true, false]);
  });

  it('组件卸载/窗口关闭后释放计时器，不再触发提示', () => {
    const state = tracker();
    const controller = new SaveFeedbackController({ onChange: state.onChange });
    controller.markSaved();
    controller.dispose();
    expect(vi.getTimerCount()).toBe(0);

    vi.advanceTimersByTime(SAVED_FEEDBACK_MS * 2);
    expect(state.visible).toEqual([true]);
    expect(controller.isVisible).toBe(false);
  });

  it('isHidden 判定为隐藏时不显示提示', () => {
    const state = tracker();
    let hidden = true;
    const controller = new SaveFeedbackController({
      onChange: state.onChange,
      isHidden: () => hidden,
    });
    controller.markSaved();
    expect(state.visible).toEqual([]);
    expect(vi.getTimerCount()).toBe(0);

    hidden = false;
    controller.markSaved();
    expect(state.visible).toEqual([true]);
  });
});
