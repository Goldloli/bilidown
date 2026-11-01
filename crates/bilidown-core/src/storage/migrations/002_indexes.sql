-- 002：队列/历史查询索引（增量迁移示例；不重建表、不丢数据）
CREATE INDEX IF NOT EXISTS idx_tasks_status ON tasks(status);
CREATE INDEX IF NOT EXISTS idx_tasks_updated ON tasks(updated_at);
CREATE INDEX IF NOT EXISTS idx_history_finished ON history(finished_at DESC);
CREATE INDEX IF NOT EXISTS idx_history_status ON history(status);
