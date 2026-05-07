//! UserRepository — CRUD operations for the `users` table.

use crate::models::User;
use sqlx::SqlitePool;

pub struct UserRepository;

impl UserRepository {
    /// Create a new user. Returns the inserted user.
    pub async fn create(pool: &SqlitePool, user: &User) -> Result<User, sqlx::Error> {
        sqlx::query(
            "INSERT INTO users (id, username, password_hash, display_name, role, disabled, resource_group_id, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)"
        )
        .bind(&user.id)
        .bind(&user.username)
        .bind(&user.password_hash)
        .bind(&user.display_name)
        .bind(&user.role)
        .bind(user.disabled)
        .bind(&user.resource_group_id)
        .bind(&user.created_at)
        .bind(&user.updated_at)
        .execute(pool)
        .await?;

        Self::find_by_id(pool, &user.id).await
    }

    /// Find a user by id.
    pub async fn find_by_id(pool: &SqlitePool, id: &str) -> Result<User, sqlx::Error> {
        sqlx::query_as("SELECT * FROM users WHERE id = ?")
            .bind(id)
            .fetch_one(pool)
            .await
    }

    /// Find a user by username.
    pub async fn find_by_username(pool: &SqlitePool, username: &str) -> Result<User, sqlx::Error> {
        sqlx::query_as("SELECT * FROM users WHERE username = ?")
            .bind(username)
            .fetch_one(pool)
            .await
    }

    /// List all users.
    pub async fn list(pool: &SqlitePool) -> Result<Vec<User>, sqlx::Error> {
        sqlx::query_as("SELECT * FROM users ORDER BY created_at ASC")
            .fetch_all(pool)
            .await
    }

    /// Update a user (all mutable fields).
    pub async fn update(pool: &SqlitePool, user: &User) -> Result<User, sqlx::Error> {
        sqlx::query(
            "UPDATE users SET username = ?, password_hash = ?, display_name = ?, role = ?, disabled = ?, resource_group_id = ?, updated_at = ?
             WHERE id = ?"
        )
        .bind(&user.username)
        .bind(&user.password_hash)
        .bind(&user.display_name)
        .bind(&user.role)
        .bind(user.disabled)
        .bind(&user.resource_group_id)
        .bind(&user.updated_at)
        .bind(&user.id)
        .execute(pool)
        .await?;

        Self::find_by_id(pool, &user.id).await
    }

    /// Delete a user by id.
    pub async fn delete(pool: &SqlitePool, id: &str) -> Result<(), sqlx::Error> {
        sqlx::query("DELETE FROM users WHERE id = ?")
            .bind(id)
            .execute(pool)
            .await?;
        Ok(())
    }

    /// Count users with a given role.
    pub async fn count_by_role(pool: &SqlitePool, role: &str) -> Result<i64, sqlx::Error> {
        let row: (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM users WHERE role = ? AND disabled = 0")
                .bind(role)
                .fetch_one(pool)
                .await?;
        Ok(row.0)
    }
}
