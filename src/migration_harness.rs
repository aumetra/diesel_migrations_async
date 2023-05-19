use async_trait::async_trait;
use diesel::associations::HasTable;
use diesel::backend::Backend;
use diesel::migration::{MigrationVersion, Result};
use diesel::query_builder::{DeleteStatement, InsertStatement, IntoUpdateTarget};
use diesel::serialize::ToSql;
use diesel::sql_types::Text;
use diesel::{dsl, ExpressionMethods, Insertable, QueryDsl, QueryResult};
use diesel_async::methods::{ExecuteDsl, LoadQuery};
use diesel_async::scoped_futures::{ScopedBoxFuture, ScopedFutureExt};
use diesel_async::{AsyncConnection, RunQueryDsl};
use std::cell::RefCell;
use std::collections::HashMap;
use std::io::Write;

use crate::errors::MigrationError;
use crate::{AsyncMigration, AsyncMigrationConnection, AsyncMigrationSource};

diesel::table! {
    __diesel_schema_migrations (version) {
        version -> VarChar,
        run_on -> Timestamp,
    }
}

/// A migration harness is an entity which applies migration to an existing database
#[async_trait(?Send)]
pub trait MigrationHarness<DB: Backend> {
    /// Checks if the database represented by the current harness has unapplied migrations
    async fn has_pending_migration<S: AsyncMigrationSource<DB>>(
        &mut self,
        source: S,
    ) -> Result<bool> {
        self.pending_migrations(source).await.map(|p| !p.is_empty())
    }

    /// Execute all unapplied migrations for a given migration source
    async fn run_pending_migrations<S: AsyncMigrationSource<DB>>(
        &mut self,
        source: S,
    ) -> Result<Vec<MigrationVersion>> {
        let pending = self.pending_migrations(source).await?;
        self.run_migrations(&pending).await
    }

    /// Execute all migrations in the given list
    ///
    /// This method does not check if a certain migration was already applied or not
    #[doc(hidden)]
    async fn run_migrations(
        &mut self,
        migrations: &[Box<dyn AsyncMigration<DB>>],
    ) -> Result<Vec<MigrationVersion>> {
        let mut versions = Vec::new();
        for migration in migrations {
            versions.push(self.run_migration(migration).await?);
        }
        Ok(versions)
    }

    /// Execute the next migration from the given migration source
    async fn run_next_migration<S: AsyncMigrationSource<DB>>(
        &mut self,
        source: S,
    ) -> Result<MigrationVersion> {
        let pending_migrations = self.pending_migrations(source).await?;
        let next_migration = pending_migrations
            .get(0)
            .ok_or(MigrationError::NoMigrationRun)?;
        self.run_migration(next_migration).await
    }

    /// Revert all applied migrations from a given migration source
    async fn revert_all_migrations<S: AsyncMigrationSource<DB>>(
        &mut self,
        source: S,
    ) -> Result<Vec<MigrationVersion>> {
        let applied_versions = self.applied_migrations().await?;
        let mut migrations = source
            .migrations()?
            .into_iter()
            .map(|m| (m.name().version().as_owned(), m))
            .collect::<HashMap<_, _>>();

        let mut reverted_version = Vec::new();
        for version in applied_versions {
            let migration_to_revert = migrations
                .remove(&version)
                .ok_or(MigrationError::UnknownMigrationVersion(version))?;
            reverted_version.push(self.revert_migration(&migration_to_revert).await?);
        }
        Ok(reverted_version)
    }

