-- 004：任务元信息（design D7）。
-- 旧记录保持默认空对象 '{}'，缺失字段按未知值恢复，不伪造作者/条目/实际参数/附件。
ALTER TABLE tasks ADD COLUMN metadata TEXT NOT NULL DEFAULT '{}';
