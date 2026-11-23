//! Revocable database ownership and tracked sensitive work for one unlock.
use rusqlite::Connection;
use std::collections::HashMap;
use std::future::{poll_fn, Future};
use std::ops::{Deref, DerefMut};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::task::{Poll, Waker};

use crate::error::{AppError, AppResult};

fn revoked() -> AppError {
    AppError::VaultLocked("保险库会话已结束，请重新解锁后重试".into())
}

fn attach_cancellation_hook(
    conn: &mut Connection,
    cancelled: &Arc<AtomicBool>,
    revocation: &Arc<AtomicU64>,
) -> AppResult<()> {
    let query_cancelled = cancelled.clone();
    let platform_cancelled = revocation.clone();
    // A held connection can be executing a large query when revoke runs.
    // Check the same session flag inside SQLite, without taking our mutexes
    // or retaining the session through a reference cycle.
    conn.progress_handler(
        1000,
        Some(move || {
            query_cancelled.load(Ordering::Acquire)
                || platform_cancelled.load(Ordering::Acquire) & 1 != 0
        }),
    )?;
    Ok(())
}

pub(super) struct VaultSession {
    cancelled: Arc<AtomicBool>,
    revocation: Arc<AtomicU64>,
    connection: Mutex<Option<Connection>>,
    read_pool: Vec<Arc<Mutex<Option<Connection>>>>,
    next_reader: AtomicUsize,
    operations: Mutex<HashMap<u64, Arc<OperationSignal>>>,
    next_id: AtomicU64,
    idle: Condvar,
}

#[derive(Default)]
struct OperationSignal {
    wakers: Mutex<Vec<Waker>>,
}

impl VaultSession {
    #[cfg(test)]
    pub(super) fn new(connection: Connection) -> AppResult<Arc<Self>> {
        Self::with_revocation_and_readers(connection, Vec::new(), Arc::new(AtomicU64::new(0)))
    }

    pub(super) fn with_revocation_and_readers(
        mut connection: Connection,
        readers: Vec<Connection>,
        revocation: Arc<AtomicU64>,
    ) -> AppResult<Arc<Self>> {
        let cancelled = Arc::new(AtomicBool::new(false));
        attach_cancellation_hook(&mut connection, &cancelled, &revocation)?;
        let mut read_pool = Vec::with_capacity(readers.len());
        for mut reader in readers {
            attach_cancellation_hook(&mut reader, &cancelled, &revocation)?;
            read_pool.push(Arc::new(Mutex::new(Some(reader))));
        }
        Ok(Arc::new(Self {
            cancelled,
            revocation,
            connection: Mutex::new(Some(connection)),
            read_pool,
            next_reader: AtomicUsize::new(0),
            operations: Mutex::new(HashMap::new()),
            next_id: AtomicU64::new(1),
            idle: Condvar::new(),
        }))
    }

    fn check(&self) -> AppResult<()> {
        if self.cancelled.load(Ordering::Acquire)
            || self.revocation.load(Ordering::Acquire) & 1 != 0
        {
            Err(revoked())
        } else {
            Ok(())
        }
    }

    pub(super) fn handle(self: &Arc<Self>) -> AppResult<ConnectionHandle> {
        self.check()?;
        Ok(ConnectionHandle {
            session: self.clone(),
        })
    }

    pub(super) fn read_handle(self: &Arc<Self>) -> AppResult<ReadConnectionHandle> {
        self.check()?;
        Ok(ReadConnectionHandle {
            session: self.clone(),
        })
    }

    pub(super) fn operation(self: &Arc<Self>) -> AppResult<VaultOperation> {
        let mut operations = self
            .operations
            .lock()
            .map_err(|_| AppError::System("任务注册锁不可用".into()))?;
        self.check()?;
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let signal = Arc::new(OperationSignal::default());
        operations.insert(id, signal.clone());
        Ok(VaultOperation {
            session: self.clone(),
            id,
            signal,
        })
    }

    pub(super) fn revoke(&self) {
        // Revoke before waiting for any connection lock. Handles already handed
        // out cannot acquire the connection after this point.
        self.cancelled.store(true, Ordering::Release);
        let signals = self
            .operations
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .values()
            .cloned()
            .collect::<Vec<_>>();
        for signal in signals {
            let wakers =
                std::mem::take(&mut *signal.wakers.lock().unwrap_or_else(|e| e.into_inner()));
            for waker in wakers {
                waker.wake();
            }
        }
    }

