import { describe, expect, it } from 'vitest';
import {
  ANONYMOUS_ACCOUNT,
  accountFromDTO,
  defaultInternalSettings,
  parseItemFromDTO,
  settingsFromDTO,
  settingsToDTO,
  snapshotFromDTO,
  taskFromDTO,
  taskToDTO,
} from '../src/lib/adapt';
import type {
  HistoryEntryDTO,
  ParseResultDTO,
  QueueSnapshotDTO,
  SettingsDTO,
  TaskDTO,
} from '../src/lib/contract';
import { historyFromDTO, historyToDTO } from '../src/lib/adapt';

function makeTaskDTO(overrides: Partial<TaskDTO> = {}): TaskDTO {
  return {
    id: 'task-1',
    status: 'downloading',
    phase: 'video',
    sourceKind: 'video',
    sourceId: 'BV1mock0001',
    title: '测试视频',
    coverUrl: '/covers/cover-01.svg',
    pageUrl: 'https://www.bilibili.com/video/BV1mock0001',
    quality: '1080P',
    outputPath: null,
    error: null,
    restored: false,
    sortIndex: 0,
    createdAt: 1_000,
    updatedAt: 2_000,
    progress: {
      bytesDone: 400,
      bytesTotal: 1_000,
      stageDone: 400,
      stageTotal: 1_000,
      percent: 0.4,
      speedBps: 1024,
      etaSecs: 30,
    },
    owner: '示例 UP 主',
    entries: ['P1'],
    entryCount: 1,
    codec: 'avc',
    audioFormat: 'original',
    outputDir: '~/Downloads/bilidown',
    ...overrides,
  };
}

