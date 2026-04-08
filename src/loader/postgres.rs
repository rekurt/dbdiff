use std::sync::OnceLock;

use tokio_postgres::Client;

use crate::error::{sanitize_dsn, DbDiffError};
use crate::model::{
    Column, Constraint, ConstraintKind, EnumType, Index, Schema, Sequence, Table, View,
};

/// SSL mode for PostgreSQL connections.
#[derive(Debug, Clone, Copy, Default)]
pub enum PgSslMode {
    Disable,
    #[default]
    Prefer,
    Require,
}

/// Load a schema from a live PostgreSQL database via DSN.
pub async fn load(dsn: &str) -> Result<Schema, DbDiffError> {
    load_with_ssl(dsn, PgSslMode::Prefer).await
}

/// Load a schema from a live PostgreSQL database via DSN with specified SSL mode.
pub async fn load_with_ssl(dsn: &str, ssl_mode: PgSslMode) -> Result<Schema, DbDiffError> {
    let client = connect(dsn, ssl_mode).await?;
    load_from_client(&client).await
}

async fn connect(dsn: &str, ssl_mode: PgSslMode) -> Result<Client, DbDiffError> {
    let host = sanitize_dsn(dsn);

    match ssl_mode {
        PgSslMode::Disable => connect_no_tls(dsn, &host).await,
        PgSslMode::Prefer => {
            // Try TLS first, fall back to plaintext
            match connect_tls(dsn, &host).await {
                Ok(client) => Ok(client),
                Err(_) => connect_no_tls(dsn, &host).await,
            }
        }
        PgSslMode::Require => connect_tls(dsn, &host).await,
    }
}

async fn connect_no_tls(dsn: &str, host: &str) -> Result<Client, DbDiffError> {
    let (client, connection) = tokio_postgres::connect(dsn, tokio_postgres::NoTls)
        .await
        .map_err(|e| DbDiffError::connection(dsn, e))?;

    let h = host.to_string();
    tokio::spawn(async move {
        if let Err(e) = connection.await {
            eprintln!("PostgreSQL connection error ({h}): {e}");
        }
    });

    Ok(client)
}

async fn connect_tls(dsn: &str, host: &str) -> Result<Client, DbDiffError> {
    let tls_connector = native_tls::TlsConnector::builder()
        .danger_accept_invalid_certs(true)
        .build()
        .map_err(|e| DbDiffError::connection(dsn, e))?;
    let connector = postgres_native_tls::MakeTlsConnector::new(tls_connector);

    let (client, connection) = tokio_postgres::connect(dsn, connector)
        .await
        .map_err(|e| DbDiffError::connection(dsn, e))?;

    let h = host.to_string();
    tokio::spawn(async move {
        if let Err(e) = connection.await {
            eprintln!("PostgreSQL connection error ({h}): {e}");
        }
    });

    Ok(client)
}

