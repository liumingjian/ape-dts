//! LicenseRepository — CRUD operations for the `licenses` table.

use crate::models::License;
use sqlx::SqlitePool;

pub struct LicenseRepository;

impl LicenseRepository {
    /// Create (activate) a license.
    pub async fn create(pool: &SqlitePool, license: &License) -> Result<License, sqlx::Error> {
        sqlx::query(
            "INSERT INTO licenses (id, sku, max_tasks, expire_at, activated_at, activation_code_hash, granted_to, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)"
        )
        .bind(&license.id)
        .bind(&license.sku)
        .bind(license.max_tasks)
        .bind(&license.expire_at)
        .bind(&license.activated_at)
        .bind(&license.activation_code_hash)
        .bind(&license.granted_to)
        .bind(&license.created_at)
        .bind(&license.updated_at)
        .execute(pool)
        .await?;

        Self::find_by_id(pool, &license.id).await
    }

    /// Find a license by id.
    pub async fn find_by_id(pool: &SqlitePool, id: &str) -> Result<License, sqlx::Error> {
        sqlx::query_as("SELECT * FROM licenses WHERE id = ?")
            .bind(id)
            .fetch_one(pool)
            .await
    }

    /// Get the current license (there should be at most one row).
    pub async fn get_current(pool: &SqlitePool) -> Result<Option<License>, sqlx::Error> {
        sqlx::query_as("SELECT * FROM licenses ORDER BY created_at DESC LIMIT 1")
            .fetch_optional(pool)
            .await
    }

    /// Alias for `get_current` — used by readyz readiness probe.
    pub async fn get(pool: &SqlitePool) -> Result<Option<License>, sqlx::Error> {
        Self::get_current(pool).await
    }

    /// Update a license.
    pub async fn update(pool: &SqlitePool, license: &License) -> Result<License, sqlx::Error> {
        sqlx::query(
            "UPDATE licenses SET sku = ?, max_tasks = ?, expire_at = ?, activated_at = ?,
             activation_code_hash = ?, granted_to = ?, updated_at = ?
             WHERE id = ?",
        )
        .bind(&license.sku)
        .bind(license.max_tasks)
        .bind(&license.expire_at)
        .bind(&license.activated_at)
        .bind(&license.activation_code_hash)
        .bind(&license.granted_to)
        .bind(&license.updated_at)
        .bind(&license.id)
        .execute(pool)
        .await?;

        Self::find_by_id(pool, &license.id).await
    }
}
