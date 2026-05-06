//! GlobalParamRepository — CRUD operations for the `global_params` table.

use crate::models::GlobalParam;
use sqlx::SqlitePool;

pub struct GlobalParamRepository;

impl GlobalParamRepository {
    /// Create a new global param.
    pub async fn create(pool: &SqlitePool, gp: &GlobalParam) -> Result<GlobalParam, sqlx::Error> {
        sqlx::query(
            "INSERT INTO global_params (id, key, value, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?)",
        )
        .bind(&gp.id)
        .bind(&gp.key)
        .bind(&gp.value)
        .bind(&gp.created_at)
        .bind(&gp.updated_at)
        .execute(pool)
        .await?;

        Self::find_by_id(pool, &gp.id).await
    }

    /// Find a global param by id.
    pub async fn find_by_id(pool: &SqlitePool, id: &str) -> Result<GlobalParam, sqlx::Error> {
        sqlx::query_as("SELECT * FROM global_params WHERE id = ?")
            .bind(id)
            .fetch_one(pool)
            .await
    }

    /// Find a global param by key.
    pub async fn find_by_key(pool: &SqlitePool, key: &str) -> Result<GlobalParam, sqlx::Error> {
        sqlx::query_as("SELECT * FROM global_params WHERE key = ?")
            .bind(key)
            .fetch_one(pool)
            .await
    }

    /// List all global params.
    pub async fn list(pool: &SqlitePool) -> Result<Vec<GlobalParam>, sqlx::Error> {
        sqlx::query_as("SELECT * FROM global_params ORDER BY key ASC")
            .fetch_all(pool)
            .await
    }

    /// Update a global param.
    pub async fn update(pool: &SqlitePool, gp: &GlobalParam) -> Result<GlobalParam, sqlx::Error> {
        sqlx::query("UPDATE global_params SET key = ?, value = ?, updated_at = ? WHERE id = ?")
            .bind(&gp.key)
            .bind(&gp.value)
            .bind(&gp.updated_at)
            .bind(&gp.id)
            .execute(pool)
            .await?;

        Self::find_by_id(pool, &gp.id).await
    }
}
