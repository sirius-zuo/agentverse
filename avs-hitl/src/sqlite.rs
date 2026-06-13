use crate::error::HitlError;
use crate::queue::ApprovalQueue;
use crate::types::{ApprovalDecision, ApprovalId, ApprovalRequest, ApprovalStatus};
use chrono::Utc;
use sqlx::SqlitePool;

pub struct SqliteQueue {
    pool: SqlitePool,
}

impl SqliteQueue {
    pub async fn new(database_url: &str) -> Result<Self, HitlError> {
        let url = if database_url == "sqlite::memory:" {
            database_url.to_string()
        } else if !database_url.starts_with("sqlite:") {
            format!("sqlite:{}?mode=rwc", database_url)
        } else {
            database_url.to_string()
        };
        let pool = SqlitePool::connect(&url)
            .await
            .map_err(|e| HitlError::Database(e.to_string()))?;
        let q = Self { pool };
        q.migrate().await?;
        Ok(q)
    }

    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    async fn migrate(&self) -> Result<(), HitlError> {
        sqlx::migrate!("./migrations")
            .run(&self.pool)
            .await
            .map_err(|e| HitlError::Database(e.to_string()))
    }

    fn row_to_status(status: &str, decision: Option<&str>) -> ApprovalStatus {
        match status {
            "resolved" => {
                let dec: ApprovalDecision = decision
                    .and_then(|d| serde_json::from_str(d).ok())
                    .unwrap_or_else(|| ApprovalDecision::Rejected {
                        reason: "data integrity error: decision missing or unparseable".into(),
                    });
                ApprovalStatus::Resolved(dec)
            }
            "expired" => ApprovalStatus::Expired,
            _ => ApprovalStatus::Pending,
        }
    }
}

#[async_trait::async_trait]
impl ApprovalQueue for SqliteQueue {
    async fn submit(&self, req: ApprovalRequest) -> Result<ApprovalId, HitlError> {
        let id = req.id.to_string();
        let sid = req.session_id.to_string();
        let kind =
            serde_json::to_string(&req.kind).map_err(|e| HitlError::Database(e.to_string()))?;
        let now = Utc::now().timestamp();
        let exp = req.expires_at.map(|t| t.timestamp());

        sqlx::query(
            "INSERT INTO hitl_approvals (id, session_id, kind_json, status, created_at, expires_at)
             VALUES (?, ?, ?, 'pending', ?, ?)",
        )
        .bind(&id)
        .bind(&sid)
        .bind(&kind)
        .bind(now)
        .bind(exp)
        .execute(&self.pool)
        .await
        .map_err(|e| HitlError::Database(e.to_string()))?;

        Ok(req.id)
    }

    async fn resolve(&self, id: ApprovalId, decision: ApprovalDecision) -> Result<(), HitlError> {
        let id_str = id.to_string();
        let dec_str =
            serde_json::to_string(&decision).map_err(|e| HitlError::Database(e.to_string()))?;
        let now = Utc::now().timestamp();

        let rows = sqlx::query(
            "UPDATE hitl_approvals SET status='resolved', decision=?, resolved_at=?
             WHERE id=? AND status='pending'",
        )
        .bind(&dec_str)
        .bind(now)
        .bind(&id_str)
        .execute(&self.pool)
        .await
        .map_err(|e| HitlError::Database(e.to_string()))?;

        if rows.rows_affected() == 0 {
            // Check whether it exists at all
            let exists: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM hitl_approvals WHERE id=?")
                .bind(&id_str)
                .fetch_one(&self.pool)
                .await
                .unwrap_or(0);
            if exists == 0 {
                Err(HitlError::NotFound(id))
            } else {
                Err(HitlError::AlreadyResolved(id))
            }
        } else {
            Ok(())
        }
    }

    async fn poll(&self, id: ApprovalId) -> Result<ApprovalStatus, HitlError> {
        let id_str = id.to_string();
        let row: Option<(String, Option<String>)> =
            sqlx::query_as("SELECT status, decision FROM hitl_approvals WHERE id=?")
                .bind(&id_str)
                .fetch_optional(&self.pool)
                .await
                .map_err(|e| HitlError::Database(e.to_string()))?;

        row.map(|(s, d)| Self::row_to_status(&s, d.as_deref()))
            .ok_or(HitlError::NotFound(id))
    }

    async fn sweep_expired(&self) -> Result<u64, HitlError> {
        let now = Utc::now().timestamp();
        let rows = sqlx::query(
            "UPDATE hitl_approvals SET status='expired'
             WHERE status='pending' AND expires_at IS NOT NULL AND expires_at < ?",
        )
        .bind(now)
        .execute(&self.pool)
        .await
        .map_err(|e| HitlError::Database(e.to_string()))?;
        Ok(rows.rows_affected())
    }
}
