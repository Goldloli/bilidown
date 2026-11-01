//! SQLite 持久化（rusqlite bundled，WAL）：任务队列与下载历史。
//!
//! schema 版本用 `PRAGMA user_version` + 顺序迁移脚本（`migrations/*.sql`），
//! 增量迁移，不重建表、不丢数据（design D4）。

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use rusqlite::{Connection, OptionalExtension, params};

use crate::api::error::{BiliError, BiliResult};

/// 顺序迁移脚本（下标 = user_version）。
const MIGRATIONS: &[&str] = &[
    include_str!("migrations/001_init.sql"),
    include_str!("migrations/002_indexes.sql"),
    include_str!("migrations/003_task_artifacts.sql"),
];

/// 当前 schema 版本。
pub const SCHEMA_VERSION: i32 = MIGRATIONS.len() as i32;

/// 队列任务行（JSON 字段由任务层负责序列化）。
#[derive(Debug, Clone, PartialEq)]
pub struct TaskRecord {
    pub id: String,
    pub status: String,
    pub phase: String,
    pub source_kind: String,
    pub source_id: String,
    pub title: String,
    pub cover_url: Option<String>,
    pub page_url: Option<String>,
    /// TaskSpec JSON。
    pub params: String,
    /// Progress JSON。
    pub progress: String,
    pub output_path: Option<String>,
    pub artifacts: String,
    /// BiliError JSON。
    pub error: Option<String>,
    pub restored: bool,
    pub sort_index: i64,
    pub created_at: i64,
    pub updated_at: i64,
}

/// 历史记录行。
#[derive(Debug, Clone, PartialEq)]
pub struct HistoryRecord {
    pub id: String,
    pub task_id: Option<String>,
    pub title: String,
    pub cover_url: Option<String>,
    pub source_kind: String,
    pub source_id: String,
    pub page_url: Option<String>,
    pub quality: Option<String>,
    pub output_path: Option<String>,
    pub file_size: Option<i64>,
    pub artifacts: String,
    pub status: String,
    /// BiliError JSON。
    pub error: Option<String>,
    pub finished_at: i64,
}

/// 历史查询条件。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct HistoryQuery {
    /// 状态过滤：`done` / `failed` / `cancelled`；None = 全部。
    pub status: Option<String>,
    pub keyword: Option<String>,
    pub limit: u32,
    pub offset: u32,
}

/// 数据库句柄（单连接 + Mutex；所有写操作经此串行化，事务简单可靠）。
pub struct Db {
    conn: Mutex<Connection>,
    path: PathBuf,
}

impl std::fmt::Debug for Db {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Db").field("path", &self.path).finish()
    }
}

