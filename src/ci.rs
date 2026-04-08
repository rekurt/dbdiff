use serde::Serialize;

use crate::diff::SchemaDiff;
use crate::migration::MigrationStatement;

/// Exit code: schemas are identical — safe to deploy.
pub const EXIT_OK: u8 = 0;

/// Exit code: schema drift detected (new columns, indexes, etc.).
pub const EXIT_DRIFT: u8 = 1;

/// Exit code: error connecting to DB or parsing schema.
pub const EXIT_ERROR: u8 = 2;

/// Exit code: drift with blocking operations (only with --fail-on-blocking).
pub const EXIT_UNSAFE: u8 = 3;

/// A structured CI report suitable for JSON/YAML serialization.
#[derive(Debug, Clone, Serialize)]
pub struct CiReport {
    pub equal: bool,
    pub summary: String,
    pub changes: Vec<CiChange>,
    pub blocking: Vec<CiChange>,
}

/// A single change entry in the CI report.
#[derive(Debug, Clone, Serialize)]
pub struct CiChange {
    #[serde(rename = "type")]
    pub change_type: String,
    pub object: String,
    pub table: String,
    pub name: String,
    pub description: String,
    pub is_blocking: bool,
    pub sql: String,
}

impl CiReport {
    /// Build a CI report from a schema diff and generated migration statements.
    pub fn from_diff(diff: &SchemaDiff, statements: &[MigrationStatement]) -> Self {
        let mut changes = Vec::new();

        // Added tables
        for table in &diff.added_tables {
            changes.push(CiChange {
                change_type: "ADD".to_string(),
                object: "TABLE".to_string(),
                table: table.name.clone(),
                name: table.name.clone(),
                description: format!("ADD TABLE {}", table.name),
                is_blocking: false,
                sql: statements
                    .iter()
                    .find(|s| s.sql.contains("CREATE TABLE") && s.sql.contains(&table.name))
                    .map(|s| s.sql.clone())
                    .unwrap_or_default(),
            });
        }

        // Removed tables
        for table in &diff.removed_tables {
            changes.push(CiChange {
                change_type: "DROP".to_string(),
                object: "TABLE".to_string(),
                table: table.name.clone(),
                name: table.name.clone(),
                description: format!("DROP TABLE {}", table.name),
                is_blocking: true,
                sql: statements
                    .iter()
                    .find(|s| s.sql.contains("DROP TABLE") && s.sql.contains(&table.name))
                    .map(|s| s.sql.clone())
                    .unwrap_or_default(),
            });
        }

        // Modified tables
        for table_diff in &diff.modified_tables {
            let tname = &table_diff.table_name;

            for col in &table_diff.added_columns {
                let stmt = statements.iter().find(|s| {
                    s.sql.contains("ADD COLUMN")
                        && s.sql.contains(&col.name)
                        && s.sql.contains(tname.as_str())
                });
                let blocking = stmt.map(|s| s.is_blocking).unwrap_or(false);
                changes.push(CiChange {
                    change_type: "ADD".to_string(),
                    object: "COLUMN".to_string(),
                    table: tname.clone(),
                    name: col.name.clone(),
                    description: format!("ADD COLUMN {}", col.definition()),
                    is_blocking: blocking,
                    sql: stmt.map(|s| s.sql.clone()).unwrap_or_default(),
                });
            }

            for col in &table_diff.removed_columns {
                changes.push(CiChange {
                    change_type: "DROP".to_string(),
                    object: "COLUMN".to_string(),
                    table: tname.clone(),
                    name: col.name.clone(),
                    description: format!("DROP COLUMN {}", col.name),
                    is_blocking: false,
                    sql: statements
                        .iter()
                        .find(|s| {
                            s.sql.contains("DROP COLUMN")
                                && s.sql.contains(&col.name)
                                && s.sql.contains(tname.as_str())
                        })
                        .map(|s| s.sql.clone())
                        .unwrap_or_default(),
                });
            }

            for col_diff in &table_diff.modified_columns {
                let col = &col_diff.new;
                // Find any ALTER statement for this column
                let stmt = statements.iter().find(|s| {
                    s.sql.contains("ALTER COLUMN")
                        && s.sql.contains(&col.name)
                        && s.sql.contains(tname.as_str())
                });
                let blocking = stmt.map(|s| s.is_blocking).unwrap_or(false);
                changes.push(CiChange {
                    change_type: "ALTER".to_string(),
                    object: "COLUMN".to_string(),
                    table: tname.clone(),
                    name: col.name.clone(),
                    description: format!(
                        "ALTER COLUMN {} {} → {}",
                        col.name,
                        col_diff.old.definition(),
                        col.definition()
                    ),
                    is_blocking: blocking,
                    sql: stmt.map(|s| s.sql.clone()).unwrap_or_default(),
                });
            }

            for idx in &table_diff.added_indexes {
                let stmt = statements
                    .iter()
                    .find(|s| s.sql.contains("CREATE") && s.sql.contains(&idx.name));
                let blocking = stmt.map(|s| s.is_blocking).unwrap_or(true);
                changes.push(CiChange {
                    change_type: "ADD".to_string(),
                    object: "INDEX".to_string(),
                    table: tname.clone(),
                    name: idx.name.clone(),
                    description: format!("ADD INDEX {}", idx.definition()),
                    is_blocking: blocking,
                    sql: stmt.map(|s| s.sql.clone()).unwrap_or_default(),
                });
            }

            for idx in &table_diff.removed_indexes {
                changes.push(CiChange {
                    change_type: "DROP".to_string(),
                    object: "INDEX".to_string(),
                    table: tname.clone(),
                    name: idx.name.clone(),
                    description: format!("DROP INDEX {}", idx.name),
                    is_blocking: true,
                    sql: statements
                        .iter()
                        .find(|s| s.sql.contains("DROP INDEX") && s.sql.contains(&idx.name))
                        .map(|s| s.sql.clone())
                        .unwrap_or_default(),
                });
            }

            // Constraints
            for c in &table_diff.added_constraints {
                let stmt = statements
                    .iter()
                    .find(|s| s.sql.contains("ADD CONSTRAINT") && s.sql.contains(&c.name));
                changes.push(CiChange {
                    change_type: "ADD".to_string(),
                    object: "CONSTRAINT".to_string(),
                    table: tname.clone(),
                    name: c.name.clone(),
                    description: format!("ADD CONSTRAINT {} {}", c.name, c.definition()),
                    is_blocking: stmt.map(|s| s.is_blocking).unwrap_or(true),
                    sql: stmt.map(|s| s.sql.clone()).unwrap_or_default(),
                });
            }

            for c in &table_diff.removed_constraints {
                changes.push(CiChange {
                    change_type: "DROP".to_string(),
                    object: "CONSTRAINT".to_string(),
                    table: tname.clone(),
                    name: c.name.clone(),
                    description: format!("DROP CONSTRAINT {}", c.name),
                    is_blocking: true,
                    sql: statements
                        .iter()
                        .find(|s| s.sql.contains("DROP CONSTRAINT") && s.sql.contains(&c.name))
                        .or_else(|| {
                            statements.iter().find(|s| {
                                s.sql.contains("DROP FOREIGN KEY") && s.sql.contains(&c.name)
                            })
                        })
                        .map(|s| s.sql.clone())
                        .unwrap_or_default(),
                });
            }
        }

        // Views
        for view in &diff.added_views {
            changes.push(CiChange {
                change_type: "ADD".to_string(),
                object: "VIEW".to_string(),
                table: String::new(),
                name: view.name.clone(),
                description: format!("ADD VIEW {}", view.name),
                is_blocking: false,
                sql: String::new(),
            });
        }
        for view in &diff.removed_views {
            changes.push(CiChange {
                change_type: "DROP".to_string(),
                object: "VIEW".to_string(),
                table: String::new(),
                name: view.name.clone(),
                description: format!("DROP VIEW {}", view.name),
                is_blocking: false,
                sql: String::new(),
            });
        }
        for vd in &diff.modified_views {
            changes.push(CiChange {
                change_type: "ALTER".to_string(),
                object: "VIEW".to_string(),
                table: String::new(),
                name: vd.name.clone(),
                description: format!("ALTER VIEW {}", vd.name),
                is_blocking: false,
                sql: String::new(),
            });
        }

        // Enums
        for e in &diff.added_enums {
            changes.push(CiChange {
                change_type: "ADD".to_string(),
                object: "ENUM".to_string(),
                table: String::new(),
                name: e.name.clone(),
                description: format!("ADD ENUM {}", e.name),
                is_blocking: false,
                sql: String::new(),
            });
        }
        for e in &diff.removed_enums {
            changes.push(CiChange {
                change_type: "DROP".to_string(),
                object: "ENUM".to_string(),
                table: String::new(),
                name: e.name.clone(),
                description: format!("DROP ENUM {}", e.name),
                is_blocking: false,
                sql: String::new(),
            });
        }
        for ed in &diff.modified_enums {
            changes.push(CiChange {
                change_type: "ALTER".to_string(),
                object: "ENUM".to_string(),
                table: String::new(),
                name: ed.name.clone(),
                description: format!("ALTER ENUM {}", ed.name),
                is_blocking: false,
                sql: String::new(),
            });
        }

        // Sequences
        for s in &diff.added_sequences {
            changes.push(CiChange {
                change_type: "ADD".to_string(),
                object: "SEQUENCE".to_string(),
                table: String::new(),
                name: s.name.clone(),
                description: format!("ADD SEQUENCE {}", s.name),
                is_blocking: false,
                sql: String::new(),
            });
        }
        for s in &diff.removed_sequences {
            changes.push(CiChange {
                change_type: "DROP".to_string(),
                object: "SEQUENCE".to_string(),
                table: String::new(),
                name: s.name.clone(),
                description: format!("DROP SEQUENCE {}", s.name),
                is_blocking: false,
                sql: String::new(),
            });
        }

        let blocking: Vec<CiChange> = changes.iter().filter(|c| c.is_blocking).cloned().collect();
        let equal = diff.is_empty();
        let summary = if equal {
            "Schemas are identical".to_string()
        } else {
            format!("{} change(s) detected", changes.len())
        };

        CiReport {
            equal,
            summary,
            changes,
            blocking,
        }
    }

