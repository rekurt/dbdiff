use crate::diff::{ColumnDiff, SchemaDiff, TableDiff};
use crate::loader::SqlDialect;
use crate::model::{Column, Index, Table};

/// Generate migration SQL statements from a schema diff.
///
/// Statements are ordered for safe execution:
/// 1. DROP INDEXes
/// 2. DROP COLUMNs
/// 3. DROP TABLEs
/// 4. CREATE TABLEs
/// 5. ADD COLUMNs
/// 6. ALTER COLUMNs
/// 7. CREATE INDEXes
pub fn generate_migration(diff: &SchemaDiff, dialect: SqlDialect) -> Vec<MigrationStatement> {
    let mut statements = Vec::new();

    // Phase 1: DROP INDEXes from modified tables
    for table_diff in &diff.modified_tables {
        for idx in &table_diff.removed_indexes {
            statements.push(MigrationStatement {
                sql: drop_index_sql(idx, dialect),
                warnings: Vec::new(),
            });
        }
    }

    // Phase 2: DROP COLUMNs from modified tables
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
                    table_diff.table_name, col.name
                ),
                warnings,
            });
        }
    }

    // Phase 3: DROP TABLEs
    for table in &diff.removed_tables {
        statements.push(MigrationStatement {
            sql: format!("DROP TABLE {};", table.name),
            warnings: vec![format!(
                "Dropping table '{}' will permanently delete all data.",
                table.name
            )],
        });
    }

    // Phase 4: CREATE TABLEs
    for table in &diff.added_tables {
        statements.push(MigrationStatement {
            sql: create_table_sql(table),
            warnings: Vec::new(),
        });

        // Indexes for new table
        for idx in table.indexes.values() {
            statements.push(MigrationStatement {
                sql: create_index_sql(idx),
                warnings: Vec::new(),
            });
        }
    }

    // Phase 5: ADD COLUMNs
    for table_diff in &diff.modified_tables {
        for col in &table_diff.added_columns {
            let warnings = add_column_warnings(col);
            statements.push(MigrationStatement {
                sql: format!(
                    "ALTER TABLE {} ADD COLUMN {};",
                    table_diff.table_name,
                    col.definition()
                ),
                warnings,
            });
        }
    }

    // Phase 6: ALTER COLUMNs
    for table_diff in &diff.modified_tables {
        let mut alter_stmts = generate_column_alterations(table_diff, dialect);
        statements.append(&mut alter_stmts);
    }

    // Phase 7: CREATE INDEXes on modified tables
    for table_diff in &diff.modified_tables {
        for idx in &table_diff.added_indexes {
            statements.push(MigrationStatement {
                sql: create_index_sql(idx),
                warnings: vec![
                    "Consider using CREATE INDEX CONCURRENTLY to avoid locking the table."
                        .to_string(),
                ],
            });
        }
    }

    statements
}

/// A single migration SQL statement with optional safety warnings.
#[derive(Debug, Clone)]
pub struct MigrationStatement {
    pub sql: String,
    pub warnings: Vec<String>,
}

fn create_table_sql(table: &Table) -> String {
    let columns: Vec<String> = table
        .columns
        .values()
        .map(|c| format!("    {}", c.definition()))
        .collect();
    format!("CREATE TABLE {} (\n{}\n);", table.name, columns.join(",\n"))
}

fn create_index_sql(idx: &Index) -> String {
    let unique = if idx.is_unique { "UNIQUE " } else { "" };
    let cols = idx.columns.join(", ");
    format!(
        "CREATE {unique}INDEX {} ON {}({});",
        idx.name, idx.table_name, cols
    )
}

fn drop_index_sql(idx: &Index, dialect: SqlDialect) -> String {
    match dialect {
        SqlDialect::MySql => format!("DROP INDEX {} ON {};", idx.name, idx.table_name),
        _ => format!("DROP INDEX {};", idx.name),
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

        // Type change
        if old.data_type != new.data_type {
            match dialect {
                SqlDialect::MySql => stmts.push(MigrationStatement {
                    sql: format!("ALTER TABLE {table} MODIFY COLUMN {};", new.definition()),
                    warnings: vec![format!(
                        "Changing column type from '{}' to '{}' may require a table rewrite \
                         and table lock.",
                        old.data_type, new.data_type
                    )],
                }),
                SqlDialect::Sqlite => stmts.push(MigrationStatement {
                    sql: format!(
                        "-- manual migration required for type change on {table}.{}",
                        new.name
                    ),
                    warnings: vec![format!(
                        "SQLite does not support ALTER COLUMN TYPE directly for '{}'. \
                         Recreate table '{table}' with the desired column definition.",
                        new.name
                    )],
                }),
                _ => stmts.push(MigrationStatement {
                    sql: format!(
                        "ALTER TABLE {table} ALTER COLUMN {} TYPE {};",
                        new.name, new.data_type
                    ),
                    warnings: vec![format!(
                        "Changing column type from '{}' to '{}' may require a table rewrite \
                         and AccessExclusiveLock.",
                        old.data_type, new.data_type
                    )],
                }),
            }
        }

        // Nullability change
        if old.is_nullable != new.is_nullable {
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
                        sql: format!("ALTER TABLE {table} MODIFY COLUMN {};", new.definition()),
                        warnings: vec![warning],
                    });
                }
                SqlDialect::Sqlite => {
                    stmts.push(MigrationStatement {
                        sql: format!(
                            "-- manual migration required for nullability change on {table}.{}",
                            new.name
                        ),
                        warnings: vec![format!(
                            "SQLite does not support ALTER COLUMN nullability directly for '{}'. \
                             Recreate table '{table}' with the desired column definition.",
                            new.name
                        )],
                    });
                }
                _ => {
                    if new.is_nullable {
                        stmts.push(MigrationStatement {
                            sql: format!(
                                "ALTER TABLE {table} ALTER COLUMN {} DROP NOT NULL;",
                                new.name
                            ),
                            warnings: Vec::new(),
                        });
                    } else {
                        stmts.push(MigrationStatement {
                            sql: format!(
                                "ALTER TABLE {table} ALTER COLUMN {} SET NOT NULL;",
                                new.name
                            ),
                            warnings: vec![format!(
                                "SET NOT NULL on '{}' will scan the entire table to verify no NULLs exist. \
                                 This acquires AccessExclusiveLock.",
                                new.name
                            )],
                        });
                    }
                }
            }
        }

        // Default change
        if old.default != new.default {
            match &new.default {
                Some(default) => {
                    stmts.push(MigrationStatement {
                        sql: format!(
                            "ALTER TABLE {table} ALTER COLUMN {} SET DEFAULT {default};",
                            new.name
                        ),
                        warnings: Vec::new(),
                    });
                }
                None => {
                    stmts.push(MigrationStatement {
                        sql: format!(
                            "ALTER TABLE {table} ALTER COLUMN {} DROP DEFAULT;",
                            new.name
                        ),
                        warnings: Vec::new(),
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
}
