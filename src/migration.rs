use serde::Serialize;

use crate::diff::{ColumnDiff, SchemaDiff, TableDiff};
use crate::loader::SqlDialect;
use crate::model::{Column, Constraint, ConstraintKind, Index, Table};

/// Generate migration SQL statements from a schema diff.
///
/// Statements are ordered for safe execution:
/// 1. DROP CONSTRAINTs
/// 2. DROP/REPLACE VIEWs (before column drops — views may reference dropped columns)
/// 3. DROP INDEXes
/// 4. DROP COLUMNs
/// 5. DROP removed VIEWs / DROP TABLEs / DROP TYPEs / DROP SEQUENCEs
/// 6. CREATE ENUMs / ALTER ENUMs
/// 7. CREATE SEQUENCEs
/// 8. CREATE TABLEs + ADD CONSTRAINTs for new tables
/// 9. ADD COLUMNs
/// 10. ALTER COLUMNs
/// 11. CREATE INDEXes
/// 12. ADD CONSTRAINTs for modified tables
/// 13. CREATE new VIEWs
pub fn generate_migration(diff: &SchemaDiff, dialect: SqlDialect) -> Vec<MigrationStatement> {
    let mut statements = Vec::new();

    // Phase 1: DROP CONSTRAINTs from modified tables
    for table_diff in &diff.modified_tables {
        for c in &table_diff.removed_constraints {
            statements.push(MigrationStatement {
                sql: drop_constraint_sql(c, dialect),
                warnings: Vec::new(),
                is_blocking: true, // DROP CONSTRAINT acquires AccessExclusiveLock
            });
        }
    }

    // Phase 2: DROP modified views before column changes (views may reference dropped columns;
    // they will be recreated with new definition after columns are added in Phase 13)
    for vd in &diff.modified_views {
        statements.push(MigrationStatement {
            sql: format!("DROP VIEW IF EXISTS {};", quote_ident(&vd.name, dialect)),
            warnings: Vec::new(),
            is_blocking: false,
        });
    }

    // Phase 3: DROP INDEXes from modified tables
    for table_diff in &diff.modified_tables {
        for idx in &table_diff.removed_indexes {
            statements.push(MigrationStatement {
                sql: drop_index_sql(idx, dialect),
                warnings: Vec::new(),
                is_blocking: true, // DROP INDEX acquires AccessExclusiveLock
            });
        }
    }

    // Phase 3: DROP COLUMNs from modified tables
    for table_diff in &diff.modified_tables {
        for col in &table_diff.removed_columns {
            let mut warnings = vec![
                "DROP COLUMN is destructive and cannot be undone. Consider renaming first."
                    .to_string(),
            ];
            if !col.is_nullable && col.default.is_none() {
                warnings.push(
                    "This column has NOT NULL constraint — dropping it may affect application code."
                        .to_string(),
                );
            }
            statements.push(MigrationStatement {
                sql: format!(
                    "ALTER TABLE {} DROP COLUMN {};",
                    quote_ident(&table_diff.table_name, dialect),
                    quote_ident(&col.name, dialect)
                ),
                warnings,
                is_blocking: true, // DROP COLUMN acquires AccessExclusiveLock
            });
        }
    }

    // Phase 4: DROP removed views (before tables, since views may depend on tables)
    for view in &diff.removed_views {
        statements.push(MigrationStatement {
            sql: format!("DROP VIEW {};", quote_ident(&view.name, dialect)),
            warnings: Vec::new(),
            is_blocking: false,
        });
    }

    // Phase 5: DROP TABLEs
    for table in &diff.removed_tables {
        statements.push(MigrationStatement {
            sql: format!("DROP TABLE {};", quote_ident(&table.name, dialect)),
            warnings: vec![format!(
                "Dropping table '{}' will permanently delete all data.",
                table.name
            )],
            is_blocking: true, // DROP TABLE acquires AccessExclusiveLock
        });
    }
    for e in &diff.removed_enums {
        statements.push(MigrationStatement {
            sql: format!("DROP TYPE {};", quote_ident(&e.name, dialect)),
            warnings: vec![format!(
                "Dropping enum type '{}' will fail if any column still uses it.",
                e.name
            )],
            is_blocking: false,
        });
    }
    for s in &diff.removed_sequences {
        statements.push(MigrationStatement {
            sql: format!("DROP SEQUENCE {};", quote_ident(&s.name, dialect)),
            warnings: Vec::new(),
            is_blocking: false,
        });
    }

    // Phase 6: CREATE/ALTER enums
    for e in &diff.added_enums {
        let values: Vec<String> = e.values.iter().map(|v| format!("'{v}'")).collect();
        statements.push(MigrationStatement {
            sql: format!(
                "CREATE TYPE {} AS ENUM ({});",
                quote_ident(&e.name, dialect),
                values.join(", ")
            ),
            warnings: Vec::new(),
            is_blocking: false,
        });
    }
    for ed in &diff.modified_enums {
        if ed.reordered {
            statements.push(MigrationStatement {
                sql: format!(
                    "-- enum type {} has values in a different order",
                    quote_ident(&ed.name, dialect)
                ),
                warnings: vec![format!(
                    "Enum '{}' has the same values but in a different order. \
                     PostgreSQL does not support reordering enum values. \
                     Recreate the type manually if ordering matters for comparisons.",
                    ed.name
                )],
                is_blocking: false,
            });
            continue;
        }
        for val in &ed.added_values {
            statements.push(MigrationStatement {
                sql: format!(
                    "ALTER TYPE {} ADD VALUE '{}';",
                    quote_ident(&ed.name, dialect),
                    val
                ),
                warnings: Vec::new(),
                is_blocking: false,
            });
        }
        if !ed.removed_values.is_empty() {
            statements.push(MigrationStatement {
                sql: format!(
                    "-- cannot remove values from enum type {}",
                    quote_ident(&ed.name, dialect)
                ),
                warnings: vec![format!(
                    "PostgreSQL does not support removing enum values. \
                     Removed values: {}. Recreate the type manually.",
                    ed.removed_values.join(", ")
                )],
                is_blocking: false,
            });
        }
    }

    // Phase 7: CREATE sequences
    for s in &diff.added_sequences {
        statements.push(MigrationStatement {
            sql: format!(
                "CREATE SEQUENCE {} AS {} START {} INCREMENT {} MINVALUE {} MAXVALUE {};",
                quote_ident(&s.name, dialect),
                s.data_type,
                s.start_value,
                s.increment,
                s.min_value,
                s.max_value
            ),
            warnings: Vec::new(),
            is_blocking: false,
        });
    }
    for sd in &diff.modified_sequences {
        let s = &sd.new;
        statements.push(MigrationStatement {
            sql: format!(
                "ALTER SEQUENCE {} AS {} START WITH {} INCREMENT BY {} MINVALUE {} MAXVALUE {};",
                quote_ident(&s.name, dialect),
                s.data_type,
                s.start_value,
                s.increment,
                s.min_value,
                s.max_value
            ),
            warnings: Vec::new(),
            is_blocking: false,
        });
    }

    // Phase 8: CREATE TABLEs (without constraints — those come after all tables exist)
    for table in &diff.added_tables {
        statements.push(MigrationStatement {
            sql: create_table_sql(table, dialect),
            warnings: Vec::new(),
            is_blocking: false, // CREATE TABLE on new table is non-blocking
        });

        for idx in table.indexes.values() {
            statements.push(MigrationStatement {
                sql: create_index_sql(idx, dialect),
                warnings: Vec::new(),
                is_blocking: false, // Index on brand-new table is non-blocking
            });
        }
    }

    // Phase 8b: ADD CONSTRAINTs for new tables (after ALL tables exist, so FK references resolve)
    for table in &diff.added_tables {
        for c in table.constraints.values() {
            statements.push(MigrationStatement {
                sql: add_constraint_sql(c, dialect),
                warnings: Vec::new(),
                is_blocking: false,
            });
        }
    }

    // Phase 9: ADD COLUMNs
    for table_diff in &diff.modified_tables {
        for col in &table_diff.added_columns {
            let warnings = add_column_warnings(col);
            // ADD COLUMN ... NOT NULL requires table rewrite / AccessExclusiveLock
            let blocking = !col.is_nullable;
            statements.push(MigrationStatement {
                sql: format!(
                    "ALTER TABLE {} ADD COLUMN {};",
                    quote_ident(&table_diff.table_name, dialect),
                    column_definition_sql(col, dialect)
                ),
                warnings,
                is_blocking: blocking,
            });
        }
    }

    // Phase 10: ALTER COLUMNs
    for table_diff in &diff.modified_tables {
        let mut alter_stmts = generate_column_alterations(table_diff, dialect);
        statements.append(&mut alter_stmts);
    }

    // Phase 11: CREATE INDEXes on modified tables
    for table_diff in &diff.modified_tables {
        for idx in &table_diff.added_indexes {
            statements.push(MigrationStatement {
                sql: create_index_sql(idx, dialect),
                warnings: vec![
                    "Consider using CREATE INDEX CONCURRENTLY to avoid locking the table."
                        .to_string(),
                ],
                is_blocking: true, // CREATE INDEX (without CONCURRENTLY) blocks writes
            });
        }
    }

    // Phase 12: ADD CONSTRAINTs on modified tables
    for table_diff in &diff.modified_tables {
        for c in &table_diff.added_constraints {
            statements.push(MigrationStatement {
                sql: add_constraint_sql(c, dialect),
                warnings: Vec::new(),
                is_blocking: false,
            });
        }
    }

    // Phase 13: CREATE/REPLACE VIEWs
    for view in &diff.added_views {
        statements.push(MigrationStatement {
            sql: format!(
                "CREATE VIEW {} AS {};",
                quote_ident(&view.name, dialect),
                view.definition
            ),
            warnings: Vec::new(),
            is_blocking: false,
        });
    }
    // Phase 13b: Recreate modified views with new definition (dropped in Phase 2)
    for vd in &diff.modified_views {
        statements.push(MigrationStatement {
            sql: format!(
                "CREATE VIEW {} AS {};",
                quote_ident(&vd.name, dialect),
                vd.new_definition
            ),
            warnings: Vec::new(),
            is_blocking: false,
        });
    }

    statements
}

