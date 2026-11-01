-- 001：任务队列与下载历史初始表（design D4）
CREATE TABLE IF NOT EXISTS tasks (
  id TEXT PRIMARY KEY,
  status TEXT NOT NULL,
  phase TEXT NOT NULL,
  source_kind TEXT NOT NULL,
  source_id TEXT NOT NULL,
  title TEXT NOT NULL DEFAULT '',
  cover_url TEXT,
  page_url TEXT,
  params TEXT NOT NULL DEFAULT '{}',
  progress TEXT NOT NULL DEFAULT '{}',
  output_path TEXT,
  error TEXT,
  restored INTEGER NOT NULL DEFAULT 0,
  sort_index INTEGER NOT NULL DEFAULT 0,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS history (
  id TEXT PRIMARY KEY,
  task_id TEXT,
  title TEXT NOT NULL,
  cover_url TEXT,
  source_kind TEXT NOT NULL,
  source_id TEXT NOT NULL,
  page_url TEXT,
  quality TEXT,
  output_path TEXT,
  file_size INTEGER,
  status TEXT NOT NULL,
  error TEXT,
  finished_at INTEGER NOT NULL
);
