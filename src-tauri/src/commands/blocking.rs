//! Execute synchronous SQL, parsing and filesystem work off the UI/runtime worker.
//!
//! Callers register their VaultOperation before dispatch and move it into the
//! closure. Queued work therefore retains its original session and must check
//! revocation before touching data, rather than acquiring a subsequent unlock.
use crate::error::{AppError, AppResult};
use crate::security::session::VaultOperation;

pub(crate) async fn run<T: Send + 'static>(
    work: impl FnOnce() -> T + Send + 'static,
) -> AppResult<T> {
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|error| AppError::System(format!("后台任务未能完成: {error}")))
}

pub(crate) async fn with_operation<T: Send + 'static>(
    parent: &VaultOperation,
    work: impl FnOnce(&VaultOperation) -> AppResult<T> + Send + 'static,
) -> AppResult<T> {
    with_tracked(parent, work).await?
}

pub(crate) async fn with_tracked<T: Send + 'static>(
    parent: &VaultOperation,
    work: impl FnOnce(&VaultOperation) -> T + Send + 'static,
) -> AppResult<T> {
    let operation = parent.fork()?;
    run(move || {
        operation.check()?;
        let result = work(&operation);
        operation.check()?;
        Ok(result)
    })
    .await?
}
