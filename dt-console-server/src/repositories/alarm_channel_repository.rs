//! AlarmChannelRepository — CRUD operations for the `alarm_channels` table.

use crate::models::AlarmChannel;
use sqlx::SqlitePool;

pub struct AlarmChannelRepository;

impl AlarmChannelRepository {
    /// Create a new alarm channel.
    pub async fn create(pool: &SqlitePool, ch: &AlarmChannel) -> Result<AlarmChannel, sqlx::Error> {
        sqlx::query(
            "INSERT INTO alarm_channels (id, name, kind, config, enabled, resource_group_id, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)"
        )
        .bind(&ch.id)
        .bind(&ch.name)
        .bind(&ch.kind)
        .bind(&ch.config)
        .bind(ch.enabled)
        .bind(&ch.resource_group_id)
        .bind(&ch.created_at)
        .bind(&ch.updated_at)
        .execute(pool)
        .await?;

        Self::find_by_id(pool, &ch.id).await
    }

    /// Find an alarm channel by id.
    pub async fn find_by_id(pool: &SqlitePool, id: &str) -> Result<AlarmChannel, sqlx::Error> {
        sqlx::query_as("SELECT * FROM alarm_channels WHERE id = ?")
            .bind(id)
            .fetch_one(pool)
            .await
    }

    /// List all alarm channels.
    pub async fn list(pool: &SqlitePool) -> Result<Vec<AlarmChannel>, sqlx::Error> {
        sqlx::query_as("SELECT * FROM alarm_channels ORDER BY created_at ASC")
            .fetch_all(pool)
            .await
    }

    /// Update an alarm channel.
    pub async fn update(pool: &SqlitePool, ch: &AlarmChannel) -> Result<AlarmChannel, sqlx::Error> {
        sqlx::query(
            "UPDATE alarm_channels SET name = ?, kind = ?, config = ?, enabled = ?,
             resource_group_id = ?, updated_at = ?
             WHERE id = ?",
        )
        .bind(&ch.name)
        .bind(&ch.kind)
        .bind(&ch.config)
        .bind(ch.enabled)
        .bind(&ch.resource_group_id)
        .bind(&ch.updated_at)
        .bind(&ch.id)
        .execute(pool)
        .await?;

        Self::find_by_id(pool, &ch.id).await
    }

    /// Delete an alarm channel by id.
    pub async fn delete(pool: &SqlitePool, id: &str) -> Result<(), sqlx::Error> {
        sqlx::query("DELETE FROM alarm_channels WHERE id = ?")
            .bind(id)
            .execute(pool)
            .await?;
        Ok(())
    }
}
