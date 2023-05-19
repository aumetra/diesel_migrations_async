use std::fmt::Display;

use crate::errors::RunMigrationsError;
use crate::file_based_migrations::{DieselMigrationName, TomlMetadataWrapper};
use crate::{AsyncMigration, AsyncMigrationSource, MigrationError};
use async_trait::async_trait;
use diesel::backend::Backend;
use diesel::migration::{MigrationName, MigrationVersion, Result};

/// A migration source that embeds migrations into the final binary
///
/// This source can be create via the [`embed_migrations!`](crate::embed_migrations!)
/// at compile time.
#[allow(missing_copy_implementations)]
pub struct EmbeddedMigrations {
    migrations: &'static [EmbeddedMigration],
}

impl EmbeddedMigrations {
    #[doc(hidden)]
    pub const fn new(migrations: &'static [EmbeddedMigration]) -> Self {
        Self { migrations }
    }
}

impl<DB: Backend> AsyncMigrationSource<DB> for EmbeddedMigrations {
    fn migrations(&self) -> Result<Vec<Box<dyn AsyncMigration<DB>>>> {
        Ok(self
            .migrations
            .iter()
            .map(|m| Box::new(m) as Box<dyn AsyncMigration<DB>>)
            .collect())
    }
}

#[doc(hidden)]
pub struct EmbeddedMigration {
    up: &'static str,
    down: Option<&'static str>,
    name: EmbeddedName,
    metadata: TomlMetadataWrapper,
}

impl EmbeddedMigration {
    #[doc(hidden)]
    pub const fn new(
        up: &'static str,
        down: Option<&'static str>,
        name: EmbeddedName,
        metadata: TomlMetadataWrapper,
    ) -> Self {
        Self {
            up,
            down,
            name,
            metadata,
        }
    }
}

#[derive(Clone, Copy)]
#[doc(hidden)]
pub struct EmbeddedName {
    name: &'static str,
}

impl EmbeddedName {
    #[doc(hidden)]
    pub const fn new(name: &'static str) -> Self {
        Self { name }
    }
}

impl MigrationName for EmbeddedName {
    fn version(&self) -> MigrationVersion {
        migrations_internals::version_from_string(self.name)
            .expect(
                "This name contains a valid version. We checked this at compile time by our macro",
            )
            .into()
    }
}

impl Display for EmbeddedName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name)
    }
}

#[async_trait]
impl<'a, DB: Backend> AsyncMigration<DB> for &'a EmbeddedMigration {
    async fn run(&self, conn: &mut dyn crate::AsyncBoxableConnection<DB>) -> Result<()> {
        Ok(conn.batch_execute(self.up).await.map_err(|e| {
            let name = DieselMigrationName::from_name(self.name.name)
                .expect("We have a vaild name here, we checked this in `embed_migration!`");
            RunMigrationsError::QueryError(name, e)
        })?)
    }

    async fn revert(&self, conn: &mut dyn crate::AsyncBoxableConnection<DB>) -> Result<()> {
        match self.down {
            Some(down) => Ok(conn.batch_execute(down).await.map_err(|e| {
                let name = DieselMigrationName::from_name(self.name.name)
                    .expect("We have a vaild name here, we checked this in `embed_migration!`");
                RunMigrationsError::QueryError(name, e)
            })?),
            None => Err(MigrationError::NoMigrationRevertFile.into()),
        }
    }

    fn metadata(&self) -> &dyn diesel::migration::MigrationMetadata {
        &self.metadata as &dyn diesel::migration::MigrationMetadata
    }

    fn name(&self) -> &(dyn MigrationName + Send + Sync) {
        &self.name
    }
}
