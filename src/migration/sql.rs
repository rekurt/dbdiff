use crate::diff::{ColumnDiff, TableDiff};
use crate::loader::SqlDialect;
use crate::model::{Column, Constraint, ConstraintKind, Index, Table};

use super::MigrationStatement;

pub fn quote_ident(name: &str, dialect: SqlDialect) -> String {
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

pub fn column_definition_sql(col: &Column, dialect: SqlDialect) -> String {
    let mut def = format!("{} {}", quote_ident(&col.name, dialect), col.data_type);
    if !col.is_nullable {
        def.push_str(" NOT NULL");
    }
    if let Some(ref default) = col.default {
        def.push_str(&format!(" DEFAULT {default}"));
    }
    def
}

pub fn create_table_sql(table: &Table, dialect: SqlDialect) -> String {
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

pub fn create_index_sql(idx: &Index, dialect: SqlDialect, concurrently: bool) -> String {
    let unique = if idx.is_unique { "UNIQUE " } else { "" };
    let concurrent = if concurrently && matches!(dialect, SqlDialect::Postgres) {
        "CONCURRENTLY "
    } else {
        ""
    };
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
        "CREATE {unique}INDEX {concurrent}{} ON {}({});",
        quote_ident(&idx.name, dialect),
        quote_ident(&idx.table_name, dialect),
        cols
    )
}

pub fn drop_index_sql(idx: &Index, dialect: SqlDialect) -> String {
    match dialect {
        SqlDialect::MySql => format!(
            "DROP INDEX {} ON {};",
            quote_ident(&idx.name, dialect),
            quote_ident(&idx.table_name, dialect)
        ),
        _ => format!("DROP INDEX {};", quote_ident(&idx.name, dialect)),
    }
}

pub fn add_constraint_sql(c: &Constraint, dialect: SqlDialect) -> String {
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
        ConstraintKind::PrimaryKey { columns } => {
            let cols: Vec<String> = columns.iter().map(|c| quote_ident(c, dialect)).collect();
            // MySQL does not accept named PRIMARY KEY in ADD CONSTRAINT
            if dialect == SqlDialect::MySql {
                format!(
                    "ALTER TABLE {table} ADD PRIMARY KEY ({});",
                    cols.join(", ")
                )
            } else {
                format!(
                    "ALTER TABLE {table} ADD CONSTRAINT {name} PRIMARY KEY ({});",
                    cols.join(", ")
                )
            }
        }
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

pub fn drop_constraint_sql(c: &Constraint, dialect: SqlDialect) -> String {
    match dialect {
        SqlDialect::MySql => {
            // MySQL uses DROP PRIMARY KEY, DROP FOREIGN KEY, DROP INDEX for different constraint types
            match &c.kind {
                ConstraintKind::PrimaryKey { .. } => format!(
                    "ALTER TABLE {} DROP PRIMARY KEY;",
                    quote_ident(&c.table_name, dialect)
                ),
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
        SqlDialect::Sqlite => format!(
            "-- manual migration required: drop constraint {} on {} \
             (SQLite does not support ALTER TABLE DROP CONSTRAINT; recreate the table)",
            c.name, c.table_name
        ),
        _ => format!(
            "ALTER TABLE {} DROP CONSTRAINT {};",
            quote_ident(&c.table_name, dialect),
            quote_ident(&c.name, dialect)
        ),
    }
}

pub fn rename_column_sql(
    table: &str,
    old_name: &str,
    new_name: &str,
    dialect: SqlDialect,
) -> String {
    format!(
        "ALTER TABLE {} RENAME COLUMN {} TO {};",
        quote_ident(table, dialect),
        quote_ident(old_name, dialect),
        quote_ident(new_name, dialect)
    )
}

pub fn rename_table_sql(old_name: &str, new_name: &str, dialect: SqlDialect) -> String {
    match dialect {
        SqlDialect::MySql => format!(
            "RENAME TABLE {} TO {};",
            quote_ident(old_name, dialect),
            quote_ident(new_name, dialect)
        ),
        _ => format!(
            "ALTER TABLE {} RENAME TO {};",
            quote_ident(old_name, dialect),
            quote_ident(new_name, dialect)
        ),
    }
}

pub fn add_column_warnings(col: &Column) -> Vec<String> {
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

/// Generate ALTER statements to revert modified columns to their old definitions (for rollback).
pub fn generate_column_alterations_reversed(
    table_diff: &TableDiff,
    dialect: SqlDialect,
) -> Vec<MigrationStatement> {
    // Swap old<->new in each ColumnDiff, then reuse the forward logic
    let reversed = TableDiff {
        table_name: table_diff.table_name.clone(),
        added_columns: Vec::new(),
        removed_columns: Vec::new(),
        renamed_columns: Vec::new(),
        modified_columns: table_diff
            .modified_columns
            .iter()
            .map(|cd| ColumnDiff {
                old: cd.new.clone(),
                new: cd.old.clone(),
            })
            .collect(),
        unchanged_columns: Vec::new(),
        added_indexes: Vec::new(),
        removed_indexes: Vec::new(),
        added_constraints: Vec::new(),
        removed_constraints: Vec::new(),
    };
    generate_column_alterations(&reversed, dialect)
}

pub fn generate_column_alterations(
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