describe('adapt：契约 DTO ↔ UI 模型', () => {
  it('TaskDTO 映射：done → completed、参数/owner/进度', () => {
    const task = taskFromDTO(
      makeTaskDTO({
        status: 'done',
        phase: 'postprocess',
        outputPath: '~/Downloads/bilidown/测试视频[1080P].mp4',
        progress: {
          bytesDone: 1_000,
          bytesTotal: 1_000,
          stageDone: 1_000,
          stageTotal: 1_000,
          percent: 1,
          speedBps: 0,
          etaSecs: null,
        },
      }),
    );
    expect(task.status).toBe('completed');
    expect(task.uploader).toBe('示例 UP 主');
    expect(task.params.qualityLabel).toBe('1080P');
    expect(task.outputPath).toContain('测试视频');
    expect(task.stages.every((stage) => stage.status === 'done' || stage.status === 'skipped')).toBe(true);
  });

  it('阶段字节按视频/音频真实占比拆分，不出现两个阶段完全相同', () => {
    const task = taskFromDTO(makeTaskDTO());
    const video = task.stages.find((stage) => stage.phase === 'video');
    const audio = task.stages.find((stage) => stage.phase === 'audio');
    expect(video?.totalBytes).toBe(880);
    expect(audio?.totalBytes).toBe(120);
    expect(video?.doneBytes).toBeGreaterThan(audio?.doneBytes ?? 0);
  });

  it('错误分类映射：login-expired → auth，未知 action 被过滤', () => {
    const task = taskFromDTO(
      makeTaskDTO({
        status: 'failed',
        error: { kind: 'login-expired', message: '登录已失效', actions: ['relogin', 'unknown-action'] },
      }),
    );
    expect(task.error?.kind).toBe('auth');
    expect(task.error?.actions).toEqual(['relogin']);
  });

  it('persistError 映射到内部模型并可往返（design D8/3.8）', () => {
    const task = taskFromDTO(
      makeTaskDTO({ status: 'done', persistError: '任务记录保存失败：database is locked' }),
    );
    expect(task.persistError).toBe('任务记录保存失败：database is locked');
    expect(taskToDTO(task).persistError).toBe('任务记录保存失败：database is locked');

    const clean = taskFromDTO(makeTaskDTO());
    expect(clean.persistError).toBeNull();
  });

  it('QueueSnapshotDTO → 内部分类统计与账号能力', () => {
    const dto: QueueSnapshotDTO = {
      revision: 3,
      tasks: [makeTaskDTO(), makeTaskDTO({ id: 'task-2', status: 'failed', error: { kind: 'network', message: 'x', actions: [] } })],
      stats: { total: 2, queued: 0, active: 1, paused: 0, failed: 1, finished: 0, cancelled: 0, bytesDone: 400, bytesTotal: 1_000 },
      account: null,
      capability: { anonymousMax: '480P', signedInMax: '1080P', vipMax: '4K' },
      updatedAt: 5,
    };
    const snapshot = snapshotFromDTO(dto);
    expect(snapshot.stats.downloading).toBe(1);
    expect(snapshot.stats.failed).toBe(1);
    expect(snapshot.account.signedIn).toBe(false);
    expect(snapshot.account.capability.signedInMax).toBe(1080);
    expect(snapshot.account.capability.vipMax).toBe(2160);
  });

  it('账号映射：匿名 / 登录 / 大会员', () => {
    expect(accountFromDTO(null).signedIn).toBe(false);
    const user = accountFromDTO({
      mid: 1,
      uname: '示例用户',
      avatarUrl: null,
      vip: 'annual',
      level: 6,
    });
    expect(user.signedIn).toBe(true);
    expect(user.vip).toBe('annual');
    expect(user.name).toBe('示例用户');
  });

  it('Settings 往返：DTO ↔ 内部模型保持语义（清晰度策略/代理/主题）', () => {
    const internal = defaultInternalSettings();
    const dto = settingsToDTO(internal);
    const back = settingsFromDTO(dto, internal);
    expect(back.download.outputDir).toBe(internal.download.outputDir);
    expect(back.interface.appearance).toBe('system');
    expect(back.quality.codecPreference).toBe('avc');
    expect(back.network.proxyMode).toBe('direct');

    const fixedDto: SettingsDTO = { ...dto, quality: '720P', theme: 'dark' };
    const fixed = settingsFromDTO(fixedDto, internal);
    expect(fixed.quality.strategy).toBe('fixed');
    expect(fixed.quality.fixedQuality).toBe(64);
    expect(fixed.interface.appearance).toBe('dark');
  });

  it('历史 DTO 往返', () => {
    const dto: HistoryEntryDTO = {
      id: 'h-1',
      taskId: 'task-1',
      title: '测试视频',
      coverUrl: null,
      sourceKind: 'video',
      sourceId: 'BV1',
      pageUrl: 'https://www.bilibili.com/video/BV1mock0001',
      quality: '1080P',
      outputPath: '~/Downloads/bilidown/x.mp4',
      fileSize: 1024,
      status: 'completed',
      error: null,
      finishedAt: 1_000,
      owner: '示例 UP 主',
      fileExists: false,
    };
    const entry = historyFromDTO(dto);
    expect(entry.uploader).toBe('示例 UP 主');
    expect(entry.fileExists).toBe(false);
    const back = historyToDTO(entry);
    expect(back.quality).toBe('1080P');
    expect(back.fileExists).toBe(false);
  });

  it('多产物与旧记录覆盖范围在 DTO 往返中保持', () => {
    const artifacts = [
      {
        path: '/tmp/p3.video.m4s',
        size: 5,
        entryIndex: 2,
        entryTitle: 'P3',
        kind: 'video-stream' as const,
        exists: true,
        sizeMatches: true,
        valid: true,
      },
      {
        path: '/tmp/p3.audio.m4s',
        size: 4,
        entryIndex: 2,
        entryTitle: 'P3',
        kind: 'audio-stream' as const,
        exists: true,
        sizeMatches: true,
        valid: true,
      },
    ];
    const task = taskFromDTO(
      makeTaskDTO({
        artifacts,
        artifactCoverage: 'complete',
        outputPath: artifacts[0].path,
      }),
    );
    expect(task.artifacts).toHaveLength(2);
    expect(task.artifactCoverage).toBe('complete');

    const history = historyFromDTO({
      id: 'legacy',
      taskId: 'task-old',
      title: '旧任务',
      coverUrl: null,
      sourceKind: 'video',
      sourceId: 'BV1',
      pageUrl: 'https://www.bilibili.com/video/BV1',
      quality: '1080P',
      outputPath: '/tmp/old.mp4',
      fileSize: 1024,
      artifacts: [
        {
          path: '/tmp/old.mp4',
          size: 1024,
          entryIndex: null,
          entryTitle: null,
          kind: 'media',
          exists: true,
          sizeMatches: true,
          valid: true,
        },
      ],
      artifactCoverage: 'legacy-single',
      status: 'completed',
      error: null,
      finishedAt: 1,
      fileExists: true,
    });
    expect(history.artifactCoverage).toBe('legacy-single');
    expect(history.artifacts).toHaveLength(1);
  });

  it('ParseResultDTO → 添加面板条目（默认选择语义）', () => {
    const dto: ParseResultDTO = {
      kind: 'bangumi',
      sourceId: 'ss1',
      title: '示例番剧',
      coverUrl: null,
      pageUrl: 'https://www.bilibili.com/bangumi/play/ss1',
      owner: '示例制作组',
      rawInput: 'https://www.bilibili.com/bangumi/play/ss1',
      entries: [
        { index: 0, title: '第 1 话', durationSecs: 1_400, selectedDefault: true },
        { index: 1, title: '第 2 话', durationSecs: 1_420, selectedDefault: false },
      ],
      qualities: [
        { id: 80, label: '1080P', resolution: '1920×1080', fps: 30, vip: false, available: true },
      ],
      codecs: [{ id: 'avc', label: 'AVC', available: true }],
    };
    const item = parseItemFromDTO(dto, ANONYMOUS_ACCOUNT);
    expect(item.entries).toHaveLength(2);
    expect(item.entries.filter((entry) => entry.selectedByDefault)).toHaveLength(1);
    expect(item.qualities[0].label).toBe('1080P');
    expect(item.uploader).toBe('示例制作组');
  });
});
