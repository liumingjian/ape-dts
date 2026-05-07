//! SessionRepository — CRUD operations for the `sessions` table.

use crate::models::Session;
use sqlx::SqlitePool;

pub struct SessionRepository;

impl SessionRepository {
    /// Create a new session.
    pub async fn create(pool: &SqlitePool, session: &Session) -> Result<Session, sqlx::Error> {
        sqlx::query(
            "INSERT INTO sessions (id, user_id, token, created_at, expires_at, ip, user_agent)
             VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&session.id)
        .bind(&session.user_id)
        .bind(&session.token)
        .bind(&session.created_at)
        .bind(&session.expires_at)
        .bind(&session.ip)
        .bind(&session.user_agent)
        .execute(pool)
        .await?;

        Self::find_by_id(pool, &session.id).await
    }

    /// Find a session by id.
    pub async fn find_by_id(pool: &SqlitePool, id: &str) -> Result<Session, sqlx::Error> {
        sqlx::query_as("SELECT * FROM sessions WHERE id = ?")
            .bind(id)
            .fetch_one(pool)
            .await
    }

    /// Find a session by token.
    pub async fn find_by_token(pool: &SqlitePool, token: &str) -> Result<Session, sqlx::Error> {
        sqlx::query_as("SELECT * FROM sessions WHERE token = ?")
            .bind(token)
            .fetch_one(pool)
            .await
    }

    /// Update a session (e.g. to refresh idle expiry).
    pub async fn update(pool: &SqlitePool, session: &Session) -> Result<Session, sqlx::Error> {
        sqlx::query(
            "UPDATE sessions SET user_id = ?, token = ?, created_at = ?, expires_at = ?, ip = ?, user_agent = ?
             WHERE id = ?",
        )
        .bind(&session.user_id)
        .bind(&session.token)
        .bind(&session.created_at)
        .bind(&session.expires_at)
        .bind(&session.ip)
        .bind(&session.user_agent)
        .bind(&session.id)
        .execute(pool)
        .await?;

        Self::find_by_id(pool, &session.id).await
    }

    /// Delete a session by id (logout).
    pub async fn delete(pool: &SqlitePool, id: &str) -> Result<(), sqlx::Error> {
        sqlx::query("DELETE FROM sessions WHERE id = ?")
            .bind(id)
            .execute(pool)
            .await?;
        Ok(())
    }

    /// Delete all sessions for a user (e.g. on disable or password change).
    pub async fn delete_by_user(pool: &SqlitePool, user_id: &str) -> Result<(), sqlx::Error> {
        sqlx::query("DELETE FROM sessions WHERE user_id = ?")
            .bind(user_id)
            .execute(pool)
            .await?;
        Ok(())
    }

    /// Find all sessions for a user.
    pub async fn find_by_user(
        pool: &SqlitePool,
        user_id: &str,
    ) -> Result<Vec<Session>, sqlx::Error> {
        sqlx::query_as("SELECT * FROM sessions WHERE user_id = ?")
            .bind(user_id)
            .fetch_all(pool)
            .await
    }

    /// Delete a session by token.
    pub async fn delete_by_token(pool: &SqlitePool, token: &str) -> Result<(), sqlx::Error> {
        sqlx::query("DELETE FROM sessions WHERE token = ?")
            .bind(token)
            .execute(pool)
            .await?;
        Ok(())
    }
}