    /// Revert the last migration from a given migration source
    ///
    /// This method returns a error if the given migration source does not
    /// contain the last applied migration
    async fn revert_last_migration<S: AsyncMigrationSource<DB>>(
        &mut self,
        source: S,
    ) -> Result<MigrationVersion<'static>> {
        let applied_versions = self.applied_migrations().await?;
        let migrations = source.migrations()?;
        let last_migration_version = applied_versions
            .get(0)
            .ok_or(MigrationError::NoMigrationRun)?;
        let migration_to_revert = migrations
            .iter()
            .find(|m| m.name().version() == *last_migration_version)
            .ok_or_else(|| {
                MigrationError::UnknownMigrationVersion(last_migration_version.as_owned())
            })?;
        self.revert_migration(migration_to_revert).await
    }

    /// Get a list of non applied migrations for a specific migration source
    ///
    /// The returned migration list is sorted in ascending order by the individual version
    /// of each migration
    async fn pending_migrations<S: AsyncMigrationSource<DB>>(
        &mut self,
        source: S,
    ) -> Result<Vec<Box<dyn AsyncMigration<DB>>>> {
        let applied_versions = self.applied_migrations().await?;
        let mut migrations = source
            .migrations()?
            .into_iter()
            .map(|m| (m.name().version().as_owned(), m))
            .collect::<HashMap<_, _>>();

        for applied_version in applied_versions {
            migrations.remove(&applied_version);
        }

        let mut migrations = migrations.into_values().collect::<Vec<_>>();

        migrations.sort_unstable_by(|a, b| a.name().version().cmp(&b.name().version()));

        Ok(migrations)
    }

    /// Apply a single migration
    ///
    /// Types implementing this trait should call [`Migration::run`] internally and record
    /// that a specific migration version was executed afterwards.
    async fn run_migration(
        &mut self,
        migration: &dyn AsyncMigration<DB>,
    ) -> Result<MigrationVersion<'static>>;

    /// Revert a single migration
    ///
    /// Types implementing this trait should call [`Migration::revert`] internally
    /// and record that a specific migration version was reverted afterwards.
    async fn revert_migration(
        &mut self,
        migration: &dyn AsyncMigration<DB>,
    ) -> Result<MigrationVersion<'static>>;

    /// Get a list of already applied migration versions
    async fn applied_migrations(&mut self) -> Result<Vec<MigrationVersion<'static>>>;
}

fn apply_migration<'a, 'b, C, DB>(
    conn: &'b mut C,
    migration: &'b (dyn AsyncMigration<DB> + 'b),
) -> ScopedBoxFuture<'a, 'b, Result<()>>
where
    DB: Backend,
    C: AsyncConnection<Backend = DB> + AsyncMigrationConnection + Sync + 'static,
    for<'i> InsertStatement<
        __diesel_schema_migrations::table,
        <dsl::Eq<__diesel_schema_migrations::version, MigrationVersion<'static>> as Insertable<
            __diesel_schema_migrations::table,
        >>::Values,
    >: diesel::query_builder::QueryFragment<DB> + ExecuteDsl<C, DB>,
{
    async move {
        migration.run(conn).await?;
        diesel::insert_into(__diesel_schema_migrations::table)
            .values(__diesel_schema_migrations::version.eq(migration.name().version().as_owned()))
            .execute(conn)
            .await?;
        Ok(())
    }
    .scope_boxed()
}

fn revert_migration<'a, 'b, C, DB>(
    conn: &'b mut C,
    migration: &'b (dyn AsyncMigration<DB> + 'b)
) -> ScopedBoxFuture<'a, 'b, Result<()>>
where
    DB: Backend,
    C: AsyncConnection<Backend = DB> + AsyncMigrationConnection + Sync + 'static,
    DeleteStatement<
        <dsl::Find<
            __diesel_schema_migrations::table,
            MigrationVersion<'static>,
        > as HasTable>::Table,
        <dsl::Find<
            __diesel_schema_migrations::table,
            MigrationVersion<'static>,
        > as IntoUpdateTarget>::WhereClause,
    >: ExecuteDsl<C>,
{
    async move {
        migration.revert(conn).await?;
        diesel::delete(
            __diesel_schema_migrations::table.find(migration.name().version().as_owned()),
        )
        .execute(conn)
        .await?;
        Ok(())
    }
    .scope_boxed()
}

