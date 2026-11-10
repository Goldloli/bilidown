/** 文件操作入口的文件状态判断（design D7/3.3）：缺失/大小不符/记录异常时给出可读提示，不当成功。 */
import { t } from '../i18n/zh-CN';
import type { QueueTask, TaskArtifact } from './types';

export interface FileTarget {
  path: string;
  /** 是否可以进行打开/定位操作 */
  available: boolean;
  /** 不可用时的可读原因 */
  reason: string | null;
}

function targetOfArtifact(artifact: TaskArtifact): FileTarget {
  if (!artifact.exists) {
    return { path: artifact.path, available: false, reason: t('file.missing') };
  }
  if (!artifact.sizeMatches) {
    return { path: artifact.path, available: false, reason: t('file.sizeMismatch') };
  }
  if (!artifact.valid) {
    return { path: artifact.path, available: false, reason: t('file.invalid') };
  }
  return { path: artifact.path, available: true, reason: null };
}

export function artifactFileTarget(artifact: TaskArtifact): FileTarget {
  return targetOfArtifact(artifact);
}

/**
 * 任务主要文件入口：优先第一个有效产物；没有产物记录时回退旧记录的主输出路径。
 * 旧记录无法判断文件有效性时不做「文件已存在」的虚构，仍先尝试打开，失败再提示。
 */
export function taskFileTarget(task: QueueTask): FileTarget {
  const artifacts = task.artifacts ?? [];
  if (artifacts.length > 0) {
    const valid = artifacts.find(
      (artifact) => artifact.exists && artifact.sizeMatches && artifact.valid,
    );
    return targetOfArtifact(valid ?? artifacts[0]);
  }
  if (task.outputPath) return { path: task.outputPath, available: true, reason: null };
  return { path: '', available: false, reason: t('file.missing') };
}
