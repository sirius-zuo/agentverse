//! Retention/maintenance methods for `PostgresSessionMemory`.
//!
//! These are implemented as free functions (rather than a second
//! `impl SessionMemory for PostgresSessionMemory` block) because Rust does
//! not allow splitting a single trait implementation across multiple `impl`
//! blocks — only inherent `impl Type { .. }` blocks can be split that way.
//! The trait methods in `session_store.rs` delegate to these functions.

use crate::session_store::PostgresSessionMemory;
use agentverse::memory::Message;
use agentverse_memory::session::{Session, SessionId, SessionMemoryError, SessionStatus};

pub(crate) async fn get_watermark(
    store: &PostgresSessionMemory,
    session_id: SessionId,
) -> Result<i64, SessionMemoryError> {
    let wm: i64 = sqlx::query_scalar("SELECT consolidation_watermark FROM sessions WHERE id = $1")
        .bind(session_id.to_string())
        .fetch_one(&store.pool)
        .await
        .map_err(|e| SessionMemoryError::Database(e.to_string()))?;
    Ok(wm)
}

pub(crate) async fn advance_watermark(
    store: &PostgresSessionMemory,
    session_id: SessionId,
    new_watermark: i64,
) -> Result<(), SessionMemoryError> {
    let result = sqlx::query(
        "UPDATE sessions \
         SET consolidation_watermark = GREATEST(consolidation_watermark, $1) \
         WHERE id = $2",
    )
    .bind(new_watermark)
    .bind(session_id.to_string())
    .execute(&store.pool)
    .await
    .map_err(|e| SessionMemoryError::Database(e.to_string()))?;
    if result.rows_affected() == 0 {
        return Err(SessionMemoryError::NotFound(session_id));
    }
    Ok(())
}

pub(crate) async fn load_messages_above_watermark(
    store: &PostgresSessionMemory,
    session_id: SessionId,
) -> Result<Vec<(i64, Message)>, SessionMemoryError> {
    let wm = get_watermark(store, session_id).await?;
    let rows = sqlx::query_as::<_, (i64, String, String)>(
        "SELECT sequence_num, role, content \
         FROM messages \
         WHERE session_id = $1 AND sequence_num > $2 \
         ORDER BY sequence_num ASC",
    )
    .bind(session_id.to_string())
    .bind(wm)
    .fetch_all(&store.pool)
    .await
    .map_err(|e| SessionMemoryError::Database(e.to_string()))?;
    rows.into_iter()
        .map(|(seq, role, content)| {
            Ok((
                seq,
                Message {
                    role: PostgresSessionMemory::str_to_role(&role)?,
                    content: crate::session_store::decode_content(&content),
                },
            ))
        })
        .collect::<Result<Vec<_>, SessionMemoryError>>()
}

pub(crate) async fn cleanup_expired_messages(
    store: &PostgresSessionMemory,
    session_id: SessionId,
    cutoff_ts: i64,
    watermark: i64,
) -> Result<u64, SessionMemoryError> {
    if watermark == 0 {
        return Ok(0);
    }
    // Cap at stored watermark to protect unconsolidated messages
    let stored_wm = get_watermark(store, session_id).await?;
    let effective_watermark = watermark.min(stored_wm);
    if effective_watermark == 0 {
        return Ok(0);
    }
    let result = sqlx::query(
        "DELETE FROM messages \
         WHERE session_id = $1 AND created_at < $2 AND sequence_num <= $3",
    )
    .bind(session_id.to_string())
    .bind(cutoff_ts)
    .bind(effective_watermark)
    .execute(&store.pool)
    .await
    .map_err(|e| SessionMemoryError::Database(e.to_string()))?;
    Ok(result.rows_affected())
}

pub(crate) async fn delete_ended_sessions_before(
    store: &PostgresSessionMemory,
    cutoff_ts: i64,
) -> Result<u64, SessionMemoryError> {
    let result = sqlx::query("DELETE FROM sessions WHERE status != 'active' AND updated_at < $1")
        .bind(cutoff_ts)
        .execute(&store.pool)
        .await
        .map_err(|e| SessionMemoryError::Database(e.to_string()))?;
    Ok(result.rows_affected())
}

pub(crate) async fn delete_session(
    store: &PostgresSessionMemory,
    session_id: SessionId,
) -> Result<(), SessionMemoryError> {
    sqlx::query("DELETE FROM sessions WHERE id = $1")
        .bind(session_id.to_string())
        .execute(&store.pool)
        .await
        .map_err(|e| SessionMemoryError::Database(e.to_string()))?;
    Ok(())
}

pub(crate) async fn list_sessions_needing_maintenance(
    store: &PostgresSessionMemory,
) -> Result<Vec<Session>, SessionMemoryError> {
    let rows = sqlx::query_as::<_, (String, String, String, i64, i64)>(
        "SELECT DISTINCT s.id, s.user_id, s.status, s.created_at, s.updated_at \
         FROM sessions s \
         JOIN messages m ON m.session_id = s.id \
         WHERE m.sequence_num > s.consolidation_watermark \
            OR (m.sequence_num <= s.consolidation_watermark AND m.created_at < $1) \
         ORDER BY s.updated_at ASC",
    )
    .bind(chrono::Utc::now().timestamp())
    .fetch_all(&store.pool)
    .await
    .map_err(|e| SessionMemoryError::Database(e.to_string()))?;

    rows.into_iter()
        .map(|(id, user_id, status, created_at, updated_at)| {
            Ok(Session {
                id: id
                    .parse()
                    .map_err(|_| SessionMemoryError::Database(format!("invalid UUID: {}", id)))?,
                user_id,
                status: status.parse().unwrap_or(SessionStatus::Active),
                created_at: chrono::DateTime::from_timestamp(created_at, 0).unwrap_or_default(),
                updated_at: chrono::DateTime::from_timestamp(updated_at, 0).unwrap_or_default(),
            })
        })
        .collect::<Result<Vec<_>, _>>()
}
