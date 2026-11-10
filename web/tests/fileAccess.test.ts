import { describe, expect, it } from 'vitest';
import { artifactFileTarget, taskFileTarget } from '../src/lib/fileAccess';
import { errorMessageOf } from '../src/lib/errors';
import { makeTask } from '../src/dev/mockData';
import type { TaskArtifact } from '../src/lib/types';

function artifact(overrides: Partial<TaskArtifact> = {}): TaskArtifact {
  return {
    path: '/tmp/a.mp4',
    size: 100,
    entryIndex: 0,
    entryTitle: 'P1',
    kind: 'media',
    exists: true,
    sizeMatches: true,
    valid: true,
    ...overrides,
  };
}

describe('文件入口状态判断（design D7/3.3）', () => {
  it('有效产物可打开/定位', () => {
    const target = artifactFileTarget(artifact());
    expect(target).toEqual({ path: '/tmp/a.mp4', available: true, reason: null });
  });

  it('缺失、大小不符、记录异常均不可打开并给出可读原因', () => {
    expect(artifactFileTarget(artifact({ exists: false })).available).toBe(false);
    expect(artifactFileTarget(artifact({ exists: false })).reason).toContain('不存在');
    expect(artifactFileTarget(artifact({ sizeMatches: false })).reason).toContain('大小');
    expect(artifactFileTarget(artifact({ valid: false })).reason).toContain('异常');
  });

  it('任务优先使用第一个有效产物，退回首个产物说明缺失原因', () => {
    const task = makeTask({
      id: 'multi',
      title: '多产物',
      uploader: 'UP',
      kind: 'video',
      coverIndex: 0,
      status: 'completed',
      percent: 1,
    });
    task.artifacts = [
      artifact({ path: '/tmp/missing.mp4', exists: false }),
      artifact({ path: '/tmp/p2.mp4', entryIndex: 1, entryTitle: 'P2' }),
    ];

    expect(taskFileTarget(task)).toEqual({ path: '/tmp/p2.mp4', available: true, reason: null });

    task.artifacts = [artifact({ path: '/tmp/missing.mp4', exists: false })];
    const target = taskFileTarget(task);
    expect(target.available).toBe(false);
    expect(target.reason).toContain('不存在');
  });

  it('旧记录没有产物列表时回退主输出路径；都没有则不可操作', () => {
    const withOutput = makeTask({
      id: 'legacy',
      title: '旧记录',
      uploader: 'UP',
      kind: 'video',
      coverIndex: 0,
      status: 'completed',
      percent: 1,
      outputPath: '~/Downloads/old.mp4',
    });
    expect(taskFileTarget(withOutput)).toEqual({
      path: '~/Downloads/old.mp4',
      available: true,
      reason: null,
    });

    const withoutOutput = makeTask({
      id: 'legacy-empty',
      title: '旧记录',
      uploader: 'UP',
      kind: 'video',
      coverIndex: 0,
      status: 'completed',
      percent: 1,
    });
    expect(taskFileTarget(withoutOutput).available).toBe(false);
  });

  it('AppError 形态的错误提取可读 message', () => {
    expect(errorMessageOf({ kind: 'disk', message: '磁盘写入失败', actions: [] })).toBe(
      '磁盘写入失败',
    );
    expect(errorMessageOf(new Error('普通错误'))).toBe('普通错误');
  });
});
