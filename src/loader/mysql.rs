use mysql_async::prelude::*;
use mysql_async::{Opts, OptsBuilder};
use std::collections::BTreeMap;

use crate::error::DbDiffError;
use crate::model::{Column, Index, Schema, Table};

/// Load a schema from a live MySQL/MariaDB database via DSN.
pub async fn load(dsn: &str) -> Result<Schema, DbDiffError> {
    let opts = Opts::from_url(dsn)
        .map_err(|e| DbDiffError::InvalidArg(format!("Invalid MySQL DSN: {e}")))?;
    let db_name = opts
        .db_name()
        .ok_or_else(|| DbDiffError::InvalidArg("MySQL DSN must include a database name".into()))?
        .to_string();

    let pool = mysql_async::Pool::new(OptsBuilder::from_opts(opts.clone()));
    let mut conn = pool.get_conn().await?;

    let mut schema = Schema::new();

    // Load columns from information_schema
    let rows: Vec<(String, String, String, String, Option<String>)> = conn
        .exec(
            "SELECT table_name, column_name, column_type, is_nullable, column_default \
             FROM information_schema.columns \
             WHERE table_schema = :db \
               AND table_name IN ( \
                 SELECT table_name \
                 FROM information_schema.tables \
                 WHERE table_schema = :db \
                   AND table_type = 'BASE TABLE' \
               ) \
             ORDER BY table_name, ordinal_position",
            mysql_async::params! { "db" => &db_name },
        )
        .await?;

    for (table_name, column_name, column_type, is_nullable, column_default) in &rows {
        let column = Column {
            name: column_name.clone(),
            data_type: normalize_type(column_type),
            is_nullable: is_nullable == "YES",
            default: column_default.as_ref().map(|d| normalize_default(d)),
        };

        schema
            .tables
            .entry(table_name.clone())
            .or_insert_with(|| Table::new(table_name))
            .columns
            .insert(column_name.clone(), column);
    }

    // Load indexes from information_schema.statistics
    let idx_rows: Vec<(String, String, Option<String>, i64)> = conn
        .exec(
            "SELECT table_name, index_name, column_name, non_unique \
             FROM information_schema.statistics \
             WHERE table_schema = :db \
               AND index_name != 'PRIMARY' \
               AND table_name IN ( \
                 SELECT table_name \
                 FROM information_schema.tables \
                 WHERE table_schema = :db \
                   AND table_type = 'BASE TABLE' \
               ) \
             ORDER BY table_name, index_name, seq_in_index",
            mysql_async::params! { "db" => &db_name },
        )
        .await?;

    let index_map = group_index_rows(&idx_rows);
    for ((table_name, index_name), (columns, is_unique)) in index_map {
        if columns.is_empty() {
            // Functional indexes can have NULL `column_name` in information_schema.statistics.
            // Skip for now to avoid creating malformed empty-column indexes in downstream SQL.
            continue;
        }

        let index = Index {
            name: index_name.clone(),
            table_name: table_name.clone(),
            columns,
            is_unique,
        };

        if let Some(table) = schema.tables.get_mut(&table_name) {
            table.indexes.insert(index_name, index);
        }
    }

    pool.disconnect().await?;
    Ok(schema)
}

fn group_index_rows(
    idx_rows: &[(String, String, Option<String>, i64)],
) -> BTreeMap<(String, String), (Vec<String>, bool)> {
    let mut index_map: BTreeMap<(String, String), (Vec<String>, bool)> = BTreeMap::new();

    for (table_name, index_name, column_name, non_unique) in idx_rows {
        let entry = index_map
            .entry((table_name.clone(), index_name.clone()))
            .or_insert_with(|| (Vec::new(), *non_unique == 0));
        if let Some(column_name) = column_name {
            entry.0.push(column_name.clone());
        }
    }

    index_map
}

/// Normalize MySQL column types.
///
/// MySQL returns types like `int(11)`, `bigint(20)` — strip display widths from integer types
/// since they don't affect storage. Keep lengths for varchar, char, decimal, etc.
fn normalize_type(column_type: &str) -> String {
    let lower = column_type.to_lowercase();

    // Strip display width from integer types: int(11) -> int, bigint(20) -> bigint
    if let Some(base) = lower.strip_suffix(")") {
        if let Some(paren_pos) = base.rfind('(') {
            let type_name = &base[..paren_pos];
            match type_name {
                "tinyint" | "smallint" | "mediumint" | "int" | "bigint" => {
                    return type_name.to_string();
                }
                _ => {}
            }
        }
    }

    // Handle unsigned variants: int(11) unsigned -> int unsigned
    if lower.contains(" unsigned") {
        let without_unsigned = lower.replace(" unsigned", "");
        let normalized = normalize_type(&without_unsigned);
        return format!("{normalized} unsigned");
    }

    lower
}

/// Clean up MySQL default values.
fn normalize_default(default: &str) -> String {
    // Preserve literal text exactly so downstream SQL rendering keeps valid quoting.
    default.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_type() {
        assert_eq!(normalize_type("int(11)"), "int");
        assert_eq!(normalize_type("bigint(20)"), "bigint");
        assert_eq!(normalize_type("tinyint(1)"), "tinyint");
        assert_eq!(normalize_type("varchar(255)"), "varchar(255)");
        assert_eq!(normalize_type("decimal(10,2)"), "decimal(10,2)");
        assert_eq!(normalize_type("text"), "text");
        assert_eq!(normalize_type("int(11) unsigned"), "int unsigned");
    }

    #[test]
    fn test_normalize_default() {
        assert_eq!(normalize_default("0"), "0");
        assert_eq!(normalize_default("'hello'"), "'hello'");
        assert_eq!(normalize_default("CURRENT_TIMESTAMP"), "CURRENT_TIMESTAMP");
        assert_eq!(normalize_default("NULL"), "NULL");
    }

    #[test]
    fn test_group_index_rows_handles_null_column_names() {
        let rows = vec![
            (
                "users".to_string(),
                "idx_users_email".to_string(),
                Some("email".to_string()),
                0,
            ),
            ("users".to_string(), "idx_users_func".to_string(), None, 1),
        ];

        let grouped = group_index_rows(&rows);

        assert_eq!(
            grouped.get(&(String::from("users"), String::from("idx_users_email"))),
            Some(&(vec![String::from("email")], true))
        );
        assert_eq!(
            grouped.get(&(String::from("users"), String::from("idx_users_func"))),
            Some(&(Vec::new(), false))
        );
    }
}