/// Generate rollback (DOWN) migration that reverses the diff.
///
/// Order: drop added views -> drop added constraints/indexes ->
/// drop added columns -> drop added tables -> drop added enums/sequences ->
/// recreate removed enums/sequences -> recreate removed tables ->
/// re-add removed columns -> recreate removed constraints/indexes ->
/// recreate removed views
pub fn generate_rollback(diff: &SchemaDiff, dialect: SqlDialect) -> Vec<MigrationStatement> {
    let mut statements = Vec::new();

    // 1. Drop views that were added
    for view in &diff.added_views {
        statements.push(MigrationStatement {
            sql: format!("DROP VIEW {};", quote_ident(&view.name, dialect)),
            warnings: Vec::new(),
            is_blocking: false,
        });
    }

    // (Modified views are restored later, after columns are re-added)

    // 2. Drop added constraints
    for table_diff in &diff.modified_tables {
        for c in &table_diff.added_constraints {
            statements.push(MigrationStatement {
                sql: drop_constraint_sql(c, dialect),
                warnings: Vec::new(),
                is_blocking: true,
            });
        }
    }

    // 6. Drop added indexes
    for table_diff in &diff.modified_tables {
        for idx in &table_diff.added_indexes {
            statements.push(MigrationStatement {
                sql: drop_index_sql(idx, dialect),
                warnings: Vec::new(),
                is_blocking: true,
            });
        }
    }

    // 7. Drop added columns
    for table_diff in &diff.modified_tables {
        for col in &table_diff.added_columns {
            statements.push(MigrationStatement {
                sql: format!(
                    "ALTER TABLE {} DROP COLUMN {};",
                    quote_ident(&table_diff.table_name, dialect),
                    quote_ident(&col.name, dialect)
                ),
                warnings: vec!["Rollback: dropping column that was added.".into()],
                is_blocking: true,
            });
        }
    }

    // 8. Drop added tables (before enums/sequences, since tables may depend on them)
    for table in &diff.added_tables {
        statements.push(MigrationStatement {
            sql: format!("DROP TABLE {};", quote_ident(&table.name, dialect)),
            warnings: Vec::new(),
            is_blocking: true,
        });
    }

    // 9. Drop added enums (after tables that use them are dropped)
    for e in &diff.added_enums {
        statements.push(MigrationStatement {
            sql: format!("DROP TYPE {};", quote_ident(&e.name, dialect)),
            warnings: Vec::new(),
            is_blocking: false,
        });
    }

    // 10. Drop added sequences (after tables that use them are dropped)
    for s in &diff.added_sequences {
        statements.push(MigrationStatement {
            sql: format!("DROP SEQUENCE {};", quote_ident(&s.name, dialect)),
            warnings: Vec::new(),
            is_blocking: false,
        });
    }

    // 11. Recreate removed enums (before tables that may use them)
    for e in &diff.removed_enums {
        let values: Vec<String> = e.values.iter().map(|v| format!("'{v}'")).collect();
        statements.push(MigrationStatement {
            sql: format!(
                "CREATE TYPE {} AS ENUM ({});",
                quote_ident(&e.name, dialect),
                values.join(", ")
            ),
            warnings: Vec::new(),
            is_blocking: false,
        });
    }

    // 12. Reverse modified enums (cannot remove added values in PG, note only)
    for ed in &diff.modified_enums {
        if !ed.added_values.is_empty() {
            statements.push(MigrationStatement {
                sql: format!(
                    "-- cannot remove values from enum type {}",
                    quote_ident(&ed.name, dialect)
                ),
                warnings: vec![format!(
                    "Rollback cannot remove enum values added to '{}'. Manual intervention needed.",
                    ed.name
                )],
                is_blocking: false,
            });
        }
        for val in &ed.removed_values {
            statements.push(MigrationStatement {
                sql: format!(
                    "ALTER TYPE {} ADD VALUE '{}';",
                    quote_ident(&ed.name, dialect),
                    val
                ),
                warnings: Vec::new(),
                is_blocking: false,
            });
        }
    }

    // 13. Recreate removed sequences
    for s in &diff.removed_sequences {
        statements.push(MigrationStatement {
            sql: format!(
                "CREATE SEQUENCE {} AS {} START {} INCREMENT {} MINVALUE {} MAXVALUE {};",
                quote_ident(&s.name, dialect),
                s.data_type,
                s.start_value,
                s.increment,
                s.min_value,
                s.max_value
            ),
            warnings: Vec::new(),
            is_blocking: false,
        });
    }

    // 14. Reverse modified sequences
    for sd in &diff.modified_sequences {
        let s = &sd.old;
        statements.push(MigrationStatement {
            sql: format!(
                "ALTER SEQUENCE {} AS {} START WITH {} INCREMENT BY {} MINVALUE {} MAXVALUE {};",
                quote_ident(&s.name, dialect),
                s.data_type,
                s.start_value,
                s.increment,
                s.min_value,
                s.max_value
            ),
            warnings: Vec::new(),
            is_blocking: false,
        });
    }

    // 15. Recreate removed tables with their indexes and constraints
    for table in &diff.removed_tables {
        statements.push(MigrationStatement {
            sql: create_table_sql(table, dialect),
            warnings: vec![
                "Rollback recreates the table structure, but data is permanently lost.".into(),
            ],
            is_blocking: false,
        });
        for idx in table.indexes.values() {
            statements.push(MigrationStatement {
                sql: create_index_sql(idx, dialect),
                warnings: Vec::new(),
                is_blocking: false,
            });
        }
        for c in table.constraints.values() {
            statements.push(MigrationStatement {
                sql: add_constraint_sql(c, dialect),
                warnings: Vec::new(),
                is_blocking: false,
            });
        }
    }

    // 16. Re-add removed columns (data is lost)
    for table_diff in &diff.modified_tables {
        for col in &table_diff.removed_columns {
            statements.push(MigrationStatement {
                sql: format!(
                    "ALTER TABLE {} ADD COLUMN {};",
                    quote_ident(&table_diff.table_name, dialect),
                    column_definition_sql(col, dialect)
                ),
                warnings: vec![
                    "Rollback re-adds the column, but original data is permanently lost.".into(),
                ],
                is_blocking: false,
            });
        }
    }

    // 17. Re-add removed constraints
    for table_diff in &diff.modified_tables {
        for c in &table_diff.removed_constraints {
            statements.push(MigrationStatement {
                sql: add_constraint_sql(c, dialect),
                warnings: Vec::new(),
                is_blocking: true,
            });
        }
    }

    // 16. Recreate removed indexes
    for table_diff in &diff.modified_tables {
        for idx in &table_diff.removed_indexes {
            statements.push(MigrationStatement {
                sql: create_index_sql(idx, dialect),
                warnings: Vec::new(),
                is_blocking: true,
            });
        }
    }

    // 17. Revert modified views (after columns are restored)
    for vd in &diff.modified_views {
        statements.push(MigrationStatement {
            sql: format!(
                "CREATE OR REPLACE VIEW {} AS {};",
                quote_ident(&vd.name, dialect),
                vd.old_definition
            ),
            warnings: Vec::new(),
            is_blocking: false,
        });
    }

    // 18. Recreate removed views (after tables are back)
    for view in &diff.removed_views {
        statements.push(MigrationStatement {
            sql: format!(
                "CREATE VIEW {} AS {};",
                quote_ident(&view.name, dialect),
                view.definition
            ),
            warnings: Vec::new(),
            is_blocking: false,
        });
    }

    statements
}