    pub(super) fn close(&self) -> AppResult<()> {
        self.revoke();
        let mut operations = self.operations.lock().unwrap_or_else(|e| e.into_inner());
        while !operations.is_empty() {
            operations = self
                .idle
                .wait(operations)
                .unwrap_or_else(|e| e.into_inner());
        }
        drop(operations);
        // Close every read-only WAL handle first so no snapshot lock can block
        // the primary connection's final wal_checkpoint(TRUNCATE).
        for reader_slot in &self.read_pool {
            let mut slot = reader_slot.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(mut reader) = slot.take() {
                let _ = reader.progress_handler(0, None::<fn() -> bool>);
                reader
                    .close()
                    .map_err(|(_, error)| AppError::Database(error))?;
            }
        }
        let mut slot = self.connection.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(mut conn) = slot.take() {
            // Revocation has drained all business work. Final WAL maintenance
            // must be allowed to finish even though this session is cancelled.
            conn.progress_handler(0, None::<fn() -> bool>)?;
            // Existing writes finish or roll back before this guard is available.
            let checkpoint = conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |r| {
                r.get::<_, i64>(0)
            });
            let close = conn.close().map_err(|(_, error)| AppError::Database(error));
            close?;
            if checkpoint? != 0 {
                return Err(AppError::Security("连接已关闭，但 WAL 未能完整合并".into()));
            }
        }
        Ok(())
    }
}

/// A handle retains the session identity, never an independently usable Connection.
#[derive(Clone)]
pub struct ConnectionHandle {
    session: Arc<VaultSession>,
}

pub struct ConnectionGuard<'a> {
    guard: MutexGuard<'a, Option<Connection>>,
}

impl ConnectionHandle {
    /// UI event callbacks must never wait behind a long database operation.
    pub fn try_lock(&self) -> AppResult<Option<ConnectionGuard<'_>>> {
        self.session.check()?;
        let guard = match self.session.connection.try_lock() {
            Ok(guard) => guard,
            Err(std::sync::TryLockError::WouldBlock) => return Ok(None),
            Err(std::sync::TryLockError::Poisoned(_)) => {
                return Err(AppError::System("数据库连接锁不可用".into()));
            }
        };
        self.session.check()?;
        if guard.is_none() {
            return Err(revoked());
        }
        Ok(Some(ConnectionGuard { guard }))
    }
    pub fn lock(&self) -> AppResult<ConnectionGuard<'_>> {
        self.session.check()?;
        let guard = self
            .session
            .connection
            .lock()
            .map_err(|_| AppError::System("数据库连接锁不可用".into()))?;
        self.session.check()?;
        if guard.is_none() {
            return Err(revoked());
        }
        Ok(ConnectionGuard { guard })
    }
}

impl Deref for ConnectionGuard<'_> {
    type Target = Connection;
    fn deref(&self) -> &Connection {
        self.guard.as_ref().expect("connection guarded until drop")
    }
}
impl DerefMut for ConnectionGuard<'_> {
    fn deref_mut(&mut self) -> &mut Connection {
        self.guard.as_mut().expect("connection guarded until drop")
    }
}

/// Concurrent read-only handle backed by the session's WAL reader pool.
#[derive(Clone)]
pub struct ReadConnectionHandle {
    session: Arc<VaultSession>,
}

pub struct ReadConnectionGuard<'a> {
    guard: MutexGuard<'a, Option<Connection>>,
}

impl ReadConnectionHandle {
    pub fn lock(&self) -> AppResult<ReadConnectionGuard<'_>> {
        self.session.check()?;
        let guard = if self.session.read_pool.is_empty() {
            self.session
                .connection
                .lock()
                .map_err(|_| AppError::System("数据库连接锁不可用".into()))?
        } else {
            let len = self.session.read_pool.len();
            let start = self.session.next_reader.fetch_add(1, Ordering::Relaxed) % len;
            let mut acquired = None;
            for offset in 0..len {
                match self.session.read_pool[(start + offset) % len].try_lock() {
                    Ok(g) => {
                        acquired = Some(g);
                        break;
                    }
                    Err(std::sync::TryLockError::WouldBlock) => continue,
                    Err(std::sync::TryLockError::Poisoned(_)) => {
                        return Err(AppError::System("只读数据库连接锁不可用".into()));
                    }
                }
            }
            match acquired {
                Some(g) => g,
                None => self.session.read_pool[start]
                    .lock()
                    .map_err(|_| AppError::System("只读数据库连接锁不可用".into()))?,
            }
        };
        self.session.check()?;
        if guard.is_none() {
            return Err(revoked());
        }
        Ok(ReadConnectionGuard { guard })
    }
}

