use rusqlite::Connection;
use std::path::Path;

use crate::error::DbDiffError;
use crate::model::{Column, Constraint, ConstraintKind, Index, Schema, Table, View};

/// Load a schema from a SQLite database file.
///
/// Accepts paths like `mydb.db`, `mydb.sqlite`, `mydb.sqlite3`, or `sqlite://path`.
/// This is a synchronous function — call via `spawn_blocking` from async contexts.
pub fn load(source: &str) -> Result<Schema, DbDiffError> {
    let path = source.strip_prefix("sqlite://").unwrap_or(source);
    if !Path::new(path).exists() {
        return Err(DbDiffError::invalid_arg(format!(
            "SQLite source file does not exist: {path}"
        )));
    }
    let conn = Connection::open(path)?;

    let mut schema = Schema::new();

    // Get all user tables (skip internal sqlite_ tables)
    let table_names = get_table_names(&conn)?;

    for table_name in &table_names {
        let mut table = Table::new(table_name);

        load_columns(&conn, table_name, &mut table)?;
        load_indexes(&conn, table_name, &mut table)?;
        load_foreign_keys(&conn, table_name, &mut table)?;

        schema.tables.insert(table_name.clone(), table);
    }

    // Load views
    load_views(&conn, &mut schema)?;

    Ok(schema)
}

fn get_table_names(conn: &Connection) -> Result<Vec<String>, DbDiffError> {
    let mut stmt = conn.prepare(
        "SELECT name FROM sqlite_master \
         WHERE type = 'table' AND name NOT LIKE 'sqlite_%' \
         ORDER BY name",
    )?;

    let names = stmt
        .query_map([], |row| row.get(0))?
        .collect::<Result<Vec<String>, _>>()?;

    Ok(names)
}

fn load_columns(conn: &Connection, table_name: &str, table: &mut Table) -> Result<(), DbDiffError> {
    // PRAGMA table_info returns: cid, name, type, notnull, dflt_value, pk
    let mut stmt = conn.prepare(&format!(
        "PRAGMA table_info({})",
        quote_identifier(table_name)
    ))?;

    let columns = stmt.query_map([], |row| {
        let name: String = row.get(1)?;
        let data_type: String = row.get(2)?;
        let notnull: bool = row.get(3)?;
        let dflt_value: Option<String> = row.get(4)?;

        Ok(Column {
            name,
            data_type: normalize_type(&data_type),
            is_nullable: !notnull,
            default: dflt_value.map(|d| normalize_default(&d)),
        })
    })?;

    for col in columns {
        let col = col?;
        table.columns.insert(col.name.clone(), col);
    }

    Ok(())
}

fn load_indexes(conn: &Connection, table_name: &str, table: &mut Table) -> Result<(), DbDiffError> {
    // PRAGMA index_list returns: seq, name, unique, origin, partial
    let mut stmt = conn.prepare(&format!(
        "PRAGMA index_list({})",
        quote_identifier(table_name)
    ))?;

    let indexes: Vec<(String, bool, String)> = stmt
        .query_map([], |row| {
            let name: String = row.get(1)?;
            let is_unique: bool = row.get(2)?;
            let origin: String = row.get(3)?;
            Ok((name, is_unique, origin))
        })?
        .collect::<Result<_, _>>()?;

    for (index_name, is_unique, origin) in indexes {
        // Skip auto-generated indexes for PRIMARY KEY / UNIQUE constraints.
        // SQLite marks these with origin:
        // - "pk": PRIMARY KEY
        // - "u": UNIQUE constraint
        // These are internal implementation details (often sqlite_autoindex_*),
        // not user-managed indexes we should diff/migrate directly.
        if matches!(origin.as_str(), "pk" | "u") {
            continue;
        }

        // PRAGMA index_info returns: seqno, cid, name
        // For expression indexes, name is NULL — skip those columns
        let mut col_stmt = conn.prepare(&format!(
            "PRAGMA index_info({})",
            quote_identifier(&index_name)
        ))?;
        let columns: Vec<String> = col_stmt
            .query_map([], |row| row.get::<_, Option<String>>(2))?
            .filter_map(|r| r.ok().flatten())
            .collect();

        if columns.is_empty() {
            // Expression indexes report NULL column names via PRAGMA index_info.
            // Skip these until expression terms are represented explicitly.
            continue;
        }

        let index = Index {
            name: index_name.clone(),
            table_name: table_name.to_string(),
            columns,
            is_unique,
        };

        table.indexes.insert(index_name, index);
    }

    Ok(())
}