/// A single migration SQL statement with optional safety warnings.
#[derive(Debug, Clone, Serialize)]
pub struct MigrationStatement {
    pub sql: String,
    pub warnings: Vec<String>,
    /// Whether this statement acquires heavy locks (e.g. AccessExclusiveLock)
    /// or performs a full table rewrite.
    pub is_blocking: bool,
}

fn quote_ident(name: &str, dialect: SqlDialect) -> String {
    let needs_quoting = name.is_empty()
        || !name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
    if needs_quoting {
        match dialect {
            SqlDialect::MySql => format!("`{}`", name.replace('`', "``")),
            _ => format!("\"{}\"", name.replace('"', "\"\"")),
        }
    } else {
        name.to_string()
    }
}

fn column_definition_sql(col: &Column, dialect: SqlDialect) -> String {
    let mut def = format!("{} {}", quote_ident(&col.name, dialect), col.data_type);
    if !col.is_nullable {
        def.push_str(" NOT NULL");
    }
    if let Some(ref default) = col.default {
        def.push_str(&format!(" DEFAULT {default}"));
    }
    def
}

fn create_table_sql(table: &Table, dialect: SqlDialect) -> String {
    let columns: Vec<String> = table
        .columns
        .values()
        .map(|c| format!("    {}", column_definition_sql(c, dialect)))
        .collect();
    format!(
        "CREATE TABLE {} (\n{}\n);",
        quote_ident(&table.name, dialect),
        columns.join(",\n")
    )
}

fn create_index_sql(idx: &Index, dialect: SqlDialect) -> String {
    let unique = if idx.is_unique { "UNIQUE " } else { "" };
    // PostgreSQL index columns are raw SQL clauses from pg_get_indexdef() and may
    // contain expressions (lower(email)), sort orders (created_at DESC), or
    // already-quoted identifiers — they must NOT be wrapped with quote_ident.
    // MySQL/SQLite index columns are plain identifier names from information_schema
    // and need proper quoting.
    let cols = match dialect {
        SqlDialect::Postgres | SqlDialect::SqlFile | SqlDialect::Snapshot => idx.columns.join(", "),
        _ => idx
            .columns
            .iter()
            .map(|c| quote_ident(c, dialect))
            .collect::<Vec<_>>()
            .join(", "),
    };
    format!(
        "CREATE {unique}INDEX {} ON {}({});",
        quote_ident(&idx.name, dialect),
        quote_ident(&idx.table_name, dialect),
        cols
    )
}

