//! Initializes writable connections while preserving existing database settings.
//! Only empty databases adopt the requested journal mode; DELETE connections use EXTRA sync.
//! Managed DELETE requirements also reject other modes on every connection.

#![expect(
    clippy::disallowed_methods,
    reason = "centralized SQLite connection initialization"
)]

use sqlx::Error;
use sqlx::SqlitePool;
use sqlx::sqlite::SqliteConnectOptions;
use sqlx::sqlite::SqlitePoolOptions;
#[cfg(feature = "lodex-low-memory")]
use std::time::Duration;

#[cfg(feature = "lodex-low-memory")]
const MAX_CONNECTIONS: u32 = 2;
#[cfg(not(feature = "lodex-low-memory"))]
const MAX_CONNECTIONS: u32 = 5;

pub(super) fn connection_options(options: SqliteConnectOptions) -> SqliteConnectOptions {
    #[cfg(feature = "lodex-low-memory")]
    let options = options
        .statement_cache_capacity(32)
        // A negative value is KiB, independent of the database's page size.
        // This is a per-connection suggestion, not a process-wide memory limit.
        .pragma("cache_size", "-512");
    options
}

pub(super) fn pool_options(max_connections: u32) -> SqlitePoolOptions {
    let options = SqlitePoolOptions::new().max_connections(max_connections);
    #[cfg(feature = "lodex-low-memory")]
    let options = options
        // Release workers, prepared statements and page caches for unused DBs.
        // The same initialization runs when a later request opens a connection.
        .idle_timeout(Some(Duration::from_secs(30)));
    options
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub(super) enum JournalModePolicy {
    InitializeWal,
    InitializeDelete,
    RequireDelete,
}

pub(super) async fn connect_once(
    options: &SqliteConnectOptions,
    journal_mode_policy: JournalModePolicy,
) -> Result<SqlitePool, Error> {
    // SQLx retries after_connect errors, eventually replacing them with PoolTimedOut.
    // Return the first initialization error directly while opening this pool.
    let (init_error_tx, mut init_error_rx) = tokio::sync::mpsc::channel(/*buffer*/ 1);
    let pool_options = pool_options(MAX_CONNECTIONS)
        // Keep an independent reader/writer available; a one-connection pool
        // would serialize both and deadlock callers holding a connection.
        .after_connect(move |connection, _metadata| {
            let init_error_tx = init_error_tx.clone();
            Box::pin(async move {
                let result = async {
                    let journal: String = sqlx::query_scalar("PRAGMA journal_mode")
                        .fetch_one(&mut *connection)
                        .await?;
                    if journal_mode_policy == JournalModePolicy::RequireDelete
                        && journal != "delete"
                    {
                        return Err(Error::Protocol(format!(
                            "NFS database requires DELETE journaling, found {journal}"
                        )));
                    }
                    if journal == "delete" {
                        // If the filesystem honors sync requests, EXTRA syncs the directory
                        // after SQLite deletes a rollback journal.
                        sqlx::query("PRAGMA synchronous = EXTRA")
                            .execute(connection)
                            .await?;
                    }
                    Ok(())
                }
                .await;
                result.map_err(|error| match init_error_tx.try_send(error) {
                    // The opener owns the original error and cancels connection retries.
                    Ok(()) => Error::PoolClosed,
                    // After opening, lazy connections retain SQLx's normal error handling.
                    Err(error) => error.into_inner(),
                })
            })
        });
    let pool = tokio::select! {
        biased;
        Some(error) = init_error_rx.recv() => Err(error),
        result = pool_options.connect_with(connection_options(options.clone())) => result,
    }?;
    // Database settings persist on disk.
    // Initialize these settings once before callers can use the pool.
    // Connections that open later need only settings that apply to each connection.
    let initialized: Result<(), Error> = async {
        let mut connection = pool.acquire().await?;
        // Auto-vacuum alone can write a header to disk.
        // If creation stops before the schema exists, startup must initialize the file.
        let empty =
            sqlx::query_scalar::<_, bool>("SELECT NOT EXISTS (SELECT 1 FROM sqlite_schema)")
                .fetch_one(&mut *connection)
                .await?;
        if empty {
            let mode: i64 = sqlx::query_scalar("PRAGMA auto_vacuum")
                .fetch_one(&mut *connection)
                .await?;
            if mode == 0 {
                sqlx::query("PRAGMA auto_vacuum = INCREMENTAL")
                    .execute(&mut *connection)
                    .await?;
            }
            let pragma = match journal_mode_policy {
                JournalModePolicy::InitializeWal => "PRAGMA journal_mode = WAL",
                JournalModePolicy::InitializeDelete | JournalModePolicy::RequireDelete => {
                    "PRAGMA journal_mode = DELETE"
                }
            };
            sqlx::query(pragma).execute(&mut *connection).await?;
        }
        // Initialization or another process can change the journal mode after after_connect.
        // Match this connection's synchronization setting to the resulting journal mode.
        let journal: String = sqlx::query_scalar("PRAGMA journal_mode")
            .fetch_one(&mut *connection)
            .await?;
        if journal == "wal" {
            sqlx::query("PRAGMA synchronous = NORMAL")
                .execute(&mut *connection)
                .await?;
        } else if journal == "delete" {
            sqlx::query("PRAGMA synchronous = EXTRA")
                .execute(&mut *connection)
                .await?;
        }
        // Wait for the connection to return to the pool before startup requests it again.
        // An asynchronous drop can otherwise cause the pool to open a second connection.
        connection.return_to_pool().await;
        Ok(())
    }
    .await;
    if let Err(error) = initialized {
        pool.close().await;
        return Err(error);
    }
    Ok(pool)
}
