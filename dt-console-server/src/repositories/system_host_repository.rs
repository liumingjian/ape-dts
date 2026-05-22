//! SystemHostRepository — CRUD operations for the `system_hosts` table.

use crate::models::SystemHost;
use sqlx::SqlitePool;

pub struct SystemHostRepository;

impl SystemHostRepository {
    /// Create a new system host.
    pub async fn create(pool: &SqlitePool, host: &SystemHost) -> Result<SystemHost, sqlx::Error> {
        sqlx::query(
            "INSERT INTO system_hosts (id, hostname, ip, status, last_heartbeat, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?)"
        )
        .bind(&host.id)
        .bind(&host.hostname)
        .bind(&host.ip)
        .bind(&host.status)
        .bind(&host.last_heartbeat)
        .bind(&host.created_at)
        .bind(&host.updated_at)
        .execute(pool)
        .await?;

        Self::find_by_id(pool, &host.id).await
    }

    /// Find a system host by id.
    pub async fn find_by_id(pool: &SqlitePool, id: &str) -> Result<SystemHost, sqlx::Error> {
        sqlx::query_as("SELECT * FROM system_hosts WHERE id = ?")
            .bind(id)
            .fetch_one(pool)
            .await
    }

    /// List all system hosts.
    pub async fn list(pool: &SqlitePool) -> Result<Vec<SystemHost>, sqlx::Error> {
        sqlx::query_as("SELECT * FROM system_hosts ORDER BY hostname ASC")
            .fetch_all(pool)
            .await
    }

    /// Update a system host.
    pub async fn update(pool: &SqlitePool, host: &SystemHost) -> Result<SystemHost, sqlx::Error> {
        sqlx::query(
            "UPDATE system_hosts SET hostname = ?, ip = ?, status = ?, last_heartbeat = ?, updated_at = ?
             WHERE id = ?"
        )
        .bind(&host.hostname)
        .bind(&host.ip)
        .bind(&host.status)
        .bind(&host.last_heartbeat)
        .bind(&host.updated_at)
        .bind(&host.id)
        .execute(pool)
        .await?;

        Self::find_by_id(pool, &host.id).await
    }

    /// Delete a system host by id.
    pub async fn delete(pool: &SqlitePool, id: &str) -> Result<(), sqlx::Error> {
        sqlx::query("DELETE FROM system_hosts WHERE id = ?")
            .bind(id)
            .execute(pool)
            .await?;
        Ok(())
    }
}
