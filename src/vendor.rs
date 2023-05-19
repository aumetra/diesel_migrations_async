use async_trait::async_trait;
use diesel::backend::Backend;
use diesel::migration::{MigrationMetadata, MigrationName, Result};
use diesel::QueryResult;
use diesel_async::{AsyncConnection, SimpleAsyncConnection};

/// Vendored async version of Diesels [`BoxableConnection`](::diesel::connection::BoxableConnection)
///
/// A variant of the [`Connection`](trait.Connection.html) trait that is
/// usable with dynamic dispatch
///
/// If you are looking for a way to use pass database connections
/// for different database backends around in your application
/// this trait won't help you much. Normally you should only
/// need to use this trait if you are interacting with a connection
/// passed to a [`Migration`](../migration/trait.Migration.html)
pub trait AsyncBoxableConnection<DB: Backend>:
    SimpleAsyncConnection + std::any::Any + Send + Sync
{
    /// Maps the current connection to `std::any::Any`
    fn as_any(&self) -> &dyn std::any::Any;

    #[doc(hidden)]
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any;
}

impl<C> AsyncBoxableConnection<C::Backend> for C
where
    C: AsyncConnection + std::any::Any + Send + Sync,
{
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

/// Vendored async version of Diesels `[Migration](::diesel::migration::Migration)` trait
///
/// Represents a migration that interacts with diesel
#[async_trait]
pub trait AsyncMigration<DB: Backend>: Send + Sync {
    /// Apply this migration
    async fn run(&self, conn: &mut dyn AsyncBoxableConnection<DB>) -> Result<()>;

    /// Revert this migration
    async fn revert(&self, conn: &mut dyn AsyncBoxableConnection<DB>) -> Result<()>;

    /// Get a the attached metadata for this migration
    fn metadata(&self) -> &dyn MigrationMetadata;

    /// Get the name of the current migration
    ///
    /// The provided name is used by migration harness
    /// to get the version of a migration and to
    /// as something to that is displayed and allows
    /// user to identify a specific migration
    fn name(&self) -> &(dyn MigrationName + Send + Sync);
}

#[async_trait]
impl<'a, DB: Backend> AsyncMigration<DB> for Box<dyn AsyncMigration<DB> + 'a> {
    async fn run(&self, conn: &mut dyn AsyncBoxableConnection<DB>) -> Result<()> {
        (**self).run(conn).await
    }

    async fn revert(&self, conn: &mut dyn AsyncBoxableConnection<DB>) -> Result<()> {
        (**self).revert(conn).await
    }

    fn metadata(&self) -> &dyn MigrationMetadata {
        (**self).metadata()
    }

    fn name(&self) -> &(dyn MigrationName + Send + Sync) {
        (**self).name()
    }
}

#[async_trait]
impl<'a, DB: Backend> AsyncMigration<DB> for &'a dyn AsyncMigration<DB> {
    async fn run(&self, conn: &mut dyn AsyncBoxableConnection<DB>) -> Result<()> {
        (**self).run(conn).await
    }

    async fn revert(&self, conn: &mut dyn AsyncBoxableConnection<DB>) -> Result<()> {
        (**self).revert(conn).await
    }

    fn metadata(&self) -> &dyn MigrationMetadata {
        (**self).metadata()
    }

    fn name(&self) -> &(dyn MigrationName + Send + Sync) {
        (**self).name()
    }
}

/// Vendored async version of Diesels `[MigrationSource](::diesel::migration::MigrationSource)`
/// A migration source is an entity that can be used
/// to receive a number of migrations from.
pub trait AsyncMigrationSource<DB: Backend> {
    /// Get a list of migrations associated with this
    /// migration soucre.
    fn migrations(&self) -> Result<Vec<Box<dyn AsyncMigration<DB>>>>;
}

/// Vendored version of Diesels `[MigrationConnection](::diesel::migration::MigrationConnection)`
#[async_trait]
pub trait AsyncMigrationConnection: AsyncConnection {
    /// Setup the Diesel migration table
    async fn setup(&mut self) -> QueryResult<usize>;
}

#[cfg(feature = "postgres")]
#[async_trait]
impl AsyncMigrationConnection for diesel_async::AsyncPgConnection {
    async fn setup(&mut self) -> QueryResult<usize> {
        use diesel_async::RunQueryDsl;

        diesel::sql_query(diesel::migration::CREATE_MIGRATIONS_TABLE)
            .execute(self)
            .await
    }
}

#[cfg(feature = "mysql")]
#[async_trait]
impl AsyncMigrationConnection for diesel_async::AsyncMysqlConnection {
    async fn setup(&mut self) -> QueryResult<usize> {
        use diesel_async::RunQueryDsl;

        diesel::sql_query(diesel::migration::CREATE_MIGRATIONS_TABLE)
            .execute(self)
            .await
    }
}