impl Deref for ReadConnectionGuard<'_> {
    type Target = Connection;
    fn deref(&self) -> &Connection {
        self.guard
            .as_ref()
            .expect("read connection guarded until drop")
    }
}

/// RAII registration for an import, export, AI request, or other multi-step job.
/// Lock waits until its working buffers and snapshots have been dropped.
pub struct VaultOperation {
    session: Arc<VaultSession>,
    id: u64,
    signal: Arc<OperationSignal>,
}

impl VaultOperation {
    /// Register a child before dispatching blocking preparation. It keeps the
    /// same unlock identity and stays tracked even if its awaiting future drops.
    pub(crate) fn fork(&self) -> AppResult<Self> {
        self.session.operation()
    }
    /// A read-only cancellation probe for synchronous preparation/progress hooks.
    /// It retains session identity without registering new work or exposing a DB.
    pub fn cancellation_probe(&self) -> Arc<dyn Fn() -> bool + Send + Sync> {
        let session = self.session.clone();
        Arc::new(move || {
            session.cancelled.load(Ordering::Acquire)
                || session.revocation.load(Ordering::Acquire) & 1 != 0
        })
    }
    pub fn check(&self) -> AppResult<()> {
        self.session.check()
    }
    pub fn connection(&self) -> AppResult<ConnectionHandle> {
        self.session.handle()
    }
    pub fn read_connection(&self) -> AppResult<ReadConnectionHandle> {
        self.session.read_handle()
    }
    pub fn with_conn<F, R>(&self, f: F) -> AppResult<R>
    where
        F: FnOnce(&Connection) -> AppResult<R>,
    {
        let handle = self.connection()?;
        let conn = handle.lock()?;
        let result = f(&conn)?;
        self.check()?;
        Ok(result)
    }
    pub fn with_read_conn<F, R>(&self, f: F) -> AppResult<R>
    where
        F: FnOnce(&Connection) -> AppResult<R>,
    {
        let handle = self.read_connection()?;
        let conn = handle.lock()?;
        let result = f(&conn)?;
        self.check()?;
        Ok(result)
    }
    pub fn with_conn_mut<F, R>(&self, f: F) -> AppResult<R>
    where
        F: FnOnce(&mut Connection) -> AppResult<R>,
    {
        let handle = self.connection()?;
        let mut conn = handle.lock()?;
        let result = f(&mut conn)?;
        self.check()?;
        Ok(result)
    }
    pub(super) fn belongs_to(&self, session: &Arc<VaultSession>) -> bool {
        Arc::ptr_eq(&self.session, session)
    }

    /// Drop the pending HTTP/future work promptly when the session is revoked.
    pub async fn run<F: Future>(&self, future: F) -> AppResult<F::Output> {
        let mut future = Box::pin(future);
        poll_fn(|cx| {
            {
                let mut wakers = self.signal.wakers.lock().unwrap_or_else(|e| e.into_inner());
                if !wakers.iter().any(|waker| waker.will_wake(cx.waker())) {
                    wakers.push(cx.waker().clone());
                }
            }
            if let Err(error) = self.check() {
                return Poll::Ready(Err(error));
            }
            match future.as_mut().poll(cx) {
                Poll::Ready(output) => Poll::Ready(self.check().map(|_| output)),
                Poll::Pending => Poll::Pending,
            }
        })
        .await
    }
}

impl Drop for VaultOperation {
    fn drop(&mut self) {
        self.session
            .operations
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&self.id);
        self.session.idle.notify_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::time::Duration;

    #[test]
    fn ui_connection_probe_never_waits_and_rejects_revocation() {
        let session = VaultSession::new(Connection::open_in_memory().unwrap()).unwrap();
        let handle = session.handle().unwrap();
        let held = handle.lock().unwrap();
        assert!(handle.try_lock().unwrap().is_none());
        drop(held);
        assert!(handle.try_lock().unwrap().is_some());
        session.revoke();
        assert!(handle.try_lock().is_err());
        session.close().unwrap();
    }

