use rusqlite::Connection;

use crate::error::DbDiffError;
use crate::model::{Column, Index, Schema, Table};

/// Load a schema from a SQLite database file.
///
/// Accepts paths like `mydb.db`, `mydb.sqlite`, `mydb.sqlite3`, or `sqlite://path`.
/// This is a synchronous function — call via `spawn_blocking` from async contexts.
pub fn load(source: &str) -> Result<Schema, DbDiffError> {
    let path = source.strip_prefix("sqlite://").unwrap_or(source);
    let conn = Connection::open(path)?;

    let mut schema = Schema::new();

    // Get all user tables (skip internal sqlite_ tables)
    let table_names = get_table_names(&conn)?;

    for table_name in &table_names {
        let mut table = Table::new(table_name);

        load_columns(&conn, table_name, &mut table)?;
        load_indexes(&conn, table_name, &mut table)?;

        schema.tables.insert(table_name.clone(), table);
    }

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
    let mut stmt = conn.prepare(&format!("PRAGMA table_info(\"{}\")", table_name))?;

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
    let mut stmt = conn.prepare(&format!("PRAGMA index_list(\"{}\")", table_name))?;

    let indexes: Vec<(String, bool, String)> = stmt
        .query_map([], |row| {
            let name: String = row.get(1)?;
            let is_unique: bool = row.get(2)?;
            let origin: String = row.get(3)?;
            Ok((name, is_unique, origin))
        })?
        .collect::<Result<_, _>>()?;

    for (index_name, is_unique, origin) in indexes {
        // Skip auto-generated primary key indexes
        if origin == "pk" {
            continue;
        }

        // PRAGMA index_info returns: seqno, cid, name
        // For expression indexes, name is NULL — skip those columns
        let mut col_stmt = conn.prepare(&format!("PRAGMA index_info(\"{}\")", index_name))?;
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
        "BOOLEAN" | "BOOL" => "integer".to_string(), // SQLite stores booleans as integers
        "DATETIME" | "TIMESTAMP" => "text".to_string(), // SQLite stores dates as text
        "" => "blob".to_string(),                    // untyped columns default to blob affinity
        _ => {
            // Handle types with length: VARCHAR(255) -> varchar(255)
            sqlite_type.to_lowercase()
        }
    }
}

/// Clean up SQLite default values.
fn normalize_default(default: &str) -> String {
    // Remove surrounding quotes if present
    if default.starts_with('\'') && default.ends_with('\'') && default.len() > 2 {
        return default[1..default.len() - 1].to_string();
    }
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
        assert_eq!(normalize_type("BOOLEAN"), "integer");
        assert_eq!(normalize_type("DATETIME"), "text");
        assert_eq!(normalize_type("VARCHAR(255)"), "varchar(255)");
        assert_eq!(normalize_type(""), "blob");
    }

    #[test]
    fn test_normalize_default() {
        assert_eq!(normalize_default("0"), "0");
        assert_eq!(normalize_default("'hello'"), "hello");
        assert_eq!(normalize_default("NULL"), "NULL");
        assert_eq!(normalize_default("CURRENT_TIMESTAMP"), "CURRENT_TIMESTAMP");
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
        assert_eq!(users.columns["active"].data_type, "integer"); // BOOLEAN -> integer
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
}
