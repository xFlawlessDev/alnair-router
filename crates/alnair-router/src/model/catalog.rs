//! Loads a [`Catalog`] snapshot from the database.

use sqlx::SqlitePool;

use super::resolver::Catalog;
use crate::error::Result;

impl Catalog {
    /// Reads the current routing configuration into a snapshot.
    pub async fn load(pool: &SqlitePool) -> Result<Self> {
        let connections = sqlx::query_as::<_, crate::db::repos::connections::Connection>(
            "SELECT * FROM connections",
        )
        .fetch_all(pool)
        .await?;

        let aliases =
            sqlx::query_as::<_, crate::db::repos::aliases::Alias>("SELECT * FROM aliases")
                .fetch_all(pool)
                .await?;

        let combos = sqlx::query_as::<_, crate::db::repos::combos::Combo>("SELECT * FROM combos")
            .fetch_all(pool)
            .await?;

        let combo_entries =
            sqlx::query_as::<_, crate::db::repos::combos::ComboEntry>("SELECT * FROM combo_entries")
                .fetch_all(pool)
                .await?;

        Ok(Catalog {
            connections,
            aliases,
            combos,
            combo_entries,
        })
    }
}