fn drop_index_sql(idx: &Index, dialect: SqlDialect) -> String {
    match dialect {
        SqlDialect::MySql => format!(
            "DROP INDEX {} ON {};",
            quote_ident(&idx.name, dialect),
            quote_ident(&idx.table_name, dialect)
        ),
        _ => format!("DROP INDEX {};", quote_ident(&idx.name, dialect)),
    }
}

fn add_constraint_sql(c: &Constraint, dialect: SqlDialect) -> String {
    // SQLite does not support ALTER TABLE ADD CONSTRAINT
    if dialect == SqlDialect::Sqlite {
        return format!(
            "-- manual migration required: add constraint {} on {}.{} \
             (SQLite does not support ALTER TABLE ADD CONSTRAINT; recreate the table)",
            c.name,
            c.table_name,
            c.definition()
        );
    }
    let table = quote_ident(&c.table_name, dialect);
    let name = quote_ident(&c.name, dialect);
    match &c.kind {
        ConstraintKind::ForeignKey {
            columns,
            ref_table,
            ref_columns,
            on_delete,
            on_update,
        } => {
            let cols: Vec<String> = columns.iter().map(|c| quote_ident(c, dialect)).collect();
            let refs: Vec<String> = ref_columns
                .iter()
                .map(|c| quote_ident(c, dialect))
                .collect();
            let mut sql = format!(
                "ALTER TABLE {table} ADD CONSTRAINT {name} FOREIGN KEY ({}) REFERENCES {}({})",
                cols.join(", "),
                quote_ident(ref_table, dialect),
                refs.join(", ")
            );
            if let Some(action) = on_delete {
                sql.push_str(&format!(" ON DELETE {action}"));
            }
            if let Some(action) = on_update {
                sql.push_str(&format!(" ON UPDATE {action}"));
            }
            sql.push(';');
            sql
        }
        ConstraintKind::Unique { columns } => {
            let cols: Vec<String> = columns.iter().map(|c| quote_ident(c, dialect)).collect();
            format!(
                "ALTER TABLE {table} ADD CONSTRAINT {name} UNIQUE ({});",
                cols.join(", ")
            )
        }
        ConstraintKind::Check { expression } => {
            format!("ALTER TABLE {table} ADD CONSTRAINT {name} CHECK ({expression});")
        }
    }
}

fn drop_constraint_sql(c: &Constraint, dialect: SqlDialect) -> String {
    match dialect {
        SqlDialect::MySql => {
            // MySQL uses DROP FOREIGN KEY for FKs, DROP INDEX for unique constraints
            match &c.kind {
                ConstraintKind::ForeignKey { .. } => format!(
                    "ALTER TABLE {} DROP FOREIGN KEY {};",
                    quote_ident(&c.table_name, dialect),
                    quote_ident(&c.name, dialect)
                ),
                ConstraintKind::Unique { .. } => format!(
                    "ALTER TABLE {} DROP INDEX {};",
                    quote_ident(&c.table_name, dialect),
                    quote_ident(&c.name, dialect)
                ),
                ConstraintKind::Check { .. } => format!(
                    "ALTER TABLE {} DROP CHECK {};",
                    quote_ident(&c.table_name, dialect),
                    quote_ident(&c.name, dialect)
                ),
            }
        }
        _ => format!(
            "ALTER TABLE {} DROP CONSTRAINT {};",
            quote_ident(&c.table_name, dialect),
            quote_ident(&c.name, dialect)
        ),
    }
}

fn add_column_warnings(col: &Column) -> Vec<String> {
    let mut warnings = Vec::new();
    if !col.is_nullable && col.default.is_none() {
        warnings.push(format!(
            "Adding NOT NULL column '{}' without DEFAULT will fail on non-empty tables. \
             Consider: ADD COLUMN ... DEFAULT NULL first, then backfill, then SET NOT NULL.",
            col.name
        ));
    }
    if !col.is_nullable && col.default.is_some() {
        warnings.push(format!(
            "On PostgreSQL < 11, adding NOT NULL column '{}' with DEFAULT will rewrite the \
             entire table and acquire AccessExclusiveLock.",
            col.name
        ));
    }
    warnings
}