    #[test]
    fn locking_interrupts_live_sql_and_rolls_back_uncommitted_writes() {
        use crate::security::vault::{VaultManager, VaultStatus};
        use rusqlite::functions::FunctionFlags;
        for write in [false, true] {
            let root =
                std::env::temp_dir().join(format!("browsory_sql_cancel_{}", uuid::Uuid::new_v4()));
            std::fs::create_dir(&root).unwrap();
            let vault = Arc::new(VaultManager::unlocked_fixture(root.clone()));
            let (ready_tx, ready_rx) = mpsc::channel();
            vault.with_conn(|conn| {
                conn.execute_batch("CREATE TABLE cancellation_rows(id INTEGER PRIMARY KEY);INSERT INTO cancellation_rows VALUES(-1);")?;
                conn.create_scalar_function("query_started",0,FunctionFlags::SQLITE_UTF8,move |_|{
                    ready_tx.send(()).unwrap();
                    Ok(0i64)
                })?;
                Ok(())
            }).unwrap();
            let operation = vault.begin_operation().unwrap();
            let worker = std::thread::spawn(move || {
                let handle = operation.connection().unwrap();
                let conn = handle.lock().unwrap();
                let result = if write {
                    conn.execute_batch("BEGIN;INSERT INTO cancellation_rows WITH RECURSIVE x(n) AS (VALUES(query_started()) UNION ALL SELECT n+1 FROM x WHERE n<1000000000) SELECT n FROM x;COMMIT;")
                } else {
                    conn.query_row("WITH RECURSIVE x(n) AS (VALUES(query_started()) UNION ALL SELECT n+1 FROM x WHERE n<1000000000) SELECT sum(n) FROM x",[],|r|r.get::<_,i64>(0)).map(|_|())
                };
                // Dropping the guard and tracked operation lets lock checkpoint
                // and close; SQLite's interrupted statement cannot commit.
                drop(conn);
                drop(operation);
                result
            });
            ready_rx.recv_timeout(Duration::from_secs(5)).unwrap();
            let (done_tx, done_rx) = mpsc::channel();
            let closing = vault.clone();
            let closer = std::thread::spawn(move || done_tx.send(closing.lock()).unwrap());
            done_rx
                .recv_timeout(Duration::from_secs(5))
                .unwrap()
                .unwrap();
            closer.join().unwrap();
            let error = worker.join().unwrap().unwrap_err();
            assert_eq!(
                error.sqlite_error_code(),
                Some(rusqlite::ErrorCode::OperationInterrupted)
            );
            assert_eq!(vault.status(), VaultStatus::Locked);
            assert!(vault.connection().is_err());
            vault
                .unlock_with_password("Synthetic-fixture-password")
                .unwrap();
            let ids = vault
                .with_conn(|c| {
                    Ok(c.prepare("SELECT id FROM cancellation_rows ORDER BY id")?
                        .query_map([], |r| r.get::<_, i64>(0))?
                        .collect::<rusqlite::Result<Vec<_>>>()?)
                })
                .unwrap();
            assert_eq!(ids, vec![-1]);
            vault.lock().unwrap();
            drop(vault);
            std::fs::remove_dir_all(root).unwrap();
        }
    }

    #[test]
    fn retained_and_queued_handles_cannot_reopen_closed_session() {
        let session = VaultSession::new(Connection::open_in_memory().unwrap()).unwrap();
        let old_handle = session.handle().unwrap();
        let held = old_handle.lock().unwrap();
        let (done_tx, done_rx) = mpsc::channel();
        let closing = session.clone();
        let thread = std::thread::spawn(move || {
            closing.close().unwrap();
            done_tx.send(()).unwrap();
        });
        while session.check().is_ok() {
            std::thread::yield_now();
        }
        assert!(old_handle.lock().is_err());
        assert!(
            done_rx.try_recv().is_err(),
            "close must wait for the active DB operation"
        );
        drop(held);
        done_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        thread.join().unwrap();
        assert!(old_handle.lock().is_err());
        assert!(session.connection.lock().unwrap().is_none());
        let next = VaultSession::new(Connection::open_in_memory().unwrap()).unwrap();
        assert!(next.handle().unwrap().lock().is_ok());
        assert!(
            old_handle.lock().is_err(),
            "unlocking another session cannot revive a stale handle"
        );
    }

