use colored::Colorize;

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
            "  ~ column  {:<20} {} → {}",
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
            "No migration needed — schemas are identical.".dimmed()
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
            println!("{}", format!("  ⚠  {warning}").yellow().dimmed());
        }
    }
}

/// Format migration statements as plain SQL (no colors).
pub fn migration_to_sql(statements: &[MigrationStatement]) -> String {
    let mut lines = Vec::new();

    for stmt in statements {
        for warning in &stmt.warnings {
            lines.push(format!("-- ⚠  {warning}"));
        }
        lines.push(stmt.sql.clone());
        lines.push(String::new());
    }

    lines.join("\n")
}
