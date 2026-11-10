/** 基础文案（zh-CN）：首屏与通用 UI 使用；按页面分组的文案见 `zh-CN.dynamic.ts`。 */
import { isWindows } from '../lib/platform';

import type { dynamicStrings } from './zh-CN.dynamic';

type StaticKey = keyof typeof staticStrings;

const staticStrings = {
  'app.name': 'bilidown',

  'titlebar.account': '账户',
  'titlebar.settings': '偏好设置',

  'bottombar.add': '添加链接',
  'bottombar.clipboard': '从剪贴板添加',
  'bottombar.search': '搜索',
  'bottombar.clear': '清空已完成',
  'bottombar.history': '历史',
  'bottombar.help': '帮助',
  'bottombar.status.empty': '没有下载',
  'bottombar.status.downloading': '正在下载 {n} 项',
  'bottombar.status.downloadingSpeed': '正在下载 {n} 项 · {speed}',
  'bottombar.status.paused': '已暂停 {n} 项',
  'bottombar.status.failed': '{n} 项失败',
  'bottombar.status.completed': '{n} 项已完成',
  'bottombar.status.queued': '{n} 项等待中',
  'bottombar.menu.addUrl': '打开链接…',
  'bottombar.menu.clipboard': '从剪贴板添加…',
  'bottombar.menu.pauseAll': '暂停全部',
  'bottombar.menu.resumeAll': '继续全部',
  'bottombar.menu.retryFailed': '重试全部失败',
  'bottombar.menu.openSettings': '偏好设置…',
  'bottombar.menu.help': '快捷键与帮助',
  'bottombar.menu.logs': '显示日志',

  'empty.title': '拖入或粘贴 B 站链接',
  'empty.subtitle': '支持视频 · 番剧 · 课程 · 音频，粘贴后自动解析',
  'empty.action': '添加链接',
  'empty.hint': '也可以按 ⌘O 打开链接面板，或把链接拖到这里',
  'empty.drag': '松开以添加链接',
  'empty.brandAlt': 'bilidown 标识',

  'queue.loading': '正在加载队列…',
  'queue.error.title': '队列加载失败',
  'queue.error.retry': '重试',
  'queue.search.placeholder': '搜索队列',
  'queue.search.empty': '没有匹配的任务',
  'queue.search.clear': '清除搜索',
  'queue.restoredHint': '已从上次退出恢复，点继续下载',
  'queue.row.openDetail': '打开任务详情',
  'queue.row.pause': '暂停',
  'queue.row.resume': '继续',
  'queue.row.cancel': '取消下载',
  'queue.row.retry': '重试',
  'queue.row.remove': '从队列移除',
  'queue.row.reveal': '在 Finder 中显示',
  'queue.row.open': '打开文件',
  'queue.row.copy': '复制链接',
  'queue.row.more': '更多操作',
  'queue.row.reset': '重新设置…',
  'queue.row.delete.detail': '仅从队列移除，不影响已下载文件与历史。',

  'status.queued': '排队中',
  'status.resolving': '解析中…',
  'status.downloading': '下载中',
  'status.merging': '合并中…',
  'status.paused': '已暂停',
  'status.completed': '下载完成',
  'status.failed': '下载失败',
  'status.cancelled': '已取消',
  'status.restored': '已恢复',
  'status.fileMissing': '文件已不存在',

  'kind.video': '视频',
  'kind.bangumi': '番剧',
  'kind.course': '课程',
  'kind.audio': '音频',

  'phase.resolve': '解析信息',
  'phase.video': '视频流',
  'phase.audio': '音频流',
  'phase.merge': '合并',
  'phase.postprocess': '后处理',

  'artifacts.total': '共 {n} 个文件 · {size}',
  'artifacts.legacy': '旧记录：仅保留单文件信息，无法恢复旧版本未保存的其他文件',
  'account.menu.info': '账号信息',
  'account.menu.refresh': '刷新登录',
  'account.menu.logout': '退出登录',
  'account.menu.login': '登录 B 站…',
  'account.menu.settings': '偏好设置…',
  'account.logoutConfirm.title': '退出登录？',
  'account.logoutConfirm.body': '将清除本机保存的登录凭据，高清晰度能力将不可用。',
  'account.logoutConfirm.ok': '退出登录',
  'account.expiredHint': '登录已失效，请重新扫码',
  'account.anonymous': '未登录',
  'account.loginHint': '登录解锁 1080P',
  'account.validating': '账号校验中…',
  'account.validationDegraded': '账号校验未完成，点击重试',
  'account.validationPendingTitle': '正在校验账号状态，本地内容可继续使用',


  'settings.title': '设置',
  'common.cancel': '取消',
  'common.confirm': '确认',
  'common.remove': '移除',
  'common.retry': '重试',
  'common.unknown': '未知错误',
  'common.logout': '退出登录',
  'common.login': '登录',
  'common.close': '关闭',
  'common.copy': '复制',
  'common.copied': '已复制',
  'common.empty': '—',
  'common.validate': '校验失败',
  'common.search': '搜索',
  'common.clear': '清除',

  'error.risk': '触发 B 站风控，请稍后重试或先登录',
  'error.auth': '登录已失效，请重新扫码',
  'error.vip': '该清晰度需要大会员',
  'error.paid': '该内容需要付费或购买',
  'error.region': '该内容在所在地区不可用',
  'error.network': '网络连接失败，请检查网络后重试',
  'error.cdn': 'CDN 拒绝访问，已自动重试',
  'error.ffmpeg': '找不到 ffmpeg，无法合并或转码',
  'error.disk': '磁盘空间不足或没有写入权限',
  'error.parse': '内容解析失败',
  'error.unknown': '发生未知错误',

  'errorAction.retry': '重试',
  'errorAction.relogin': '重新登录',
  'errorAction.open-settings': '打开设置',
  'errorAction.copy-diagnostics': '复制诊断信息',
  'errorAction.open-log': '显示日志',
  'errorAction.choose-dir': '更换目录',
  'errorAction.install-ffmpeg': '安装指引',

  'toast.taskCompleted': '已下载完成：{title}',
  'toast.taskFailed': '下载失败：{title}',
  'toast.copied': '链接已复制',
  'toast.removed': '已从队列移除',
  'toast.cleared': '已清空已完成任务',
  'toast.clipboardEmpty': '剪贴板中没有识别到 B 站链接',
  'toast.clipboardAdded': '已从剪贴板添加链接',
  'toast.tasksAdded': '已添加 {n} 个任务',
  'toast.loginSuccess': '登录成功',
  'toast.loginExpired': '登录已失效，请重新扫码',
  'toast.historyCleared': '历史记录已清空',
  'toast.historyDeleted': '已删除记录',
  'toast.diagnosticsCopied': '诊断信息已复制',
  'toast.notImplemented': '该功能将在后续接入',

  'confirm.removeTask.title': '移除任务？',
  'confirm.removeTask.body': '{title}\n{detail}',
  'confirm.removeTask.ok': '移除',

  'tooltip.settings': '偏好设置 (⌘,)',
  'tooltip.account': '账户',
  'tooltip.login': '登录 B 站',

  'help.title': '快捷键',
  'help.openLink': '⌘O 打开链接',
  'help.clipboard': '⌃⌘V 从剪贴板添加',
  'help.startAll': '⌘S 开始全部',
  'help.pauseAll': '⌃⌘S 暂停全部',
  'help.retryAll': '⇧⌘R 重试全部失败',
  'help.clearDone': '⌘K 清空已完成',
  'help.clearIdle': '⇧⌘K 清空未开始',
  'help.settings': '⌘, 偏好设置',
  'help.close': '⌘W 关闭窗口',
  'help.quit': '⌘Q 退出应用',

  'collection.seasonTitle': '检测到合集',
  'collection.seasonBody': '《{title}》共 {n} 集，是否下载整个合集？',
  'collection.seasonConfirm': '下载整个合集',
  'collection.seasonCurrent': '仅当前视频',

  // ---- 任务记录持久化失败（design D8/3.8）----
  'persist.rowBadge': '记录未保存',
  'persist.title': '任务记录未保存',
  'persist.body': '任务记录写入失败；已下载的文件与产物不受影响。可重试保存。',
  'persist.retry': '重试保存',
  'persist.retrying': '保存中…',

  // ---- 文件缺失/操作失败（design D7/3.3）----
  'files.refreshFailed': '文件状态刷新失败：{message}',
  'files.checking': '文件状态核查中…',
  'file.missing': '文件已不存在，无法打开或定位',
  'file.sizeMismatch': '文件大小与记录不一致，无法打开',
  'file.invalid': '文件记录异常，无法打开',
  'file.openFailed': '打开文件失败：{message}',
  'file.revealFailed': '定位文件失败：{message}',

  // ---- 退出保存失败（design D8/3.9）----
  'toast.clearFinished': '已清空 {n} 个已完成任务',
  'toast.clearInactive': '已清空 {n} 个未激活任务',
  'toast.clearPartial': '部分失败，保留 {n} 行',
  'toast.clearEmpty': '没有可清理的任务',
  'toast.persistenceRetried': '已重试保存任务记录',
  'toast.persistenceRetryFailed': '重试保存失败：{message}',

  // ---- 按内容分别入队（design D8/6.4）----
  'startup.core.title': '引擎初始化失败',
  'startup.core.hint': '可先重试；若持续失败，请检查网络或数据目录权限后重启应用。',
  'startup.core.retry': '重试初始化',
  'startup.settingsFailed': '设置加载失败：{message}',
  'startup.historyFailed': '历史记录加载失败：{message}',
  'startup.networkWarning': '网络准备未完成：{message}',
  'startup.networkRetry': '重试网络',
  'startup.retry': '重试',
  'add.partial.title': '以下内容入队失败',
  'add.partial.summary': '已添加 {added} 个任务，{failed} 个内容入队失败',
  'add.partial.allFailed': '{failed} 个内容入队失败',
  'add.partial.retry': '重试失败内容（{n}）',
  'add.partial.remaining': '剩余 {n} 个内容待入队',
} as const;