    /// Determine the appropriate exit code.
    pub fn exit_code(&self, fail_on_blocking: bool) -> u8 {
        if fail_on_blocking && !self.blocking.is_empty() {
            EXIT_UNSAFE
        } else if !self.equal {
            EXIT_DRIFT
        } else {
            EXIT_OK
        }
    }

    /// Emit GitHub Actions annotations to stderr.
    pub fn emit_github_annotations(&self) {
        for change in &self.blocking {
            eprintln!("::error title=Blocking migration::{}", change.sql);
        }
        if !self.equal {
            eprintln!("::warning title=Schema drift::{}", self.summary);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diff::diff_schemas;
    use crate::loader::SqlDialect;
    use crate::migration::generate_migration;
    use crate::model::{Column, Schema, Table};

    fn col(name: &str, dtype: &str, nullable: bool, default: Option<&str>) -> Column {
        Column {
            name: name.into(),
            data_type: dtype.into(),
            is_nullable: nullable,
            default: default.map(Into::into),
        }
    }

    #[test]
    fn exit_ok_for_identical_schemas() {
        let schema = Schema::new();
        let diff = diff_schemas(&schema, &schema);
        let stmts = generate_migration(&diff, SqlDialect::Postgres);
        let report = CiReport::from_diff(&diff, &stmts);

        assert!(report.equal);
        assert_eq!(report.exit_code(false), EXIT_OK);
        assert_eq!(report.exit_code(true), EXIT_OK);
        assert!(report.changes.is_empty());
        assert!(report.blocking.is_empty());
    }

    #[test]
    fn exit_drift_for_non_blocking_changes() {
        let mut left = Schema::new();
        let mut t = Table::new("users");
        t.columns
            .insert("id".into(), col("id", "integer", false, None));
        left.tables.insert("users".into(), t);

        let mut right = Schema::new();
        let mut t = Table::new("users");
        t.columns
            .insert("id".into(), col("id", "integer", false, None));
        t.columns
            .insert("bio".into(), col("bio", "text", true, None));
        right.tables.insert("users".into(), t);

        let diff = diff_schemas(&left, &right);
        let stmts = generate_migration(&diff, SqlDialect::Postgres);
        let report = CiReport::from_diff(&diff, &stmts);

        assert!(!report.equal);
        assert_eq!(report.exit_code(false), EXIT_DRIFT);
        assert_eq!(report.exit_code(true), EXIT_DRIFT); // no blocking → still drift, not unsafe
        assert!(report.blocking.is_empty());
    }

    #[test]
    fn exit_unsafe_for_blocking_changes() {
        let mut left = Schema::new();
        let mut t = Table::new("orders");
        t.columns
            .insert("id".into(), col("id", "integer", false, None));
        left.tables.insert("orders".into(), t);

        let mut right = Schema::new();
        let mut t = Table::new("orders");
        t.columns
            .insert("id".into(), col("id", "integer", false, None));
        t.columns.insert(
            "paid_at".into(),
            col("paid_at", "timestamptz", false, Some("now()")),
        );
        right.tables.insert("orders".into(), t);

        let diff = diff_schemas(&left, &right);
        let stmts = generate_migration(&diff, SqlDialect::Postgres);
        let report = CiReport::from_diff(&diff, &stmts);

        assert!(!report.equal);
        assert!(!report.blocking.is_empty());
        assert_eq!(report.exit_code(false), EXIT_DRIFT);
        assert_eq!(report.exit_code(true), EXIT_UNSAFE);
    }

    #[test]
    fn report_summary_counts_changes() {
        let left = Schema::new();
        let mut right = Schema::new();
        let mut t = Table::new("x");
        t.columns.insert("a".into(), col("a", "int", true, None));
        right.tables.insert("x".into(), t);

        let diff = diff_schemas(&left, &right);
        let stmts = generate_migration(&diff, SqlDialect::Postgres);
        let report = CiReport::from_diff(&diff, &stmts);

        assert!(report.summary.contains("1 change(s) detected"));
    }

    #[test]
    fn report_serializes_to_json() {
        let left = Schema::new();
        let mut right = Schema::new();
        let mut t = Table::new("orders");
        t.columns.insert("id".into(), col("id", "int", false, None));
        right.tables.insert("orders".into(), t);

        let diff = diff_schemas(&left, &right);
        let stmts = generate_migration(&diff, SqlDialect::Postgres);
        let report = CiReport::from_diff(&diff, &stmts);

        let json = serde_json::to_string_pretty(&report).unwrap();
        assert!(json.contains("\"equal\": false"));
        assert!(json.contains("\"changes\""));
        assert!(json.contains("\"blocking\""));
    }

    #[test]
    fn report_serializes_to_yaml() {
        let left = Schema::new();
        let mut right = Schema::new();
        let mut t = Table::new("users");
        t.columns.insert("id".into(), col("id", "int", true, None));
        right.tables.insert("users".into(), t);

        let diff = diff_schemas(&left, &right);
        let stmts = generate_migration(&diff, SqlDialect::Postgres);
        let report = CiReport::from_diff(&diff, &stmts);

        let yaml = serde_yaml::to_string(&report).unwrap();
        assert!(yaml.contains("equal: false"));
        assert!(yaml.contains("changes:"));
    }
}
