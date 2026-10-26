use anyhow::Result;
use sqlx::PgPool;
use tracing::info;

use crate::catalog::Catalog;
use crate::config::Config;
use crate::db::plan_executor::{RenderedSection, execute_sections, retain_statements};
use crate::render::Safety;

use super::ApplyOutcome;
use super::verification::verify_final_state;

/// Apply all rendered steps (both safe and destructive)
pub async fn apply_all_rendered_steps(
    sections: &[RenderedSection],
    dev_pool: &PgPool,
    expected_catalog: &Catalog,
    config: &Config,
) -> Result<ApplyOutcome> {
    run_sections(sections, dev_pool).await?;
    verify_final_state(dev_pool, expected_catalog, config).await?;
    Ok(ApplyOutcome::Applied)
}

/// Apply only safe rendered steps, optionally showing destructive ones
pub async fn apply_safe_rendered_steps(
    sections: &[RenderedSection],
    dev_pool: &PgPool,
    expected_catalog: &Catalog,
    config: &Config,
    show_skipped: bool,
) -> Result<ApplyOutcome> {
    let rendered: Vec<_> = sections.iter().flat_map(|s| &s.statements).collect();
    let safe_count = rendered.iter().filter(|s| s.safety == Safety::Safe).count();
    let destructive_count = rendered
        .iter()
        .filter(|s| s.safety == Safety::Destructive)
        .count();

    let has_skipped = destructive_count > 0;

    // Handle destructive operations
    if destructive_count > 0 && show_skipped {
        println!(
            "⚠️  {} destructive operation{} detected but not applied in safe-only mode",
            destructive_count,
            if destructive_count == 1 { "" } else { "s" }
        );

        for (i, step) in rendered.iter().enumerate() {
            if step.safety == Safety::Destructive {
                let sql_preview = step.sql.lines().next().unwrap_or("").trim();
                println!("   ❌ Skipped: {}", sql_preview);
                info!("Skipped step {}: {}", i + 1, sql_preview);
            }
        }

        if safe_count == 0 {
            println!("⚠️  No safe operations to apply");
            return Ok(ApplyOutcome::Skipped);
        }
    }

    // Apply safe operations
    if safe_count > 0 {
        println!(
            "✅ Auto-applying {} safe operation{}...",
            safe_count,
            if safe_count == 1 { "" } else { "s" }
        );

        let safe = retain_statements(sections, |s| s.safety == Safety::Safe);
        run_sections(&safe, dev_pool).await?;

        info!("Safe operations completed successfully");

        verify_final_state(dev_pool, expected_catalog, config).await?;
    }

    if has_skipped {
        Ok(ApplyOutcome::Skipped)
    } else {
        Ok(ApplyOutcome::Applied)
    }
}

/// Run `sections` on the dev database, logging progress after each round trip.
pub async fn run_sections(sections: &[RenderedSection], dev_pool: &PgPool) -> Result<()> {
    let total: usize = sections.iter().map(|s| s.statements.len()).sum();
    info!("Executing {} migration steps...", total);
    execute_sections(dev_pool, sections, |done| {
        info!("Applied {}/{} steps", done, total)
    })
    .await
}