/** 运行时文案表：基础文案 + 懒加载页面注册的文案。 */
const dictionary: Record<string, string> = { ...staticStrings };

export type StringKey = StaticKey | keyof typeof dynamicStrings;

/** 懒加载页面注册自己的文案（tasks 8.1：按页面拆分首屏文案）。 */
export function registerStrings(extra: Record<string, string>) {
  Object.assign(dictionary, extra);
}



/** Windows 平台术语替换（Finder→资源管理器、Dock→任务栏 等），避免 UI 残留 macOS 用语。 */
const WINDOWS_WORDING: Array<[string, string]> = [
  ['在 Finder 中显示', '在资源管理器中显示'],
  ['Finder', '资源管理器'],
  ['Dock 徽标', '任务栏进度'],
  ['Dock 图标', '任务栏图标'],
  ['macOS 外观', '系统外观'],
  ['macOS「减弱动态效果」', '系统「减弱动态效果」'],
  ['macOS', 'Windows'],
];

export function t(key: StringKey, vars?: Record<string, string | number>): string {
  let s: string = dictionary[key] ?? key;
  if (isWindows()) {
    for (const [from, to] of WINDOWS_WORDING) s = s.replaceAll(from, to);
  }
  if (!vars) return s;
  for (const [k, v] of Object.entries(vars)) {
    s = s.replace(new RegExp(`\\{${k}\\}`, 'g'), String(v));
  }
  return s;
}