async fn load_from_client(client: &Client) -> Result<Schema, DbDiffError> {
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

    // Load foreign key constraints
    let rows = client
        .query(
            "SELECT tc.constraint_name, tc.table_name, \
                    kcu.column_name, \
                    ccu.table_name AS ref_table, \
                    ccu.column_name AS ref_column, \
                    rc.delete_rule, rc.update_rule \
             FROM information_schema.table_constraints tc \
             JOIN information_schema.key_column_usage kcu \
               ON tc.constraint_name = kcu.constraint_name \
               AND tc.table_schema = kcu.table_schema \
               AND tc.table_name = kcu.table_name \
             JOIN information_schema.constraint_column_usage ccu \
               ON ccu.constraint_name = tc.constraint_name \
               AND ccu.table_schema = tc.table_schema \
             LEFT JOIN information_schema.referential_constraints rc \
               ON rc.constraint_name = tc.constraint_name \
               AND rc.constraint_schema = tc.table_schema \
             WHERE tc.table_schema = 'public' \
               AND tc.constraint_type IN ('FOREIGN KEY', 'UNIQUE') \
             ORDER BY tc.table_name, tc.constraint_name, kcu.ordinal_position",
            &[],
        )
        .await?;

    // Group FK columns by constraint name
    // (table, columns, ref_table, ref_columns, on_delete, on_update)
    type FkEntry = (
        String,
        Vec<String>,
        String,
        Vec<String>,
        Option<String>,
        Option<String>,
    );
    // Key by (table_name, constraint_name) since constraint names are only unique per table
    let mut fk_map: std::collections::BTreeMap<(String, String), FkEntry> =
        std::collections::BTreeMap::new();
    let mut unique_map: std::collections::BTreeMap<(String, String), Vec<String>> =
        std::collections::BTreeMap::new();

    for row in &rows {
        let constraint_name: String = row.get("constraint_name");
        let table_name: String = row.get("table_name");
        let column_name: String = row.get("column_name");
        let ref_table: String = row.get("ref_table");
        let ref_column: String = row.get("ref_column");
        let delete_rule: Option<String> = row.get("delete_rule");
        let update_rule: Option<String> = row.get("update_rule");

        let key = (table_name.clone(), constraint_name.clone());

        if delete_rule.is_some() {
            // FK constraint
            let entry = fk_map.entry(key).or_insert_with(|| {
                (
                    table_name,
                    Vec::new(),
                    ref_table,
                    Vec::new(),
                    delete_rule.filter(|r| r != "NO ACTION"),
                    update_rule.filter(|r| r != "NO ACTION"),
                )
            });
            if !entry.1.contains(&column_name) {
                entry.1.push(column_name);
            }
            if !entry.3.contains(&ref_column) {
                entry.3.push(ref_column);
            }
        } else {
            // UNIQUE constraint
            let entry = unique_map.entry(key).or_default();
            if !entry.contains(&column_name) {
                entry.push(column_name);
            }
        }
    }

    for ((table_name, name), (_, columns, ref_table, ref_columns, on_delete, on_update)) in fk_map {
        if let Some(table) = schema.tables.get_mut(&table_name) {
            table.constraints.insert(
                name.clone(),
                Constraint {
                    name: name.clone(),
                    table_name: table_name.clone(),
                    kind: ConstraintKind::ForeignKey {
                        columns,
                        ref_table,
                        ref_columns,
                        on_delete,
                        on_update,
                    },
                },
            );
        }
    }

    for ((table_name, name), columns) in unique_map {
        if let Some(table) = schema.tables.get_mut(&table_name) {
            table.constraints.insert(
                name.clone(),
                Constraint {
                    name: name.clone(),
                    table_name: table_name.clone(),
                    kind: ConstraintKind::Unique { columns },
                },
            );
        }
    }

    // Load check constraints
    let rows = client
        .query(
            "SELECT con.conname AS constraint_name, \
                    rel.relname AS table_name, \
                    pg_get_constraintdef(con.oid) AS definition \
             FROM pg_constraint con \
             JOIN pg_class rel ON con.conrelid = rel.oid \
             JOIN pg_namespace nsp ON rel.relnamespace = nsp.oid \
             WHERE nsp.nspname = 'public' AND con.contype = 'c' \
             ORDER BY rel.relname, con.conname",
            &[],
        )
        .await?;

    for row in &rows {
        let constraint_name: String = row.get("constraint_name");
        let table_name: String = row.get("table_name");
        let definition: String = row.get("definition");

        // pg_get_constraintdef returns "CHECK ((expr))" — strip outer CHECK(...)
        let expression = definition
            .strip_prefix("CHECK (")
            .and_then(|s| s.strip_suffix(')'))
            .unwrap_or(&definition)
            .to_string();

        if let Some(table) = schema.tables.get_mut(&table_name) {
            table.constraints.insert(
                constraint_name.clone(),
                Constraint {
                    name: constraint_name,
                    table_name: table_name.clone(),
                    kind: ConstraintKind::Check { expression },
                },
            );
        }
    }

    // Load views
    let rows = client
        .query(
            "SELECT viewname, definition \
             FROM pg_views \
             WHERE schemaname = 'public' \
             ORDER BY viewname",
            &[],
        )
        .await?;

    for row in &rows {
        let name: String = row.get("viewname");
        let definition: String = row.get("definition");
        schema.views.insert(
            name.clone(),
            View {
                name,
                definition: definition.trim().to_string(),
            },
        );
    }

    // Load enum types
    let rows = client
        .query(
            "SELECT t.typname AS enum_name, \
                    array_agg(e.enumlabel ORDER BY e.enumsortorder) AS enum_values \
             FROM pg_type t \
             JOIN pg_enum e ON t.oid = e.enumtypid \
             JOIN pg_namespace n ON t.typnamespace = n.oid \
             WHERE n.nspname = 'public' \
             GROUP BY t.typname \
             ORDER BY t.typname",
            &[],
        )
        .await?;

    for row in &rows {
        let name: String = row.get("enum_name");
        let values: Vec<String> = row.get("enum_values");
        schema.enums.insert(name.clone(), EnumType { name, values });
    }

    // Load sequences
    let rows = client
        .query(
            "SELECT sequencename, data_type, start_value, increment_by, min_value, max_value \
             FROM pg_sequences \
             WHERE schemaname = 'public' \
             ORDER BY sequencename",
            &[],
        )
        .await?;

    for row in &rows {
        let name: String = row.get("sequencename");
        let data_type: String = row.get("data_type");
        let start_value: i64 = row.get("start_value");
        let increment: i64 = row.get("increment_by");
        let min_value: i64 = row.get("min_value");
        let max_value: i64 = row.get("max_value");
        schema.sequences.insert(
            name.clone(),
            Sequence {
                name,
                data_type,
                start_value,
                increment,
                min_value,
                max_value,
            },
        );
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
        regex::Regex::new(r"::\w[\w\s]*(?:\([\d,]+\))?").expect("hardcoded regex must compile")
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
    // PostgreSQL indexdef format: CREATE [UNIQUE] INDEX name ON [schema.]table [USING method] (columns)
    // Use ASCII case-insensitive search to find " ON " without byte-offset issues from
    // to_uppercase() on non-ASCII characters (e.g. 'ı' → 'I' changes byte length).
    let on_pos = match indexdef
        .as_bytes()
        .windows(4)
        .position(|w| w.eq_ignore_ascii_case(b" ON "))
    {
        Some(p) => p,
        None => return Vec::new(),
    };

    // Find the opening '(' of the column list, skipping any '(' inside double-quoted
    // identifiers (e.g. table names containing parentheses).
    let after_on = &indexdef[on_pos + 4..];
    let open = match find_unquoted_char(after_on, '(') {
        Some(p) => on_pos + 4 + p,
        None => return Vec::new(),
    };

    // Walk forward to find the matching close paren, skipping quoted content.
    let close = match find_matching_close_paren(indexdef, open) {
        Some(p) => p,
        None => return Vec::new(),
    };

    let cols_str = &indexdef[open + 1..close];

    // Split on commas at top level (depth 0), quote-aware.
    let mut result = Vec::new();
    let mut current = String::new();
    let mut paren_depth: i32 = 0;
    let mut in_quotes = false;
    for ch in cols_str.chars() {
        if ch == '"' {
            in_quotes = !in_quotes;
            current.push(ch);
        } else if in_quotes {
            current.push(ch);
        } else {
            match ch {
                '(' => {
                    paren_depth += 1;
                    current.push(ch);
                }
                ')' => {
                    paren_depth -= 1;
                    current.push(ch);
                }
                ',' if paren_depth == 0 => {
                    result.push(current.trim().to_string());
                    current.clear();
                }
                _ => current.push(ch),
            }
        }
    }

    let trimmed = current.trim().to_string();
    if !trimmed.is_empty() {
        result.push(trimmed);
    }

    result
}

/// Find the byte position of `target` that is not inside double-quoted identifiers.
fn find_unquoted_char(s: &str, target: char) -> Option<usize> {
    let mut in_quotes = false;
    for (i, ch) in s.char_indices() {
        if ch == '"' {
            in_quotes = !in_quotes;
        } else if !in_quotes && ch == target {
            return Some(i);
        }
    }
    None
}

/// Find the matching close paren for the '(' at `open`, skipping quoted content.
fn find_matching_close_paren(s: &str, open: usize) -> Option<usize> {
    let mut depth = 1;
    let mut in_quotes = false;
    for (i, ch) in s[open + 1..].char_indices() {
        if ch == '"' {
            in_quotes = !in_quotes;
        } else if !in_quotes {
            match ch {
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        return Some(open + 1 + i);
                    }
                }
                _ => {}
            }
        }
    }
    None
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

    #[test]
    fn test_parse_expression_index_columns() {
        assert_eq!(
            parse_index_columns("CREATE INDEX idx_lower_email ON users USING btree (lower(email))"),
            vec!["lower(email)"]
        );
    }

    #[test]
    fn test_parse_sorted_index_columns() {
        assert_eq!(
            parse_index_columns("CREATE INDEX idx ON events USING btree (created_at DESC)"),
            vec!["created_at DESC"]
        );
        assert_eq!(
            parse_index_columns(
                "CREATE INDEX idx ON events USING btree (user_id ASC, created_at DESC)"
            ),
            vec!["user_id ASC", "created_at DESC"]
        );
    }

    #[test]
    fn test_parse_index_with_quoted_identifiers() {
        assert_eq!(
            parse_index_columns("CREATE INDEX idx ON users USING btree (\"Email\", \"FirstName\")"),
            vec!["\"Email\"", "\"FirstName\""]
        );
    }

    #[test]
    fn test_parse_index_with_non_ascii_identifiers() {
        assert_eq!(
            parse_index_columns(
                "CREATE INDEX \"ındex_türkçe\" ON \"schéma\".\"tablo\" USING btree (\"sütun\")"
            ),
            vec!["\"sütun\""]
        );
    }

    #[test]
    fn test_parse_index_table_name_with_parens() {
        assert_eq!(
            parse_index_columns("CREATE INDEX idx ON \"table(weird)\" USING btree (col1, col2)"),
            vec!["col1", "col2"]
        );
    }

    #[test]
    fn test_parse_index_quoted_parens_in_columns() {
        assert_eq!(
            parse_index_columns("CREATE INDEX idx ON t USING btree (\"a)\", b)"),
            vec!["\"a)\"", "b"]
        );
    }

    #[test]
    fn test_parse_index_no_parens_returns_empty() {
        assert!(parse_index_columns("not a valid index def").is_empty());
    }

    #[test]
    fn test_normalize_default_with_cast() {
        assert_eq!(normalize_default("'active'::character varying"), "active");
        assert_eq!(normalize_default("0::integer"), "0");
    }

    #[test]
    fn test_normalize_default_preserves_functions() {
        assert_eq!(normalize_default("now()"), "now()");
        assert_eq!(normalize_default("gen_random_uuid()"), "gen_random_uuid()");
    }

    #[test]
    fn test_normalize_type_all_variants() {
        assert_eq!(normalize_type("time without time zone", None), "time");
        assert_eq!(normalize_type("time with time zone", None), "timetz");
        assert_eq!(normalize_type("double precision", None), "float8");
        assert_eq!(normalize_type("real", None), "float4");
        assert_eq!(normalize_type("smallint", None), "smallint");
        assert_eq!(normalize_type("character", Some(1)), "char(1)");
        assert_eq!(normalize_type("character", None), "char");
    }
}
