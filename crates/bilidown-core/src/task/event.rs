//! 任务事件（broadcast 通道，立即推送给壳 → 前端）。
//!
//! 进度不走事件通道（由 watch 快照 ≤10Hz 承载）；事件只覆盖
//! 创建/状态变化/终态/队列结构变化等重要节点。

use serde::{Deserialize, Serialize};

use super::state::{TaskId, TaskPhase, TaskView};
use crate::api::error::BiliError;

/// 结构化任务事件。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TaskEvent {
    /// 新任务入队（含快照视图）。
    Created { view: TaskView },
    /// 任务元信息更新（标题/封面/实际参数等）。
    Updated { view: TaskView },
    /// 阶段变化。
    PhaseChanged {
        id: TaskId,
        phase: TaskPhase,
        view: TaskView,
    },
    /// 完成（成功）。
    Done { view: TaskView },
    /// 失败（含结构化错误）。
    Failed { view: TaskView, error: BiliError },
    /// 已取消。
    Cancelled { id: TaskId },
    /// 已暂停。
    Paused { id: TaskId },
    /// 已继续/重新开始。
    Resumed { id: TaskId },
    /// 从队列移除（历史保留）。
    Removed { id: TaskId },
    /// 历史记录变化（前端刷新历史视图）。
    HistoryChanged,
    /// 清空/队列结构变化后建议前端重拉快照。
    QueueChanged,
}

impl TaskEvent {
    pub fn id(&self) -> Option<&str> {
        match self {
            TaskEvent::Created { view }
            | TaskEvent::Updated { view }
            | TaskEvent::PhaseChanged { view, .. }
            | TaskEvent::Done { view }
            | TaskEvent::Failed { view, .. } => Some(&view.id),
            TaskEvent::Cancelled { id }
            | TaskEvent::Paused { id }
            | TaskEvent::Resumed { id }
            | TaskEvent::Removed { id } => Some(id),
            TaskEvent::HistoryChanged | TaskEvent::QueueChanged => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_serde_shape() {
        let ev = TaskEvent::Paused { id: "abc".into() };
        let json = serde_json::to_value(&ev).unwrap();
        assert_eq!(json, serde_json::json!({"type": "paused", "id": "abc"}));
        let back: TaskEvent = serde_json::from_value(json).unwrap();
        assert_eq!(back, ev);
    }
}