    #[test]
    fn lock_cancels_pending_async_work_and_drains_its_registration() {
        let session = VaultSession::new(Connection::open_in_memory().unwrap()).unwrap();
        let operation = session.operation().unwrap();
        let (ready_tx, ready_rx) = mpsc::channel();
        let worker = std::thread::spawn(move || {
            tauri::async_runtime::block_on(async move {
                let mut ready = Some(ready_tx);
                let pending = poll_fn(move |_| {
                    if let Some(tx) = ready.take() {
                        tx.send(()).unwrap();
                    }
                    Poll::<()>::Pending
                });
                assert!(operation.run(pending).await.is_err());
            });
        });
        ready_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        session.close().unwrap();
        worker.join().unwrap();
        assert!(session.operation().is_err());
        assert!(session.connection.lock().unwrap().is_none());
    }

    #[test]
    fn all_futures_sharing_an_operation_are_woken_on_revocation() {
        let session = VaultSession::new(Connection::open_in_memory().unwrap()).unwrap();
        let operation = Arc::new(session.operation().unwrap());
        let (ready_tx, ready_rx) = mpsc::channel();
        let mut workers = Vec::new();
        for _ in 0..2 {
            let operation = operation.clone();
            let ready_tx = ready_tx.clone();
            workers.push(std::thread::spawn(move || {
                tauri::async_runtime::block_on(async move {
                    let mut ready = Some(ready_tx);
                    let pending = poll_fn(move |_| {
                        if let Some(tx) = ready.take() {
                            tx.send(()).unwrap();
                        }
                        Poll::<()>::Pending
                    });
                    assert!(operation.run(pending).await.is_err());
                });
            }));
        }
        drop(operation);
        ready_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        ready_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        session.close().unwrap();
        for worker in workers {
            worker.join().unwrap();
        }
    }

    #[test]
    fn read_pool_runs_while_primary_write_lock_is_held_and_revokes_cleanly() {
        use crate::security::vault::VaultManager;
        use std::sync::mpsc;
        let root =
            std::env::temp_dir().join(format!("browsory_read_pool_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        let vault = Arc::new(VaultManager::unlocked_fixture(root.clone()));

        // Hold the primary write connection exclusively; reads on the pool must
        // complete immediately without waiting behind the write lock.
        let write_op = vault.begin_operation().unwrap();
        let write_handle = write_op.connection().unwrap();
        let _write_guard = write_handle.lock().unwrap();

        let (tx, rx) = mpsc::channel();
        let v_reader = vault.clone();
        std::thread::spawn(move || {
            let count = v_reader
                .with_read_conn(|conn| {
                    conn.query_row("SELECT count(*) FROM sources", [], |r| r.get::<_, i64>(0))
                        .map_err(Into::into)
                })
                .unwrap();
            tx.send(count).unwrap();
        });
        assert_eq!(rx.recv_timeout(Duration::from_secs(2)).unwrap(), 0);
        drop(_write_guard);
        drop(write_op);

        // Lock must also cancel live queries on a read connection and close cleanly.
        let (started_tx, started_rx) = mpsc::channel();
        let op = vault.begin_operation().unwrap();
        let worker = std::thread::spawn(move || {
            let res = op.with_read_conn(|conn| {
                started_tx.send(()).unwrap();
                conn.query_row(
                    "WITH RECURSIVE x(n) AS (VALUES(1) UNION ALL SELECT n+1 FROM x WHERE n<1000000000) SELECT sum(n) FROM x",
                    [],
                    |r| r.get::<_, i64>(0),
                )
                .map(|_| ())
                .map_err(Into::into)
            });
            drop(op);
            res
        });
        started_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        vault.lock().unwrap();
        let err = worker.join().unwrap().unwrap_err();
        assert!(
            matches!(
                err,
                AppError::Database(ref db_err)
                    if db_err.sqlite_error_code() == Some(rusqlite::ErrorCode::OperationInterrupted)
            ),
            "expected SQLite OperationInterrupted on read pool revocation, got {err:?}"
        );
        drop(vault);
        std::fs::remove_dir_all(root).unwrap();
    }
}
