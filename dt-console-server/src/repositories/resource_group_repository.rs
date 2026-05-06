//! ResourceGroupRepository — CRUD operations for the `resource_groups` table.

use crate::models::ResourceGroup;
use sqlx::SqlitePool;

pub struct ResourceGroupRepository;

impl ResourceGroupRepository {
    /// Create a new resource group.
    pub async fn create(
        pool: &SqlitePool,
        rg: &ResourceGroup,
    ) -> Result<ResourceGroup, sqlx::Error> {
        sqlx::query(
            "INSERT INTO resource_groups (id, name, is_default, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?)",
        )
        .bind(&rg.id)
        .bind(&rg.name)
        .bind(rg.is_default)
        .bind(&rg.created_at)
        .bind(&rg.updated_at)
        .execute(pool)
        .await?;

        Self::find_by_id(pool, &rg.id).await
    }

    /// Find a resource group by id.
    pub async fn find_by_id(pool: &SqlitePool, id: &str) -> Result<ResourceGroup, sqlx::Error> {
        sqlx::query_as("SELECT * FROM resource_groups WHERE id = ?")
            .bind(id)
            .fetch_one(pool)
            .await
    }

    /// Find a resource group by name.
    pub async fn find_by_name(pool: &SqlitePool, name: &str) -> Result<ResourceGroup, sqlx::Error> {
        sqlx::query_as("SELECT * FROM resource_groups WHERE name = ?")
            .bind(name)
            .fetch_one(pool)
            .await
    }

    /// Get the default resource group.
    pub async fn get_default(pool: &SqlitePool) -> Result<ResourceGroup, sqlx::Error> {
        sqlx::query_as("SELECT * FROM resource_groups WHERE is_default = 1 LIMIT 1")
            .fetch_one(pool)
            .await
    }

    /// List all resource groups.
    pub async fn list(pool: &SqlitePool) -> Result<Vec<ResourceGroup>, sqlx::Error> {
        sqlx::query_as("SELECT * FROM resource_groups ORDER BY created_at ASC")
            .fetch_all(pool)
            .await
    }

    /// Update a resource group name.
    pub async fn update(
        pool: &SqlitePool,
        rg: &ResourceGroup,
    ) -> Result<ResourceGroup, sqlx::Error> {
        sqlx::query(
            "UPDATE resource_groups SET name = ?, is_default = ?, updated_at = ? WHERE id = ?",
        )
        .bind(&rg.name)
        .bind(rg.is_default)
        .bind(&rg.updated_at)
        .bind(&rg.id)
        .execute(pool)
        .await?;

        Self::find_by_id(pool, &rg.id).await
    }

    /// Delete a resource group by id.
    pub async fn delete(pool: &SqlitePool, id: &str) -> Result<(), sqlx::Error> {
        sqlx::query("DELETE FROM resource_groups WHERE id = ?")
            .bind(id)
            .execute(pool)
            .await?;
        Ok(())
    }
}
