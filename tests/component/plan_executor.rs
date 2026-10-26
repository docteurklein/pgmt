//! `db::plan_executor` runs every generated plan: `pgmt apply`, settling the
//! desired state, and the migration test harness.

use crate::helpers::harness::with_test_db;
use anyhow::Result;
use pgmt::db::plan_executor::{RenderedSection, execute_sections};
use pgmt::migration::section_parser::TransactionMode;
use pgmt::render::RenderedSql;

fn section(mode: TransactionMode, statements: &[String]) -> RenderedSection {
    RenderedSection {
        mode,
        statements: statements
            .iter()
            .map(|sql| RenderedSql::new(sql.clone()))
            .collect(),
    }
}

fn creates(names: impl IntoIterator<Item = String>) -> Vec<String> {
    names
        .into_iter()
        .map(|name| format!("CREATE TABLE {name} (id int);"))
        .collect()
}

async fn table_count(pool: &sqlx::PgPool, pattern: &str) -> Result<i64> {
    Ok(sqlx::query_scalar(
        "SELECT count(*) FROM pg_tables WHERE schemaname = 'public' AND tablename LIKE $1",
    )
    .bind(pattern)
    .fetch_one(pool)
    .await?)
}

/// A failed section's batch does not say which statement broke, so the
/// executor replays it to find out, and leaves none of the section behind.
/// The failure sits past the first batch, so the replay has to rerun the
/// batches before it; the section before it has already committed.
#[tokio::test]
async fn test_failed_section_names_its_statement_and_rolls_back() -> Result<()> {
    with_test_db(async |db| {
        let mut failing = creates((1..=700).map(|i| format!("t{i}")));
        failing[649] = "CREATE TABLE broken (id missing_type);".to_string();
        let sections = [
            section(TransactionMode::Transactional, &creates(["kept".into()])),
            section(TransactionMode::Transactional, &failing),
        ];

        let error = execute_sections(db.pool(), &sections, |_| {})
            .await
            .expect_err("the second section should fail")
            .to_string();

        assert!(error.contains("step 651"), "{error}");
        assert!(error.contains("missing_type"), "{error}");
        assert_eq!(table_count(db.pool(), "kept").await?, 1);
        assert_eq!(table_count(db.pool(), "t%").await?, 0);
        Ok(())
    })
    .await
}

/// A non-transactional section runs outside a transaction block, where a
/// statement like `CREATE INDEX CONCURRENTLY` is allowed.
#[tokio::test]
async fn test_non_transactional_section_runs_outside_a_transaction() -> Result<()> {
    with_test_db(async |db| {
        let sections = [
            section(TransactionMode::Transactional, &creates(["t".into()])),
            section(
                TransactionMode::NonTransactional,
                &["CREATE INDEX CONCURRENTLY t_id ON t (id);".to_string()],
            ),
        ];

        let mut progress = Vec::new();
        execute_sections(db.pool(), &sections, |done| progress.push(done)).await?;

        let indexed: bool =
            sqlx::query_scalar("SELECT EXISTS (SELECT FROM pg_indexes WHERE indexname = 't_id')")
                .fetch_one(db.pool())
                .await?;
        assert!(indexed);
        assert_eq!(progress, [1, 2]);
        Ok(())
    })
    .await
}