fn load_foreign_keys(
    conn: &Connection,
    table_name: &str,
    table: &mut Table,
) -> Result<(), DbDiffError> {
    let mut stmt = conn.prepare(&format!(
        "PRAGMA foreign_key_list({})",
        quote_identifier(table_name)
    ))?;

    // PRAGMA foreign_key_list returns: id, seq, table, from, to, on_update, on_delete, match
    // Note: `to` (index 4) can be NULL when the FK references the primary key implicitly
    let fks: Vec<(i32, String, String, Option<String>, String, String)> = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, i32>(0)?,
                row.get::<_, String>(2)?,         // ref_table
                row.get::<_, String>(3)?,         // from column
                row.get::<_, Option<String>>(4)?, // to column (nullable)
                row.get::<_, String>(5)?,         // on_update
                row.get::<_, String>(6)?,         // on_delete
            ))
        })?
        .filter_map(|r| r.ok())
        .collect();

    // Group by FK id: (ref_table, columns, ref_columns, on_update, on_delete)
    type FkGroup = (String, Vec<String>, Vec<String>, String, String);
    let mut fk_groups: std::collections::BTreeMap<i32, FkGroup> = std::collections::BTreeMap::new();

    for (id, ref_table, from_col, to_col, on_update, on_delete) in &fks {
        let entry = fk_groups.entry(*id).or_insert_with(|| {
            (
                ref_table.clone(),
                Vec::new(),
                Vec::new(),
                on_update.clone(),
                on_delete.clone(),
            )
        });
        entry.1.push(from_col.clone());
        // When `to` is NULL, SQLite references the table's primary key implicitly.
        // Use the source column name as best-effort (matches common patterns like
        // user_id -> user_id). The actual PK name isn't available from foreign_key_list.
        if let Some(to) = to_col {
            entry.2.push(to.clone());
        } else {
            entry.2.push(from_col.clone());
        }
    }

    for (id, (ref_table, columns, ref_columns, on_update, on_delete)) in fk_groups {
        let name = format!("fk_{}_{}", table_name, id);
        table.constraints.insert(
            name.clone(),
            Constraint {
                name: name.clone(),
                table_name: table_name.to_string(),
                kind: ConstraintKind::ForeignKey {
                    columns,
                    ref_table,
                    ref_columns,
                    on_delete: if on_delete != "NO ACTION" {
                        Some(on_delete)
                    } else {
                        None
                    },
                    on_update: if on_update != "NO ACTION" {
                        Some(on_update)
                    } else {
                        None
                    },
                },
            },
        );
    }

    Ok(())
}

fn load_views(conn: &Connection, schema: &mut Schema) -> Result<(), DbDiffError> {
    let mut stmt =
        conn.prepare("SELECT name, sql FROM sqlite_master WHERE type = 'view' ORDER BY name")?;

    let views: Vec<(String, String)> = stmt
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
        .filter_map(|r| r.ok())
        .collect();

    let view_as_re = regex::Regex::new(r"(?i)\bAS\s+").unwrap();
    for (name, sql) in views {
        // Extract definition from "CREATE VIEW name AS ..." (case-insensitive,
        // flexible whitespace — handles tabs, newlines around AS keyword)
        let definition = view_as_re
            .find(&sql)
            .map(|m| sql[m.end()..].to_string())
            .unwrap_or(sql);
        schema.views.insert(
            name.clone(),
            View {
                name,
                definition: definition.trim().to_string(),
            },
        );
    }

    Ok(())
}

fn quote_identifier(identifier: &str) -> String {
    format!("\"{}\"", identifier.replace('"', "\"\""))
}

/// Normalize SQLite type names.
///
/// SQLite has flexible typing. We normalize common aliases to canonical forms.
fn normalize_type(sqlite_type: &str) -> String {
    let upper = sqlite_type.trim().to_uppercase();

    match upper.as_str() {
        "INT" | "INTEGER" => "integer".to_string(),
        "TEXT" | "CLOB" => "text".to_string(),
        "REAL" | "DOUBLE" | "DOUBLE PRECISION" | "FLOAT" => "real".to_string(),
        "BLOB" => "blob".to_string(),
        "BOOLEAN" | "BOOL" => "bool".to_string(),
        "DATETIME" | "TIMESTAMP" => "datetime".to_string(),
        "" => "blob".to_string(), // untyped columns default to blob affinity
        _ => {
            // Handle types with length: VARCHAR(255) -> varchar(255)
            sqlite_type.to_lowercase()
        }
    }
}