impl Db {
    /// 打开（或创建）数据库：WAL + busy_timeout + 迁移。
    pub fn open(path: &Path) -> BiliResult<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path).map_err(db_err)?;
        let db = Self {
            conn: Mutex::new(conn),
            path: path.to_path_buf(),
        };
        db.configure()?;
        db.migrate()?;
        Ok(db)
    }

    /// 内存数据库（测试用）。
    pub fn open_in_memory() -> BiliResult<Self> {
        let conn = Connection::open_in_memory().map_err(db_err)?;
        let db = Self {
            conn: Mutex::new(conn),
            path: PathBuf::from(":memory:"),
        };
        db.configure()?;
        db.migrate()?;
        Ok(db)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    fn configure(&self) -> BiliResult<()> {
        let conn = self.conn.lock().expect("db lock");
        conn.pragma_update(None, "journal_mode", "WAL")
            .map_err(db_err)?;
        conn.pragma_update(None, "busy_timeout", 5000)
            .map_err(db_err)?;
        conn.pragma_update(None, "synchronous", "NORMAL")
            .map_err(db_err)?;
        conn.pragma_update(None, "foreign_keys", "ON")
            .map_err(db_err)?;
        Ok(())
    }

    /// 增量迁移；可重复调用（幂等）。
    pub fn migrate(&self) -> BiliResult<i32> {
        let mut conn = self.conn.lock().expect("db lock");
        let mut version: i32 = conn
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .map_err(db_err)?;
        while version < SCHEMA_VERSION {
            let sql = MIGRATIONS[version as usize];
            let tx = conn.transaction().map_err(db_err)?;
            tx.execute_batch(sql).map_err(db_err)?;
            version += 1;
            tx.pragma_update(None, "user_version", version)
                .map_err(db_err)?;
            tx.commit().map_err(db_err)?;
        }
        Ok(version)
    }

    pub fn user_version(&self) -> BiliResult<i32> {
        let conn = self.conn.lock().expect("db lock");
        conn.pragma_query_value(None, "user_version", |row| row.get(0))
            .map_err(db_err)
    }

    // ---- 队列任务 ----

    pub fn upsert_task(&self, rec: &TaskRecord) -> BiliResult<()> {
        let conn = self.conn.lock().expect("db lock");
        conn.execute(
            "INSERT INTO tasks (id, status, phase, source_kind, source_id, title, cover_url, page_url,
                                params, progress, output_path, artifacts, error, restored, sort_index,
                                created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)
             ON CONFLICT(id) DO UPDATE SET
                status=excluded.status, phase=excluded.phase, source_kind=excluded.source_kind,
                source_id=excluded.source_id, title=excluded.title, cover_url=excluded.cover_url,
                page_url=excluded.page_url, params=excluded.params, progress=excluded.progress,
                output_path=excluded.output_path, artifacts=excluded.artifacts, error=excluded.error,
                restored=excluded.restored, sort_index=excluded.sort_index,
                updated_at=excluded.updated_at",
            params![
                rec.id, rec.status, rec.phase, rec.source_kind, rec.source_id, rec.title,
                rec.cover_url, rec.page_url, rec.params, rec.progress, rec.output_path,
                rec.artifacts, rec.error, rec.restored as i64, rec.sort_index, rec.created_at,
                rec.updated_at
            ],
        )
        .map_err(db_err)?;
        Ok(())
    }

    pub fn load_tasks(&self) -> BiliResult<Vec<TaskRecord>> {
        let conn = self.conn.lock().expect("db lock");
        let mut stmt = conn
            .prepare(
                "SELECT id, status, phase, source_kind, source_id, title, cover_url, page_url,
                        params, progress, output_path, artifacts, error, restored, sort_index,
                        created_at, updated_at
                 FROM tasks ORDER BY sort_index ASC, created_at ASC",
            )
            .map_err(db_err)?;
        let rows = stmt
            .query_map([], |row| {
                Ok(TaskRecord {
                    id: row.get(0)?,
                    status: row.get(1)?,
                    phase: row.get(2)?,
                    source_kind: row.get(3)?,
                    source_id: row.get(4)?,
                    title: row.get(5)?,
                    cover_url: row.get(6)?,
                    page_url: row.get(7)?,
                    params: row.get(8)?,
                    progress: row.get(9)?,
                    output_path: row.get(10)?,
                    artifacts: row.get(11)?,
                    error: row.get(12)?,
                    restored: row.get::<_, i64>(13)? != 0,
                    sort_index: row.get(14)?,
                    created_at: row.get(15)?,
                    updated_at: row.get(16)?,
                })
            })
            .map_err(db_err)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(db_err)
    }

    pub fn delete_task(&self, id: &str) -> BiliResult<()> {
        let conn = self.conn.lock().expect("db lock");
        conn.execute("DELETE FROM tasks WHERE id = ?1", params![id])
            .map_err(db_err)?;
        Ok(())
    }

    // ---- 历史 ----

    pub fn insert_history(&self, rec: &HistoryRecord) -> BiliResult<()> {
        let conn = self.conn.lock().expect("db lock");
        conn.execute(
            "INSERT INTO history (id, task_id, title, cover_url, source_kind, source_id, page_url,
                                  quality, output_path, file_size, artifacts, status, error, finished_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)
             ON CONFLICT(id) DO UPDATE SET status=excluded.status, error=excluded.error,
                output_path=excluded.output_path, file_size=excluded.file_size,
                artifacts=excluded.artifacts, finished_at=excluded.finished_at",
            params![
                rec.id, rec.task_id, rec.title, rec.cover_url, rec.source_kind, rec.source_id,
                rec.page_url, rec.quality, rec.output_path, rec.file_size, rec.artifacts, rec.status,
                rec.error, rec.finished_at
            ],
        )
        .map_err(db_err)?;
        Ok(())
    }

    pub fn list_history(&self, query: &HistoryQuery) -> BiliResult<Vec<HistoryRecord>> {
        let limit = if query.limit == 0 { 200 } else { query.limit };
        let conn = self.conn.lock().expect("db lock");
        let mut sql = String::from(
            "SELECT id, task_id, title, cover_url, source_kind, source_id, page_url, quality,
                    output_path, file_size, artifacts, status, error, finished_at
             FROM history WHERE 1=1",
        );
        let mut args: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
        if let Some(status) = &query.status {
            sql.push_str(" AND status = ?");
            args.push(Box::new(status.clone()));
        }
        if let Some(keyword) = &query.keyword
            && !keyword.trim().is_empty()
        {
            sql.push_str(" AND title LIKE ? ESCAPE '\\'");
            let escaped = keyword
                .replace('\\', "\\\\")
                .replace('%', "\\%")
                .replace('_', "\\_");
            args.push(Box::new(format!("%{escaped}%")));
        }
        sql.push_str(" ORDER BY finished_at DESC LIMIT ? OFFSET ?");
        args.push(Box::new(limit as i64));
        args.push(Box::new(query.offset as i64));
        let mut stmt = conn.prepare(&sql).map_err(db_err)?;
        let params_ref: Vec<&dyn rusqlite::ToSql> = args.iter().map(|b| b.as_ref()).collect();
        let rows = stmt
            .query_map(params_ref.as_slice(), |row| {
                Ok(HistoryRecord {
                    id: row.get(0)?,
                    task_id: row.get(1)?,
                    title: row.get(2)?,
                    cover_url: row.get(3)?,
                    source_kind: row.get(4)?,
                    source_id: row.get(5)?,
                    page_url: row.get(6)?,
                    quality: row.get(7)?,
                    output_path: row.get(8)?,
                    file_size: row.get(9)?,
                    artifacts: row.get(10)?,
                    status: row.get(11)?,
                    error: row.get(12)?,
                    finished_at: row.get(13)?,
                })
            })
            .map_err(db_err)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(db_err)
    }

    pub fn history_count(&self) -> BiliResult<u64> {
        let conn = self.conn.lock().expect("db lock");
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM history", [], |row| row.get(0))
            .map_err(db_err)?;
        Ok(n as u64)
    }

    /// 只保留最近 `keep` 条历史（按完成时间倒序），返回删除条数。
    pub fn prune_history(&self, keep: usize) -> BiliResult<usize> {
        let conn = self.conn.lock().expect("db lock");
        let deleted = conn
            .execute(
                "DELETE FROM history WHERE id NOT IN (
                    SELECT id FROM history ORDER BY finished_at DESC LIMIT ?1
                 )",
                params![keep as i64],
            )
            .map_err(db_err)?;
        Ok(deleted)
    }

    pub fn delete_history(&self, id: &str) -> BiliResult<()> {
        let conn = self.conn.lock().expect("db lock");
        conn.execute("DELETE FROM history WHERE id = ?1", params![id])
            .map_err(db_err)?;
        Ok(())
    }

    pub fn clear_history(&self) -> BiliResult<usize> {
        let conn = self.conn.lock().expect("db lock");
        let n = conn.execute("DELETE FROM history", []).map_err(db_err)?;
        Ok(n)
    }

    /// 历史单条（测试/校验用）。
    pub fn get_history(&self, id: &str) -> BiliResult<Option<HistoryRecord>> {
        let conn = self.conn.lock().expect("db lock");
        let rec = conn
            .query_row(
                "SELECT id, task_id, title, cover_url, source_kind, source_id, page_url, quality,
                        output_path, file_size, artifacts, status, error, finished_at
                 FROM history WHERE id = ?1",
                params![id],
                |row| {
                    Ok(HistoryRecord {
                        id: row.get(0)?,
                        task_id: row.get(1)?,
                        title: row.get(2)?,
                        cover_url: row.get(3)?,
                        source_kind: row.get(4)?,
                        source_id: row.get(5)?,
                        page_url: row.get(6)?,
                        quality: row.get(7)?,
                        output_path: row.get(8)?,
                        file_size: row.get(9)?,
                        artifacts: row.get(10)?,
                        status: row.get(11)?,
                        error: row.get(12)?,
                        finished_at: row.get(13)?,
                    })
                },
            )
            .optional()
            .map_err(db_err)?;
        Ok(rec)
    }
}

