use colored::Colorize;
use serde::Serialize;

use crate::diff::{ColumnDiff, SchemaDiff, TableDiff};
use crate::migration::MigrationStatement;
use crate::model::Column;

/// Print a colored schema diff to stdout.
pub fn print_diff(diff: &SchemaDiff) {
    // Added tables
    for table in &diff.added_tables {
        println!("{}", format!("+ table: {}", table.name).green());
        for col in table.columns.values() {
            println!(
                "{}",
                format!("  + column  {:<20} {}", col.name, col_type_str(col)).green()
            );
        }
        for idx in table.indexes.values() {
            println!("{}", format!("  + index   {}", idx.definition()).green());
        }
        println!();
    }

    // Removed tables
    for table in &diff.removed_tables {
        println!("{}", format!("- table: {}", table.name).red());
        for col in table.columns.values() {
            println!(
                "{}",
                format!("  - column  {:<20} {}", col.name, col_type_str(col)).red()
            );
        }
        println!();
    }

    // Modified tables
    for table_diff in &diff.modified_tables {
        println!("{}", format!("~ table: {}", table_diff.table_name).yellow());
        print_table_diff(table_diff);
        println!();
    }

    // Unchanged tables (dimmed)
    if !diff.unchanged_tables.is_empty() {
        for name in &diff.unchanged_tables {
            println!("{}", format!("  table: {} [unchanged]", name).dimmed());
        }
        println!();
    }
}

fn print_table_diff(diff: &TableDiff) {
    for col in &diff.added_columns {
        println!(
            "{}",
            format!("  + column  {:<20} {}", col.name, col_type_str(col)).green()
        );
    }

    for col in &diff.removed_columns {
        println!(
            "{}",
            format!("  - column  {:<20} {}", col.name, col_type_str(col)).red()
        );
    }

    for col_diff in &diff.modified_columns {
        print_column_diff(col_diff);
    }

    for name in &diff.unchanged_columns {
        println!("{}", format!("    column  {name:<20} [unchanged]").dimmed());
    }

    for idx in &diff.added_indexes {
        println!("{}", format!("  + index   {}", idx.definition()).green());
    }

    for idx in &diff.removed_indexes {
        println!("{}", format!("  - index   {}", idx.definition()).red());
    }
}

fn print_column_diff(diff: &ColumnDiff) {
    println!(
        "{}",
        format!(
            "  ~ column  {:<20} {} -> {}",
            diff.new.name,
            col_type_str(&diff.old),
            col_type_str(&diff.new)
        )
        .yellow()
    );
}

fn col_type_str(col: &Column) -> String {
    let mut s = col.data_type.clone();
    if !col.is_nullable {
        s.push_str(" NOT NULL");
    }
    if let Some(ref default) = col.default {
        s.push_str(&format!(" DEFAULT {default}"));
    }
    s
}

/// Print generated migration statements with warnings.
pub fn print_migration(statements: &[MigrationStatement]) {
    if statements.is_empty() {
        println!(
            "{}",
            "No migration needed -- schemas are identical.".dimmed()
        );
        return;
    }

    println!("{}", "Generated migration".bold().underline());
    println!();

    for stmt in statements {
        // Determine color based on statement type
        let sql_upper = stmt.sql.to_uppercase();
        if sql_upper.starts_with("DROP") {
            println!("{}", format!("  {}", stmt.sql).red());
        } else if sql_upper.starts_with("CREATE") || sql_upper.contains("ADD COLUMN") {
            println!("{}", format!("  {}", stmt.sql).green());
        } else {
            println!("{}", format!("  {}", stmt.sql).yellow());
        }

        for warning in &stmt.warnings {
            println!("{}", format!("  !!  {warning}").yellow().dimmed());
        }
    }
}

/// Format migration statements as plain SQL (no colors).
pub fn migration_to_sql(statements: &[MigrationStatement]) -> String {
    let mut lines = Vec::new();

    for stmt in statements {
        for warning in &stmt.warnings {
            lines.push(format!("-- !!  {warning}"));
        }
        lines.push(stmt.sql.clone());
        lines.push(String::new());
    }

    lines.join("\n")
}

/// Summary statistics for a schema diff.
#[derive(Debug, Clone, Serialize)]
pub struct DiffSummary {
    pub tables_added: usize,
    pub tables_removed: usize,
    pub tables_modified: usize,
    pub tables_unchanged: usize,
    pub columns_added: usize,
    pub columns_removed: usize,
    pub columns_modified: usize,
    pub indexes_added: usize,
    pub indexes_removed: usize,
}

/// Compute summary statistics from a diff.
pub fn diff_summary(diff: &SchemaDiff) -> DiffSummary {
    let columns_added: usize = diff
        .modified_tables
        .iter()
        .map(|t| t.added_columns.len())
        .sum::<usize>()
        + diff
            .added_tables
            .iter()
            .map(|t| t.columns.len())
            .sum::<usize>();

    let columns_removed: usize = diff
        .modified_tables
        .iter()
        .map(|t| t.removed_columns.len())
        .sum::<usize>()
        + diff
            .removed_tables
            .iter()
            .map(|t| t.columns.len())
            .sum::<usize>();

    let columns_modified: usize = diff
        .modified_tables
        .iter()
        .map(|t| t.modified_columns.len())
        .sum();

    let indexes_added: usize = diff
        .modified_tables
        .iter()
        .map(|t| t.added_indexes.len())
        .sum::<usize>()
        + diff
            .added_tables
            .iter()
            .map(|t| t.indexes.len())
            .sum::<usize>();

    let indexes_removed: usize = diff
        .modified_tables
        .iter()
        .map(|t| t.removed_indexes.len())
        .sum::<usize>()
        + diff
            .removed_tables
            .iter()
            .map(|t| t.indexes.len())
            .sum::<usize>();

    DiffSummary {
        tables_added: diff.added_tables.len(),
        tables_removed: diff.removed_tables.len(),
        tables_modified: diff.modified_tables.len(),
        tables_unchanged: diff.unchanged_tables.len(),
        columns_added,
        columns_removed,
        columns_modified,
        indexes_added,
        indexes_removed,
    }
}

/// Print a summary line to stdout.
pub fn print_summary(diff: &SchemaDiff) {
    if diff.is_empty() {
        println!("{}", "Schemas are identical.".green());
        return;
    }

    let s = diff_summary(diff);
    let mut parts = Vec::new();

    if s.tables_added > 0 {
        parts.push(format!("{} table(s) added", s.tables_added));
    }
    if s.tables_removed > 0 {
        parts.push(format!("{} table(s) removed", s.tables_removed));
    }
    if s.tables_modified > 0 {
        parts.push(format!("{} table(s) modified", s.tables_modified));
    }

    let mut detail_parts = Vec::new();
    if s.columns_added > 0 {
        detail_parts.push(format!("{} columns added", s.columns_added));
    }
    if s.columns_removed > 0 {
        detail_parts.push(format!("{} columns removed", s.columns_removed));
    }
    if s.columns_modified > 0 {
        detail_parts.push(format!("{} columns altered", s.columns_modified));
    }
    if s.indexes_added > 0 {
        detail_parts.push(format!("{} indexes added", s.indexes_added));
    }
    if s.indexes_removed > 0 {
        detail_parts.push(format!("{} indexes removed", s.indexes_removed));
    }

    let mut summary = format!("Summary: {}", parts.join(", "));
    if !detail_parts.is_empty() {
        summary.push_str(&format!(" ({})", detail_parts.join(", ")));
    }

    println!("{}", summary.bold());
}