#[async_trait(?Send)]
impl<'b, C, DB> MigrationHarness<DB> for C
where
    DB: Backend,
    C: AsyncConnection<Backend = DB> + AsyncMigrationConnection + Sync + 'static,
    dsl::Order<
        dsl::Select<__diesel_schema_migrations::table, __diesel_schema_migrations::version>,
        dsl::Desc<__diesel_schema_migrations::version>,
    >: LoadQuery<'b, C, MigrationVersion<'static>>,
    for<'a> InsertStatement<
        __diesel_schema_migrations::table,
        <dsl::Eq<__diesel_schema_migrations::version, MigrationVersion<'static>> as Insertable<
            __diesel_schema_migrations::table,
        >>::Values,
    >: diesel::query_builder::QueryFragment<DB> + ExecuteDsl<C, DB>,
    DeleteStatement<
        <dsl::Find<
            __diesel_schema_migrations::table,
            MigrationVersion<'static>,
        > as HasTable>::Table,
        <dsl::Find<
            __diesel_schema_migrations::table,
            MigrationVersion<'static>,
        > as IntoUpdateTarget>::WhereClause,
    >: ExecuteDsl<C>,
    str: ToSql<Text, DB>,
{
    async fn run_migration(
        &mut self,
        migration: &dyn AsyncMigration<DB>,
    ) -> Result<MigrationVersion<'static>> {

        if migration.metadata().run_in_transaction() {
            self.transaction(|c| apply_migration(c, migration)).await?;
        } else {
            apply_migration(self, migration).await?;
        }
        Ok(migration.name().version().as_owned())
    }

    async fn revert_migration(
        &mut self,
        migration: &dyn AsyncMigration<DB>,
    ) -> Result<MigrationVersion<'static>> {
        if migration.metadata().run_in_transaction() {
            self.transaction(|c| revert_migration(c, migration)).await?;
        } else {
            revert_migration(self, migration).await?;
        }
        Ok(migration.name().version().as_owned())
    }

    async fn applied_migrations(&mut self) -> Result<Vec<MigrationVersion<'static>>> {
        setup_database(self).await?;
        Ok(
            __diesel_schema_migrations::table
            .select(__diesel_schema_migrations::version)
            .order(__diesel_schema_migrations::version.desc())
            .load(self)
            .await?
        )
    }
}

/// A migration harness that writes messages
/// into some output for each applied/reverted migration
pub struct HarnessWithOutput<'a, C, W> {
    connection: &'a mut C,
    output: RefCell<W>,
}

impl<'a, C, W> HarnessWithOutput<'a, C, W> {
    /// Create a new `HarnessWithOutput` that writes to a specific writer
    pub fn new<DB>(harness: &'a mut C, output: W) -> Self
    where
        C: MigrationHarness<DB>,
        DB: Backend,
        W: Write,
    {
        Self {
            connection: harness,
            output: RefCell::new(output),
        }
    }
}

impl<'a, C> HarnessWithOutput<'a, C, std::io::Stdout> {
    /// Create a new `HarnessWithOutput` that writes to stdout
    pub fn write_to_stdout<DB>(harness: &'a mut C) -> Self
    where
        C: MigrationHarness<DB>,
        DB: Backend,
    {
        Self {
            connection: harness,
            output: RefCell::new(std::io::stdout()),
        }
    }
}

#[async_trait(?Send)]
impl<'a, C, W, DB> MigrationHarness<DB> for HarnessWithOutput<'a, C, W>
where
    W: Write,
    C: MigrationHarness<DB>,
    DB: Backend,
{
    async fn run_migration(
        &mut self,
        migration: &dyn AsyncMigration<DB>,
    ) -> Result<MigrationVersion<'static>> {
        if migration.name().version() != MigrationVersion::from("00000000000000") {
            let mut output = self.output.try_borrow_mut()?;
            writeln!(output, "Running migration {}", migration.name())?;
        }
        self.connection.run_migration(migration).await
    }

    async fn revert_migration(
        &mut self,
        migration: &dyn AsyncMigration<DB>,
    ) -> Result<MigrationVersion<'static>> {
        if migration.name().version() != MigrationVersion::from("00000000000000") {
            let mut output = self.output.try_borrow_mut()?;
            writeln!(output, "Rolling back migration {}", migration.name())?;
        }
        self.connection.revert_migration(migration).await
    }

    async fn applied_migrations(&mut self) -> Result<Vec<MigrationVersion<'static>>> {
        self.connection.applied_migrations().await
    }
}

async fn setup_database<Conn: AsyncMigrationConnection>(conn: &mut Conn) -> QueryResult<usize> {
    conn.setup().await
}