/// Clean up SQLite default values.
fn normalize_default(default: &str) -> String {
    // Preserve literal text exactly so downstream SQL rendering keeps valid quoting.
    default.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_type() {
        assert_eq!(normalize_type("INTEGER"), "integer");
        assert_eq!(normalize_type("INT"), "integer");
        assert_eq!(normalize_type("TEXT"), "text");
        assert_eq!(normalize_type("REAL"), "real");
        assert_eq!(normalize_type("BLOB"), "blob");
        assert_eq!(normalize_type("BOOLEAN"), "bool");
        assert_eq!(normalize_type("DATETIME"), "datetime");
        assert_eq!(normalize_type("VARCHAR(255)"), "varchar(255)");
        assert_eq!(normalize_type(""), "blob");
    }

    #[test]
    fn test_normalize_default() {
        assert_eq!(normalize_default("0"), "0");
        assert_eq!(normalize_default("'hello'"), "'hello'");
        assert_eq!(normalize_default("NULL"), "NULL");
        assert_eq!(normalize_default("CURRENT_TIMESTAMP"), "CURRENT_TIMESTAMP");
    }

    #[test]
    fn test_quote_identifier() {
        assert_eq!(quote_identifier("users"), "\"users\"");
        assert_eq!(quote_identifier("foo\"bar"), "\"foo\"\"bar\"");
    }

    #[test]
    fn load_fails_when_sqlite_file_is_missing() {
        let missing_path = format!(
            "/tmp/dbdiff-missing-{}-{}.sqlite",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );

        let err = load(&missing_path).unwrap_err();
        assert_eq!(err.code, crate::error::ErrorCode::InvalidArg);
        assert!(err.message.contains("does not exist"));
        assert!(err.message.contains(&missing_path));
    }

    #[test]
    fn load_sqlite_schema() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE users (
                id INTEGER PRIMARY KEY NOT NULL,
                email TEXT NOT NULL,
                name VARCHAR(100),
                active BOOLEAN DEFAULT 1
            );
            CREATE INDEX idx_users_email ON users(email);
            CREATE TABLE orders (
                id INTEGER PRIMARY KEY NOT NULL,
                user_id INTEGER NOT NULL,
                total REAL NOT NULL DEFAULT 0.0
            );
            CREATE INDEX idx_orders_user_id ON orders(user_id);",
        )
        .unwrap();

        // Use the internal functions to test
        let mut schema = Schema::new();
        let table_names = get_table_names(&conn).unwrap();
        assert_eq!(table_names, vec!["orders", "users"]);

        for name in &table_names {
            let mut table = Table::new(name);
            load_columns(&conn, name, &mut table).unwrap();
            load_indexes(&conn, name, &mut table).unwrap();
            schema.tables.insert(name.clone(), table);
        }

        // Verify users table
        let users = &schema.tables["users"];
        assert_eq!(users.columns.len(), 4);
        assert_eq!(users.columns["id"].data_type, "integer");
        assert!(!users.columns["id"].is_nullable);
        assert_eq!(users.columns["email"].data_type, "text");
        assert_eq!(users.columns["name"].data_type, "varchar(100)");
        assert_eq!(users.columns["active"].data_type, "bool");
        assert_eq!(users.columns["active"].default.as_deref(), Some("1"));
        assert_eq!(users.indexes.len(), 1);
        assert!(users.indexes.contains_key("idx_users_email"));
        assert_eq!(users.indexes["idx_users_email"].columns, vec!["email"]);

        // Verify orders table
        let orders = &schema.tables["orders"];
        assert_eq!(orders.columns.len(), 3);
        assert_eq!(orders.indexes.len(), 1);
        assert!(orders.indexes.contains_key("idx_orders_user_id"));
    }

    #[test]
    fn expression_indexes_are_skipped() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE items (
                id INTEGER PRIMARY KEY NOT NULL,
                name TEXT NOT NULL
            );
            CREATE INDEX idx_items_lower_name ON items(lower(name));",
        )
        .unwrap();

        let mut table = Table::new("items");
        load_indexes(&conn, "items", &mut table).unwrap();

        // Expression index is skipped to avoid producing empty-column SQL definitions.
        assert!(!table.indexes.contains_key("idx_items_lower_name"));
    }

    #[test]
    fn unique_constraint_autoindexes_are_skipped() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE users (
                id INTEGER PRIMARY KEY NOT NULL,
                email TEXT NOT NULL UNIQUE
            );
            CREATE INDEX idx_users_email ON users(email);",
        )
        .unwrap();

        let mut table = Table::new("users");
        load_indexes(&conn, "users", &mut table).unwrap();

        assert!(table.indexes.contains_key("idx_users_email"));
        assert_eq!(table.indexes.len(), 1);
        assert!(!table
            .indexes
            .keys()
            .any(|index_name| index_name.starts_with("sqlite_autoindex_")));
    }
}
