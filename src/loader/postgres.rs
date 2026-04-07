use std::sync::OnceLock;

use tokio_postgres::NoTls;

use crate::error::{sanitize_dsn, DbDiffError};
use crate::model::{Column, Index, Schema, Table};

/// Load a schema from a live PostgreSQL database via DSN.
pub async fn load(dsn: &str) -> Result<Schema, DbDiffError> {
    let host = sanitize_dsn(dsn);

    let (client, connection) = tokio_postgres::connect(dsn, NoTls)
        .await
        .map_err(|e| DbDiffError::PostgresConnect {
            host: host.clone(),
            source: e,
        })?;

    // Spawn the connection handler
    tokio::spawn(async move {
        if let Err(e) = connection.await {
            use std::error::Error;
            let mut msg = e.to_string();
            let mut source = e.source();
            while let Some(cause) = source {
                msg.push_str(": ");
                msg.push_str(&cause.to_string());
                source = cause.source();
            }
            eprintln!("PostgreSQL connection error ({}): {msg}", host);
        }
    });

    let mut schema = Schema::new();

    // Load columns from information_schema
    let rows = client
        .query(
            "SELECT table_name, column_name, data_type, character_maximum_length, \
                    is_nullable, column_default \
             FROM information_schema.columns \
             WHERE table_schema = 'public' \
             ORDER BY table_name, ordinal_position",
            &[],
        )
        .await?;

    for row in &rows {
        let table_name: String = row.get("table_name");
        let column_name: String = row.get("column_name");
        let data_type: String = row.get("data_type");
        let char_max_len: Option<i32> = row.get("character_maximum_length");
        let is_nullable: String = row.get("is_nullable");
        let column_default: Option<String> = row.get("column_default");

        let normalized_type = normalize_type(&data_type, char_max_len);

        let column = Column {
            name: column_name.clone(),
            data_type: normalized_type,
            is_nullable: is_nullable == "YES",
            default: column_default.map(|d| normalize_default(&d)),
        };

        schema
            .tables
            .entry(table_name.clone())
            .or_insert_with(|| Table::new(&table_name))
            .columns
            .insert(column_name, column);
    }

    // Load indexes from pg_indexes
    let rows = client
        .query(
            "SELECT indexname, tablename, indexdef \
             FROM pg_indexes \
             WHERE schemaname = 'public' \
             ORDER BY tablename, indexname",
            &[],
        )
        .await?;

    for row in &rows {
        let index_name: String = row.get("indexname");
        let table_name: String = row.get("tablename");
        let index_def: String = row.get("indexdef");

        // Skip primary key indexes (auto-generated)
        if index_name.ends_with("_pkey") {
            continue;
        }

        let is_unique = index_def.to_uppercase().contains("CREATE UNIQUE INDEX");
        let columns = parse_index_columns(&index_def);

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

    Ok(schema)
}

/// Normalize PostgreSQL type names to shorter canonical forms.
fn normalize_type(data_type: &str, char_max_len: Option<i32>) -> String {
    match data_type {
        "character varying" => match char_max_len {
            Some(len) => format!("varchar({len})"),
            None => "varchar".to_string(),
        },
        "character" => match char_max_len {
            Some(len) => format!("char({len})"),
            None => "char".to_string(),
        },
        "timestamp without time zone" => "timestamp".to_string(),
        "timestamp with time zone" => "timestamptz".to_string(),
        "time without time zone" => "time".to_string(),
        "time with time zone" => "timetz".to_string(),
        "boolean" => "bool".to_string(),
        "integer" => "integer".to_string(),
        "bigint" => "bigint".to_string(),
        "smallint" => "smallint".to_string(),
        "double precision" => "float8".to_string(),
        "real" => "float4".to_string(),
        other => other.to_string(),
    }
}

/// Clean up default expressions from PostgreSQL.
fn normalize_default(default: &str) -> String {
    // Remove type casts like ::character varying, ::text, etc.
    static RE: OnceLock<regex::Regex> = OnceLock::new();
    let re = RE.get_or_init(|| {
        regex::Regex::new(r"::\w[\w\s]*(?:\([\d,]+\))?")
            .expect("hardcoded regex must compile")
    });
    let cleaned = re.replace_all(default, "").trim().to_string();

    // Remove surrounding quotes from string defaults
    if cleaned.starts_with('\'') && cleaned.ends_with('\'') && cleaned.len() > 2 {
        return cleaned[1..cleaned.len() - 1].to_string();
    }

    cleaned
}

/// Extract column names from an index definition string.
/// Example: "CREATE INDEX idx_name ON table_name USING btree (col1, col2)"
fn parse_index_columns(indexdef: &str) -> Vec<String> {
    if let Some(start) = indexdef.rfind('(') {
        if let Some(end) = indexdef.rfind(')') {
            let cols = &indexdef[start + 1..end];
            return cols.split(',').map(|c| c.trim().to_string()).collect();
        }
    }
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_type() {
        assert_eq!(
            normalize_type("character varying", Some(255)),
            "varchar(255)"
        );
        assert_eq!(normalize_type("character varying", None), "varchar");
        assert_eq!(
            normalize_type("timestamp with time zone", None),
            "timestamptz"
        );
        assert_eq!(normalize_type("boolean", None), "bool");
        assert_eq!(normalize_type("integer", None), "integer");
        assert_eq!(normalize_type("text", None), "text");
    }

    #[test]
    fn test_normalize_default() {
        assert_eq!(normalize_default("now()"), "now()");
        assert_eq!(normalize_default("'hello'::character varying"), "hello");
        assert_eq!(normalize_default("0"), "0");
        assert_eq!(normalize_default("true"), "true");
    }

    #[test]
    fn test_parse_index_columns() {
        assert_eq!(
            parse_index_columns("CREATE INDEX idx ON table USING btree (col1, col2)"),
            vec!["col1", "col2"]
        );
        assert_eq!(
            parse_index_columns("CREATE UNIQUE INDEX idx ON table (email)"),
            vec!["email"]
        );
    }
}