fn db_err(e: rusqlite::Error) -> BiliError {
    BiliError::Disk(format!("sqlite: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn task(id: &str, status: &str, phase: &str, updated: i64) -> TaskRecord {
        TaskRecord {
            id: id.into(),
            status: status.into(),
            phase: phase.into(),
            source_kind: "video".into(),
            source_id: "BV1xx".into(),
            title: format!("任务 {id}"),
            cover_url: None,
            page_url: Some("https://www.bilibili.com/video/BV1xx".into()),
            params: "{}".into(),
            progress: "{\"downloaded\":1,\"total\":2}".into(),
            output_path: None,
            artifacts: "[]".into(),
            error: None,
            restored: false,
            sort_index: 0,
            created_at: 1,
            updated_at: updated,
        }
    }

    #[test]
    fn migration_creates_schema_and_is_idempotent() {
        let db = Db::open_in_memory().unwrap();
        assert_eq!(db.user_version().unwrap(), SCHEMA_VERSION);
        db.migrate().unwrap();
        db.migrate().unwrap();
        assert_eq!(db.user_version().unwrap(), SCHEMA_VERSION);
        assert_eq!(db.load_tasks().unwrap().len(), 0);
        assert_eq!(db.history_count().unwrap(), 0);
    }

    #[test]
    fn incremental_migration_from_v1() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("queue.sqlite");
        {
            // 模拟旧版本：只有 001 与 user_version=1
            let conn = Connection::open(&path).unwrap();
            conn.execute_batch(MIGRATIONS[0]).unwrap();
            conn.pragma_update(None, "user_version", 1).unwrap();
        }
        let db = Db::open(&path).unwrap();
        assert_eq!(db.user_version().unwrap(), SCHEMA_VERSION);
        // 索引已建立（用 query 验证）
        db.upsert_task(&task("a", "queued", "queued", 1)).unwrap();
        let loaded = db.load_tasks().unwrap();
        assert_eq!(loaded.len(), 1);
    }

    #[test]
    fn incremental_migration_from_v2_preserves_legacy_outputs() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("queue.sqlite");
        {
            let conn = Connection::open(&path).unwrap();
            conn.execute_batch(MIGRATIONS[0]).unwrap();
            conn.execute_batch(MIGRATIONS[1]).unwrap();
            conn.pragma_update(None, "user_version", 2).unwrap();
            conn.execute(
                "INSERT INTO tasks (
                    id, status, phase, source_kind, source_id, title, params, progress,
                    output_path, created_at, updated_at
                 ) VALUES ('legacy-task', 'done', 'done', 'video', 'BV1', '旧任务', '{}', '{}',
                           '/tmp/legacy.mp4', 1, 1)",
                [],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO history (
                    id, task_id, title, source_kind, source_id, output_path, file_size,
                    status, finished_at
                 ) VALUES ('legacy-history', 'legacy-task', '旧任务', 'video', 'BV1',
                           '/tmp/legacy.mp4', 1024, 'done', 1)",
                [],
            )
            .unwrap();
        }

        let db = Db::open(&path).unwrap();
        assert_eq!(db.user_version().unwrap(), SCHEMA_VERSION);
        let tasks = db.load_tasks().unwrap();
        assert_eq!(tasks[0].artifacts, "[]");
        assert_eq!(tasks[0].output_path.as_deref(), Some("/tmp/legacy.mp4"));
        let history = db.list_history(&HistoryQuery::default()).unwrap();
        assert_eq!(history[0].artifacts, "[]");
        assert_eq!(history[0].file_size, Some(1024));
    }

    #[test]
    fn wal_mode_enabled() {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open(&dir.path().join("q.sqlite")).unwrap();
        let mode: String = {
            let conn = db.conn.lock().unwrap();
            conn.pragma_query_value(None, "journal_mode", |row| row.get(0))
                .unwrap()
        };
        assert_eq!(mode.to_lowercase(), "wal");
    }

    #[test]
    fn task_upsert_load_delete() {
        let db = Db::open_in_memory().unwrap();
        let mut a = task("a", "queued", "queued", 1);
        a.sort_index = 2;
        let mut b = task("b", "downloading", "downloading", 2);
        b.sort_index = 1;
        db.upsert_task(&a).unwrap();
        db.upsert_task(&b).unwrap();
        let loaded = db.load_tasks().unwrap();
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[0].id, "b", "按 sort_index 排序");
        a.progress = "{\"downloaded\":99}".into();
        a.status = "failed".into();
        db.upsert_task(&a).unwrap();
        let loaded = db.load_tasks().unwrap();
        let a2 = loaded.iter().find(|r| r.id == "a").unwrap();
        assert_eq!(a2.progress, "{\"downloaded\":99}");
        assert_eq!(a2.status, "failed");
        assert_eq!(a2.artifacts, "[]");
        db.delete_task("a").unwrap();
        assert_eq!(db.load_tasks().unwrap().len(), 1);
    }

    fn hist(id: &str, status: &str, title: &str, finished: i64) -> HistoryRecord {
        HistoryRecord {
            id: id.into(),
            task_id: Some("t".into()),
            title: title.into(),
            cover_url: None,
            source_kind: "video".into(),
            source_id: "BV1".into(),
            page_url: Some("https://www.bilibili.com/video/BV1".into()),
            quality: Some("1080P".into()),
            output_path: Some(format!("/tmp/{id}.mp4")),
            file_size: Some(1024),
            artifacts: "[]".into(),
            status: status.into(),
            error: None,
            finished_at: finished,
        }
    }

    #[test]
    fn history_crud_filter_search_prune() {
        let db = Db::open_in_memory().unwrap();
        for i in 0..5 {
            db.insert_history(&hist(
                &format!("h{i}"),
                "done",
                &format!("视频 {i}"),
                i as i64,
            ))
            .unwrap();
        }
        db.insert_history(&hist("f1", "failed", "失败任务", 10))
            .unwrap();
        assert_eq!(db.list_history(&HistoryQuery::default()).unwrap().len(), 6);
        // 状态过滤
        let failed = db
            .list_history(&HistoryQuery {
                status: Some("failed".into()),
                limit: 10,
                ..Default::default()
            })
            .unwrap();
        assert_eq!(failed.len(), 1);
        assert_eq!(failed[0].title, "失败任务");
        // 搜索（LIKE 转义）
        let found = db
            .list_history(&HistoryQuery {
                keyword: Some("视频 3".into()),
                limit: 10,
                ..Default::default()
            })
            .unwrap();
        assert_eq!(found.len(), 1);
        let none = db
            .list_history(&HistoryQuery {
                keyword: Some("100%".into()),
                limit: 10,
                ..Default::default()
            })
            .unwrap();
        assert!(none.is_empty());
        // 排序（倒序）
        let all = db
            .list_history(&HistoryQuery {
                limit: 10,
                ..Default::default()
            })
            .unwrap();
        assert_eq!(all[0].id, "f1");
        // 裁剪
        let deleted = db.prune_history(3).unwrap();
        assert_eq!(deleted, 3);
        assert_eq!(db.history_count().unwrap(), 3);
        // 单条 + 删除
        assert!(db.get_history("f1").unwrap().is_some());
        db.delete_history("f1").unwrap();
        assert!(db.get_history("f1").unwrap().is_none());
        db.clear_history().unwrap();
        assert_eq!(db.history_count().unwrap(), 0);
    }

    #[test]
    fn concurrent_writes_are_safe() {
        let dir = tempfile::tempdir().unwrap();
        let db = std::sync::Arc::new(Db::open(&dir.path().join("q.sqlite")).unwrap());
        std::thread::scope(|scope| {
            for t in 0..8 {
                let db = std::sync::Arc::clone(&db);
                scope.spawn(move || {
                    for i in 0..25 {
                        db.upsert_task(&task(&format!("t{t}-{i}"), "queued", "queued", i))
                            .unwrap();
                    }
                });
            }
        });
        assert_eq!(db.load_tasks().unwrap().len(), 200);
    }
}
