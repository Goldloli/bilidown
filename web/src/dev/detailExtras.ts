import type { QueueTask, TaskExtras } from '../lib/types';

function safeBase(task: QueueTask): string {
  return task.title.replace(/[\\/:*?"<>|]/g, '_').slice(0, 48);
}

/**
 * mock 的只读下载产物（与最终契约 TaskExtras 对齐；仅完成态有值）。
 * 真实模式由 core/shell 提供 extras，经 adapt 映射；这里仅供截图/自测。
 */
export function mockTaskExtras(task: QueueTask): TaskExtras | null {
  if (task.status !== 'completed') return null;
  const base = safeBase(task);
  return {
    subtitles: [`${base}.zh-CN.srt`, `${base}.ai-zh.srt`],
    cover: `${task.params.outputDir}/${base}.jpg`,
    subtitleError: null,
    coverError: task.id === 'task-done' ? '封面下载失败：HTTP 403（mock）' : null,
  };
}