fn generate_column_alterations(
    table_diff: &TableDiff,
    dialect: SqlDialect,
) -> Vec<MigrationStatement> {
    let mut stmts = Vec::new();

    for col_diff in &table_diff.modified_columns {
        let ColumnDiff { old, new } = col_diff;
        let table = &table_diff.table_name;

        // Type change — blocking: requires table rewrite / AccessExclusiveLock
        if old.data_type != new.data_type {
            match dialect {
                SqlDialect::MySql => stmts.push(MigrationStatement {
                    sql: format!(
                        "ALTER TABLE {} MODIFY COLUMN {};",
                        quote_ident(table, dialect),
                        column_definition_sql(new, dialect)
                    ),
                    warnings: vec![format!(
                        "Changing column type from '{}' to '{}' may require a table rewrite \
                         and table lock.",
                        old.data_type, new.data_type
                    )],
                    is_blocking: true,
                }),
                SqlDialect::Sqlite => stmts.push(MigrationStatement {
                    sql: format!(
                        "-- manual migration required for type change on {}.{}",
                        quote_ident(table, dialect),
                        quote_ident(&new.name, dialect)
                    ),
                    warnings: vec![format!(
                        "SQLite does not support ALTER COLUMN TYPE directly for '{}'. \
                         Recreate table '{table}' with the desired column definition.",
                        new.name
                    )],
                    is_blocking: true,
                }),
                _ => stmts.push(MigrationStatement {
                    sql: format!(
                        "ALTER TABLE {} ALTER COLUMN {} TYPE {};",
                        quote_ident(table, dialect),
                        quote_ident(&new.name, dialect),
                        new.data_type
                    ),
                    warnings: vec![format!(
                        "Changing column type from '{}' to '{}' may require a table rewrite \
                         and AccessExclusiveLock.",
                        old.data_type, new.data_type
                    )],
                    is_blocking: true,
                }),
            }
        }

        // Nullability change
        if old.is_nullable != new.is_nullable {
            // SET NOT NULL is blocking (full table scan + AccessExclusiveLock)
            // DROP NOT NULL is non-blocking
            let blocking = !new.is_nullable;
            match dialect {
                SqlDialect::MySql => {
                    let warning = if new.is_nullable {
                        format!(
                            "Changing '{}' to NULL via MODIFY COLUMN may rebuild the table depending \
                             on MySQL/MariaDB version and storage engine.",
                            new.name
                        )
                    } else {
                        format!(
                            "Changing '{}' to NOT NULL via MODIFY COLUMN can fail if existing rows \
                             contain NULL values.",
                            new.name
                        )
                    };
                    stmts.push(MigrationStatement {
                        sql: format!(
                            "ALTER TABLE {} MODIFY COLUMN {};",
                            quote_ident(table, dialect),
                            column_definition_sql(new, dialect)
                        ),
                        warnings: vec![warning],
                        is_blocking: blocking,
                    });
                }
                SqlDialect::Sqlite => {
                    stmts.push(MigrationStatement {
                        sql: format!(
                            "-- manual migration required for nullability change on {}.{}",
                            quote_ident(table, dialect),
                            quote_ident(&new.name, dialect)
                        ),
                        warnings: vec![format!(
                            "SQLite does not support ALTER COLUMN nullability directly for '{}'. \
                             Recreate table '{table}' with the desired column definition.",
                            new.name
                        )],
                        is_blocking: blocking,
                    });
                }
                _ => {
                    if new.is_nullable {
                        stmts.push(MigrationStatement {
                            sql: format!(
                                "ALTER TABLE {} ALTER COLUMN {} DROP NOT NULL;",
                                quote_ident(table, dialect),
                                quote_ident(&new.name, dialect)
                            ),
                            warnings: Vec::new(),
                            is_blocking: false,
                        });
                    } else {
                        stmts.push(MigrationStatement {
                            sql: format!(
                                "ALTER TABLE {} ALTER COLUMN {} SET NOT NULL;",
                                quote_ident(table, dialect),
                                quote_ident(&new.name, dialect)
                            ),
                            warnings: vec![format!(
                                "SET NOT NULL on '{}' will scan the entire table to verify no NULLs exist. \
                                 This acquires AccessExclusiveLock.",
                                new.name
                            )],
                            is_blocking: true,
                        });
                    }
                }
            }
        }

        // Default change — non-blocking (metadata-only on modern PG)
        if old.default != new.default {
            match &new.default {
                Some(default) => {
                    stmts.push(MigrationStatement {
                        sql: format!(
                            "ALTER TABLE {} ALTER COLUMN {} SET DEFAULT {default};",
                            quote_ident(table, dialect),
                            quote_ident(&new.name, dialect)
                        ),
                        warnings: Vec::new(),
                        is_blocking: false,
                    });
                }
                None => {
                    stmts.push(MigrationStatement {
                        sql: format!(
                            "ALTER TABLE {} ALTER COLUMN {} DROP DEFAULT;",
                            quote_ident(table, dialect),
                            quote_ident(&new.name, dialect)
                        ),
                        warnings: Vec::new(),
                        is_blocking: false,
                    });
                }
            }
        }
    }

    stmts
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diff::diff_schemas;
    use crate::model::{Column, Index, Schema, Table};

    fn col(name: &str, dtype: &str, nullable: bool, default: Option<&str>) -> Column {
        Column {
            name: name.into(),
            data_type: dtype.into(),
            is_nullable: nullable,
            default: default.map(Into::into),
        }
    }

    #[test]
    fn add_column_generates_alter() {
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
            .insert("email".into(), col("email", "varchar(255)", false, None));
        right.tables.insert("users".into(), t);

        let diff = diff_schemas(&left, &right);
        let stmts = generate_migration(&diff, SqlDialect::Postgres);

        assert_eq!(stmts.len(), 1);
        assert_eq!(
            stmts[0].sql,
            "ALTER TABLE users ADD COLUMN email varchar(255) NOT NULL;"
        );
        assert!(!stmts[0].warnings.is_empty()); // NOT NULL without DEFAULT warning
    }

    #[test]
    fn drop_column_generates_alter_with_warning() {
        let mut left = Schema::new();
        let mut t = Table::new("users");
        t.columns
            .insert("id".into(), col("id", "integer", false, None));
        t.columns
            .insert("old_field".into(), col("old_field", "text", true, None));
        left.tables.insert("users".into(), t);

        let mut right = Schema::new();
        let mut t = Table::new("users");
        t.columns
            .insert("id".into(), col("id", "integer", false, None));
        right.tables.insert("users".into(), t);

        let diff = diff_schemas(&left, &right);
        let stmts = generate_migration(&diff, SqlDialect::Postgres);

        assert_eq!(stmts.len(), 1);
        assert_eq!(stmts[0].sql, "ALTER TABLE users DROP COLUMN old_field;");
        assert!(stmts[0].warnings[0].contains("destructive"));
    }

    #[test]
    fn new_table_generates_create() {
        let left = Schema::new();

        let mut right = Schema::new();
        let mut t = Table::new("orders");
        t.columns
            .insert("id".into(), col("id", "serial", false, None));
        t.columns
            .insert("total".into(), col("total", "numeric(10,2)", false, None));
        right.tables.insert("orders".into(), t);

        let diff = diff_schemas(&left, &right);
        let stmts = generate_migration(&diff, SqlDialect::Postgres);

        assert_eq!(stmts.len(), 1);
        assert!(stmts[0].sql.starts_with("CREATE TABLE orders"));
        assert!(stmts[0].sql.contains("id serial NOT NULL"));
        assert!(stmts[0].sql.contains("total numeric(10,2) NOT NULL"));
    }

    #[test]
    fn type_change_generates_alter_type() {
        let mut left = Schema::new();
        let mut t = Table::new("users");
        t.columns
            .insert("email".into(), col("email", "varchar(100)", false, None));
        left.tables.insert("users".into(), t);

        let mut right = Schema::new();
        let mut t = Table::new("users");
        t.columns
            .insert("email".into(), col("email", "varchar(255)", false, None));
        right.tables.insert("users".into(), t);

        let diff = diff_schemas(&left, &right);
        let stmts = generate_migration(&diff, SqlDialect::Postgres);

        assert_eq!(stmts.len(), 1);
        assert_eq!(
            stmts[0].sql,
            "ALTER TABLE users ALTER COLUMN email TYPE varchar(255);"
        );
    }

    #[test]
    fn index_operations() {
        let mut left = Schema::new();
        let mut t = Table::new("orders");
        t.columns
            .insert("id".into(), col("id", "integer", false, None));
        left.tables.insert("orders".into(), t);

        let mut right = Schema::new();
        let mut t = Table::new("orders");
        t.columns
            .insert("id".into(), col("id", "integer", false, None));
        t.indexes.insert(
            "idx_orders_id".into(),
            Index {
                name: "idx_orders_id".into(),
                table_name: "orders".into(),
                columns: vec!["id".into()],
                is_unique: false,
            },
        );
        right.tables.insert("orders".into(), t);

        let diff = diff_schemas(&left, &right);
        let stmts = generate_migration(&diff, SqlDialect::Postgres);

        assert_eq!(stmts.len(), 1);
        assert_eq!(stmts[0].sql, "CREATE INDEX idx_orders_id ON orders(id);");
    }

    #[test]
    fn migration_ordering() {
        // Complex scenario: drop table + add table + modify columns + add index
        let mut left = Schema::new();
        let mut legacy = Table::new("legacy");
        legacy
            .columns
            .insert("id".into(), col("id", "integer", false, None));
        left.tables.insert("legacy".into(), legacy);

        let mut users = Table::new("users");
        users
            .columns
            .insert("id".into(), col("id", "integer", false, None));
        users
            .columns
            .insert("old_col".into(), col("old_col", "text", true, None));
        left.tables.insert("users".into(), users);

        let mut right = Schema::new();
        let mut users = Table::new("users");
        users
            .columns
            .insert("id".into(), col("id", "integer", false, None));
        users
            .columns
            .insert("new_col".into(), col("new_col", "text", true, None));
        right.tables.insert("users".into(), users);

        let mut orders = Table::new("orders");
        orders
            .columns
            .insert("id".into(), col("id", "serial", false, None));
        right.tables.insert("orders".into(), orders);

        let diff = diff_schemas(&left, &right);
        let stmts = generate_migration(&diff, SqlDialect::Postgres);

        let sqls: Vec<&str> = stmts.iter().map(|s| s.sql.as_str()).collect();

        // DROP COLUMN before DROP TABLE before CREATE TABLE before ADD COLUMN
        let drop_col_pos = sqls.iter().position(|s| s.contains("DROP COLUMN")).unwrap();
        let drop_table_pos = sqls.iter().position(|s| s.contains("DROP TABLE")).unwrap();
        let create_table_pos = sqls
            .iter()
            .position(|s| s.contains("CREATE TABLE"))
            .unwrap();
        let add_col_pos = sqls.iter().position(|s| s.contains("ADD COLUMN")).unwrap();

        assert!(drop_col_pos < drop_table_pos);
        assert!(drop_table_pos < create_table_pos);
        assert!(create_table_pos < add_col_pos);
    }

    #[test]
    fn mysql_drop_index_uses_on_clause() {
        let mut left = Schema::new();
        let mut t = Table::new("orders");
        t.columns
            .insert("id".into(), col("id", "integer", false, None));
        t.indexes.insert(
            "idx_orders_id".into(),
            Index {
                name: "idx_orders_id".into(),
                table_name: "orders".into(),
                columns: vec!["id".into()],
                is_unique: false,
            },
        );
        left.tables.insert("orders".into(), t);

        let mut right = Schema::new();
        let mut t = Table::new("orders");
        t.columns
            .insert("id".into(), col("id", "integer", false, None));
        right.tables.insert("orders".into(), t);

        let diff = diff_schemas(&left, &right);
        let stmts = generate_migration(&diff, SqlDialect::MySql);

        assert_eq!(stmts[0].sql, "DROP INDEX idx_orders_id ON orders;");
    }

    #[test]
    fn sqlite_type_change_generates_manual_warning() {
        let mut left = Schema::new();
        let mut t = Table::new("users");
        t.columns
            .insert("email".into(), col("email", "text", false, None));
        left.tables.insert("users".into(), t);

        let mut right = Schema::new();
        let mut t = Table::new("users");
        t.columns
            .insert("email".into(), col("email", "varchar(255)", false, None));
        right.tables.insert("users".into(), t);

        let diff = diff_schemas(&left, &right);
        let stmts = generate_migration(&diff, SqlDialect::Sqlite);

        assert!(stmts[0].sql.starts_with("-- manual migration required"));
        assert!(stmts[0].warnings[0].contains("does not support ALTER COLUMN TYPE"));
    }

    #[test]
    fn mysql_nullability_change_uses_modify_column() {
        let mut left = Schema::new();
        let mut t = Table::new("users");
        t.columns
            .insert("email".into(), col("email", "varchar(255)", true, None));
        left.tables.insert("users".into(), t);

        let mut right = Schema::new();
        let mut t = Table::new("users");
        t.columns
            .insert("email".into(), col("email", "varchar(255)", false, None));
        right.tables.insert("users".into(), t);

        let diff = diff_schemas(&left, &right);
        let stmts = generate_migration(&diff, SqlDialect::MySql);

        assert_eq!(stmts.len(), 1);
        assert_eq!(
            stmts[0].sql,
            "ALTER TABLE users MODIFY COLUMN email varchar(255) NOT NULL;"
        );
    }

    #[test]
    fn sqlite_nullability_change_generates_manual_warning() {
        let mut left = Schema::new();
        let mut t = Table::new("users");
        t.columns
            .insert("email".into(), col("email", "text", true, None));
        left.tables.insert("users".into(), t);

        let mut right = Schema::new();
        let mut t = Table::new("users");
        t.columns
            .insert("email".into(), col("email", "text", false, None));
        right.tables.insert("users".into(), t);

        let diff = diff_schemas(&left, &right);
        let stmts = generate_migration(&diff, SqlDialect::Sqlite);

        assert_eq!(stmts.len(), 1);
        assert!(stmts[0]
            .sql
            .starts_with("-- manual migration required for nullability change"));
        assert!(stmts[0].warnings[0].contains("does not support ALTER COLUMN nullability"));
    }

    #[test]
    fn expression_index_columns_are_not_quoted() {
        let mut left = Schema::new();
        let mut t = Table::new("users");
        t.columns
            .insert("id".into(), col("id", "integer", false, None));
        t.columns
            .insert("email".into(), col("email", "text", false, None));
        t.columns.insert(
            "created_at".into(),
            col("created_at", "timestamptz", false, None),
        );
        left.tables.insert("users".into(), t);

        let mut right = Schema::new();
        let mut t = Table::new("users");
        t.columns
            .insert("id".into(), col("id", "integer", false, None));
        t.columns
            .insert("email".into(), col("email", "text", false, None));
        t.columns.insert(
            "created_at".into(),
            col("created_at", "timestamptz", false, None),
        );
        // Expression index: lower(email)
        t.indexes.insert(
            "idx_users_email_lower".into(),
            Index {
                name: "idx_users_email_lower".into(),
                table_name: "users".into(),
                columns: vec!["lower(email)".into()],
                is_unique: false,
            },
        );
        // Sorted index: created_at DESC
        t.indexes.insert(
            "idx_users_created_at_desc".into(),
            Index {
                name: "idx_users_created_at_desc".into(),
                table_name: "users".into(),
                columns: vec!["created_at DESC".into()],
                is_unique: false,
            },
        );
        // Already-quoted identifier from pg_indexes
        t.indexes.insert(
            "idx_users_mixed".into(),
            Index {
                name: "idx_users_mixed".into(),
                table_name: "users".into(),
                columns: vec!["\"Email\"".into(), "created_at DESC".into()],
                is_unique: false,
            },
        );
        right.tables.insert("users".into(), t);

        let diff = diff_schemas(&left, &right);
        let stmts = generate_migration(&diff, SqlDialect::Postgres);

        let sqls: Vec<&str> = stmts.iter().map(|s| s.sql.as_str()).collect();

        // Expression index — column clause must not be quoted
        assert!(sqls.contains(&"CREATE INDEX idx_users_email_lower ON users(lower(email));"));
        // Sorted index — DESC must not be quoted
        assert!(sqls.contains(&"CREATE INDEX idx_users_created_at_desc ON users(created_at DESC);"));
        // Mixed: already-quoted ident + sort order
        assert!(
            sqls.contains(&"CREATE INDEX idx_users_mixed ON users(\"Email\", created_at DESC);")
        );
    }

    #[test]
    fn mysql_migration_uses_backtick_quoting_for_camelcase() {
        let left = Schema::new();
        let mut right = Schema::new();
        let mut t = Table::new("UserAccounts");
        t.columns
            .insert("UserId".into(), col("UserId", "int", false, None));
        t.columns
            .insert("email".into(), col("email", "varchar(255)", true, None));
        t.indexes.insert(
            "idx_UserAccounts_email".into(),
            Index {
                name: "idx_UserAccounts_email".into(),
                table_name: "UserAccounts".into(),
                columns: vec!["email".into()],
                is_unique: false,
            },
        );
        right.tables.insert("UserAccounts".into(), t);

        let diff = diff_schemas(&left, &right);
        let stmts = generate_migration(&diff, SqlDialect::MySql);

        let sqls: Vec<&str> = stmts.iter().map(|s| s.sql.as_str()).collect();
        // Table and column names with uppercase must use backticks, not double quotes
        assert!(sqls.iter().any(|s| s.contains("`UserAccounts`")));
        assert!(sqls.iter().any(|s| s.contains("`UserId`")));
        assert!(sqls.iter().any(|s| s.contains("`idx_UserAccounts_email`")));
        // Must NOT contain double-quoted identifiers
        assert!(!sqls.iter().any(|s| s.contains("\"UserAccounts\"")));
        assert!(!sqls.iter().any(|s| s.contains("\"UserId\"")));
    }

    #[test]
    fn mysql_index_columns_are_quoted() {
        // MySQL index columns are plain identifiers from information_schema,
        // not raw SQL — they must be quoted when needed.
        let left = Schema::new();
        let mut right = Schema::new();
        let mut t = Table::new("items");
        t.columns
            .insert("Select".into(), col("Select", "varchar(50)", true, None));
        t.indexes.insert(
            "idx_items_select".into(),
            Index {
                name: "idx_items_select".into(),
                table_name: "items".into(),
                columns: vec!["Select".into()],
                is_unique: false,
            },
        );
        right.tables.insert("items".into(), t);

        let diff = diff_schemas(&left, &right);
        let stmts = generate_migration(&diff, SqlDialect::MySql);

        let sqls: Vec<&str> = stmts.iter().map(|s| s.sql.as_str()).collect();
        // Column name with uppercase must be backtick-quoted in MySQL index
        assert!(sqls.iter().any(|s| s.contains("(`Select`)")));
    }

    #[test]
    fn sqlfile_dialect_does_not_quote_simple_names() {
        let left = Schema::new();
        let mut right = Schema::new();
        let mut t = Table::new("users");
        t.columns
            .insert("email".into(), col("email", "text", true, None));
        right.tables.insert("users".into(), t);

        let diff = diff_schemas(&left, &right);
        let stmts = generate_migration(&diff, SqlDialect::SqlFile);

        // Simple lowercase names should not be quoted for SqlFile dialect
        assert!(stmts[0].sql.contains("CREATE TABLE users"));
        assert!(stmts[0].sql.contains("email text"));
    }

    #[test]
    fn quote_ident_postgres() {
        use super::quote_ident;
        let pg = SqlDialect::Postgres;

        // Simple names pass through unquoted
        assert_eq!(quote_ident("users", pg), "users");
        assert_eq!(quote_ident("idx_orders_id", pg), "idx_orders_id");

        // Names with uppercase get double-quoted
        assert_eq!(quote_ident("Users", pg), "\"Users\"");

        // Names with spaces get double-quoted
        assert_eq!(quote_ident("my table", pg), "\"my table\"");

        // Names with embedded quotes get escaped
        assert_eq!(quote_ident("a\"b", pg), "\"a\"\"b\"");

        // Empty string gets quoted
        assert_eq!(quote_ident("", pg), "\"\"");
    }

    #[test]
    fn quote_ident_mysql_uses_backticks() {
        use super::quote_ident;
        let my = SqlDialect::MySql;

        // Simple names pass through unquoted
        assert_eq!(quote_ident("users", my), "users");

        // Names needing quoting use backticks
        assert_eq!(quote_ident("Users", my), "`Users`");
        assert_eq!(quote_ident("my table", my), "`my table`");

        // Embedded backtick gets escaped
        assert_eq!(quote_ident("a`b", my), "`a``b`");
    }

    #[test]
    fn drop_column_is_blocking() {
        let mut left = Schema::new();
        let mut t = Table::new("users");
        t.columns
            .insert("id".into(), col("id", "integer", false, None));
        t.columns
            .insert("old".into(), col("old", "text", true, None));
        left.tables.insert("users".into(), t);

        let mut right = Schema::new();
        let mut t = Table::new("users");
        t.columns
            .insert("id".into(), col("id", "integer", false, None));
        right.tables.insert("users".into(), t);

        let diff = diff_schemas(&left, &right);
        let stmts = generate_migration(&diff, SqlDialect::Postgres);
        assert!(stmts[0].is_blocking, "DROP COLUMN should be blocking");
    }

    #[test]
    fn drop_index_is_blocking() {
        let mut left = Schema::new();
        let mut t = Table::new("users");
        t.columns
            .insert("id".into(), col("id", "integer", false, None));
        t.indexes.insert(
            "idx".into(),
            Index {
                name: "idx".into(),
                table_name: "users".into(),
                columns: vec!["id".into()],
                is_unique: false,
            },
        );
        left.tables.insert("users".into(), t);

        let mut right = Schema::new();
        let mut t = Table::new("users");
        t.columns
            .insert("id".into(), col("id", "integer", false, None));
        right.tables.insert("users".into(), t);

        let diff = diff_schemas(&left, &right);
        let stmts = generate_migration(&diff, SqlDialect::Postgres);
        assert!(stmts[0].is_blocking, "DROP INDEX should be blocking");
    }

    #[test]
    fn create_table_is_not_blocking() {
        let left = Schema::new();
        let mut right = Schema::new();
        let mut t = Table::new("users");
        t.columns
            .insert("id".into(), col("id", "integer", false, None));
        right.tables.insert("users".into(), t);

        let diff = diff_schemas(&left, &right);
        let stmts = generate_migration(&diff, SqlDialect::Postgres);
        assert!(!stmts[0].is_blocking, "CREATE TABLE should not be blocking");
    }

    #[test]
    fn drop_table_is_blocking() {
        let mut left = Schema::new();
        let mut t = Table::new("old");
        t.columns
            .insert("id".into(), col("id", "integer", false, None));
        left.tables.insert("old".into(), t);
        let right = Schema::new();

        let diff = diff_schemas(&left, &right);
        let stmts = generate_migration(&diff, SqlDialect::Postgres);
        assert!(
            stmts
                .iter()
                .any(|s| s.sql.contains("DROP TABLE") && s.is_blocking),
            "DROP TABLE should be blocking"
        );
    }

    #[test]
    fn mysql_fk_drop_uses_drop_foreign_key() {
        use crate::model::{Constraint, ConstraintKind};

        let mut left = Schema::new();
        let mut t = Table::new("orders");
        t.columns
            .insert("id".into(), col("id", "integer", false, None));
        t.columns
            .insert("user_id".into(), col("user_id", "integer", false, None));
        t.constraints.insert(
            "fk_user".into(),
            Constraint {
                name: "fk_user".into(),
                table_name: "orders".into(),
                kind: ConstraintKind::ForeignKey {
                    columns: vec!["user_id".into()],
                    ref_table: "users".into(),
                    ref_columns: vec!["id".into()],
                    on_delete: None,
                    on_update: None,
                },
            },
        );
        left.tables.insert("orders".into(), t);

        let mut right = Schema::new();
        let mut t = Table::new("orders");
        t.columns
            .insert("id".into(), col("id", "integer", false, None));
        t.columns
            .insert("user_id".into(), col("user_id", "integer", false, None));
        right.tables.insert("orders".into(), t);

        let diff = diff_schemas(&left, &right);
        let stmts = generate_migration(&diff, SqlDialect::MySql);

        assert!(
            stmts[0].sql.contains("DROP FOREIGN KEY"),
            "MySQL should use DROP FOREIGN KEY, got: {}",
            stmts[0].sql
        );
    }

    #[test]
    fn mysql_unique_drop_uses_drop_index() {
        use crate::model::{Constraint, ConstraintKind};

        let mut left = Schema::new();
        let mut t = Table::new("users");
        t.columns
            .insert("email".into(), col("email", "varchar(255)", false, None));
        t.constraints.insert(
            "unique_email".into(),
            Constraint {
                name: "unique_email".into(),
                table_name: "users".into(),
                kind: ConstraintKind::Unique {
                    columns: vec!["email".into()],
                },
            },
        );
        left.tables.insert("users".into(), t);

        let mut right = Schema::new();
        let mut t = Table::new("users");
        t.columns
            .insert("email".into(), col("email", "varchar(255)", false, None));
        right.tables.insert("users".into(), t);

        let diff = diff_schemas(&left, &right);
        let stmts = generate_migration(&diff, SqlDialect::MySql);

        assert!(
            stmts[0].sql.contains("DROP INDEX"),
            "MySQL should use DROP INDEX for unique constraints, got: {}",
            stmts[0].sql
        );
    }

    #[test]
    fn constraint_add_generates_alter_table() {
        use crate::model::{Constraint, ConstraintKind};

        let mut left = Schema::new();
        let mut t = Table::new("orders");
        t.columns
            .insert("user_id".into(), col("user_id", "integer", false, None));
        left.tables.insert("orders".into(), t);

        let mut right = Schema::new();
        let mut t = Table::new("orders");
        t.columns
            .insert("user_id".into(), col("user_id", "integer", false, None));
        t.constraints.insert(
            "fk_user".into(),
            Constraint {
                name: "fk_user".into(),
                table_name: "orders".into(),
                kind: ConstraintKind::ForeignKey {
                    columns: vec!["user_id".into()],
                    ref_table: "users".into(),
                    ref_columns: vec!["id".into()],
                    on_delete: Some("CASCADE".into()),
                    on_update: None,
                },
            },
        );
        right.tables.insert("orders".into(), t);

        let diff = diff_schemas(&left, &right);
        let stmts = generate_migration(&diff, SqlDialect::Postgres);

        assert!(stmts
            .iter()
            .any(|s| s.sql.contains("ADD CONSTRAINT fk_user FOREIGN KEY")));
        assert!(stmts.iter().any(|s| s.sql.contains("ON DELETE CASCADE")));
    }

    #[test]
    fn rollback_reverses_added_table() {
        let left = Schema::new();
        let mut right = Schema::new();
        let mut t = Table::new("orders");
        t.columns
            .insert("id".into(), col("id", "integer", false, None));
        right.tables.insert("orders".into(), t);

        let diff = diff_schemas(&left, &right);
        let rollback = generate_rollback(&diff, SqlDialect::Postgres);

        assert!(rollback.iter().any(|s| s.sql.contains("DROP TABLE orders")));
        assert!(
            rollback
                .iter()
                .any(|s| s.sql.contains("DROP TABLE") && s.is_blocking),
            "Rollback DROP TABLE should be blocking"
        );
    }

    #[test]
    fn rollback_reverses_removed_table() {
        let mut left = Schema::new();
        let mut t = Table::new("legacy");
        t.columns
            .insert("id".into(), col("id", "integer", false, None));
        left.tables.insert("legacy".into(), t);
        let right = Schema::new();

        let diff = diff_schemas(&left, &right);
        let rollback = generate_rollback(&diff, SqlDialect::Postgres);

        assert!(rollback
            .iter()
            .any(|s| s.sql.contains("CREATE TABLE legacy")));
    }

    #[test]
    fn rollback_drop_column_is_blocking() {
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
            .insert("name".into(), col("name", "text", true, None));
        right.tables.insert("users".into(), t);

        let diff = diff_schemas(&left, &right);
        let rollback = generate_rollback(&diff, SqlDialect::Postgres);

        let drop_col = rollback
            .iter()
            .find(|s| s.sql.contains("DROP COLUMN"))
            .unwrap();
        assert!(
            drop_col.is_blocking,
            "Rollback DROP COLUMN should be blocking"
        );
    }

    #[test]
    fn postgres_identifiers_are_escaped_against_injection() {
        let left = Schema::new();
        let mut right = Schema::new();
        let mut t = Table::new("users\"; DROP TABLE payments; --");
        t.columns.insert(
            "email\"; DELETE FROM users; --".into(),
            col("email\"; DELETE FROM users; --", "text", true, None),
        );
        right.tables.insert(t.name.clone(), t);

        let diff = diff_schemas(&left, &right);
        let stmts = generate_migration(&diff, SqlDialect::Postgres);

        assert!(stmts[0]
            .sql
            .starts_with("CREATE TABLE \"users\"\"; DROP TABLE payments; --\""));
        assert!(stmts[0]
            .sql
            .contains("\"email\"\"; DELETE FROM users; --\" text"));
    }
}
