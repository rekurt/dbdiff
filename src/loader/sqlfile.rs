use regex::Regex;

use crate::error::DbDiffError;
use crate::model::{Column, Index, Schema, Table};

/// Load a schema by parsing a `.sql` file containing CREATE TABLE and CREATE INDEX statements.
pub fn load(content: &str) -> Result<Schema, DbDiffError> {
    let mut schema = Schema::new();

    parse_create_tables(content, &mut schema)?;
    parse_create_indexes(content, &mut schema)?;

    Ok(schema)
}

/// Load a schema from a file path.
pub fn load_file(path: &str) -> Result<Schema, DbDiffError> {
    let content = std::fs::read_to_string(path)?;
    load(&content)
}

/// Parse all CREATE TABLE statements from SQL content.
fn parse_create_tables(content: &str, schema: &mut Schema) -> Result<(), DbDiffError> {
    let re = Regex::new(r"(?i)CREATE\s+TABLE\s+(?:IF\s+NOT\s+EXISTS\s+)?(\w+)\s*\(")
        .map_err(|e| DbDiffError::SqlParse(e.to_string()))?;

    for cap in re.captures_iter(content) {
        let table_name = cap[1].to_string();
        let match_end = cap.get(0).expect("group 0 always exists").end();

        // Find the matching closing parenthesis, handling nested parens
        let body = extract_parenthesized_body(content, match_end)?;

        let mut table = Table::new(&table_name);
        parse_column_definitions(&body, &mut table)?;
        schema.tables.insert(table_name, table);
    }

    Ok(())
}

/// Extract the body inside parentheses, handling nested parens (e.g., DEFAULT func()).
fn extract_parenthesized_body(content: &str, start: usize) -> Result<String, DbDiffError> {
    let bytes = content.as_bytes();
    let mut depth = 1;
    let mut pos = start;

    while pos < bytes.len() && depth > 0 {
        match bytes[pos] {
            b'(' => depth += 1,
            b')' => depth -= 1,
            b'\'' => {
                // Skip string literals
                pos += 1;
                while pos < bytes.len() && bytes[pos] != b'\'' {
                    if bytes[pos] == b'\\' {
                        pos += 1; // skip escaped char
                    }
                    pos += 1;
                }
            }
            b'-' if pos + 1 < bytes.len() && bytes[pos + 1] == b'-' => {
                // Skip single-line comments
                while pos < bytes.len() && bytes[pos] != b'\n' {
                    pos += 1;
                }
            }
            _ => {}
        }
        pos += 1;
    }

    if depth != 0 {
        return Err(DbDiffError::SqlParse(
            "Unmatched parenthesis in CREATE TABLE statement".to_string(),
        ));
    }

    // pos is one past the closing paren
    Ok(content[start..pos - 1].to_string())
}

/// Parse column definitions from the body of a CREATE TABLE.
fn parse_column_definitions(body: &str, table: &mut Table) -> Result<(), DbDiffError> {
    let parts = split_top_level(body, ',');

    let constraint_re =
        Regex::new(r"(?i)^\s*(CONSTRAINT|PRIMARY\s+KEY|FOREIGN\s+KEY|UNIQUE|CHECK|EXCLUDE)")
            .map_err(|e| DbDiffError::SqlParse(e.to_string()))?;

    let col_re = Regex::new(
        r"(?i)^\s*(\w+)\s+([\w]+(?:\s*\([^)]*\))?(?:\s+(?:varying|precision|without|with|time|zone|double)\s*(?:\([^)]*\))?)*)\s*(.*)",
    )
    .map_err(|e| DbDiffError::SqlParse(e.to_string()))?;

    for part in &parts {
        let trimmed = part.trim();
        if trimmed.is_empty() || constraint_re.is_match(trimmed) {
            continue;
        }

        if let Some(cap) = col_re.captures(trimmed) {
            let name = cap[1].to_string();
            let raw_type = cap[2].trim().to_string();
            let modifiers = cap.get(3).map(|m| m.as_str()).unwrap_or("");

            let data_type = normalize_sql_type(&raw_type);
            let is_nullable = !modifiers.to_uppercase().contains("NOT NULL");
            let default = parse_default(modifiers);

            let column = Column {
                name: name.clone(),
                data_type,
                is_nullable,
                default,
            };
            table.columns.insert(name, column);
        }
    }

    Ok(())
}

/// Split a string by a delimiter, but only at the top level (not inside parentheses).
fn split_top_level(s: &str, delim: char) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut depth = 0;
    let mut in_string = false;

    for ch in s.chars() {
        if ch == '\'' && !in_string {
            in_string = true;
            current.push(ch);
        } else if ch == '\'' && in_string {
            in_string = false;
            current.push(ch);
        } else if in_string {
            current.push(ch);
        } else if ch == '(' {
            depth += 1;
            current.push(ch);
        } else if ch == ')' {
            depth -= 1;
            current.push(ch);
        } else if ch == delim && depth == 0 {
            parts.push(current.clone());
            current.clear();
        } else {
            current.push(ch);
        }
    }
    if !current.trim().is_empty() {
        parts.push(current);
    }
    parts
}

/// Normalize SQL type names.
fn normalize_sql_type(raw: &str) -> String {
    let upper = raw.to_uppercase();
    let normalized = match upper.as_str() {
        "CHARACTER VARYING" => "varchar".to_string(),
        "BOOLEAN" => "bool".to_string(),
        "TIMESTAMP WITHOUT TIME ZONE" => "timestamp".to_string(),
        "TIMESTAMP WITH TIME ZONE" | "TIMESTAMPTZ" => "timestamptz".to_string(),
        "INTEGER" | "INT" => "integer".to_string(),
        "BIGINT" | "INT8" => "bigint".to_string(),
        "SMALLINT" | "INT2" => "smallint".to_string(),
        "DOUBLE PRECISION" | "FLOAT8" => "float8".to_string(),
        "REAL" | "FLOAT4" => "float4".to_string(),
        "SERIAL" => "serial".to_string(),
        "BIGSERIAL" => "bigserial".to_string(),
        _ => {
            // Handle types with length: VARCHAR(255), NUMERIC(10,2)
            if let Some(paren_pos) = raw.find('(') {
                let base = raw[..paren_pos].trim();
                let params = &raw[paren_pos..];
                let base_normalized = match base.to_uppercase().as_str() {
                    "CHARACTER VARYING" | "VARCHAR" => "varchar",
                    "CHARACTER" | "CHAR" => "char",
                    "NUMERIC" | "DECIMAL" => "numeric",
                    _ => base,
                };
                format!(
                    "{}{}",
                    base_normalized.to_lowercase(),
                    params.to_lowercase()
                )
            } else {
                raw.to_lowercase()
            }
        }
    };
    normalized
}

/// Extract DEFAULT value from column modifiers string.
fn parse_default(modifiers: &str) -> Option<String> {
    // Use ASCII case-insensitive search on the original string to avoid byte-offset
    // mismatch when non-ASCII characters change length under to_uppercase().
    let idx = modifiers
        .as_bytes()
        .windows(7)
        .position(|w| w.eq_ignore_ascii_case(b"DEFAULT"))?;
    let after = &modifiers[idx + 7..].trim_start();

    // Parse the default value — could be a function call, string, or simple value
    let mut result = String::new();
    let mut depth = 0;
    let mut in_string = false;

    for ch in after.chars() {
        if ch == '\'' {
            in_string = !in_string;
            result.push(ch);
        } else if in_string {
            result.push(ch);
        } else if ch == '(' {
            depth += 1;
            result.push(ch);
        } else if ch == ')' {
            if depth == 0 {
                break;
            }
            depth -= 1;
            result.push(ch);
        } else if depth == 0 && (ch == ',' || ch == ' ' || ch == '\n' || ch == '\t') {
            // Check if we've hit another keyword
            let remaining = after[result.len()..].trim_start().to_uppercase();
            if remaining.starts_with("NOT")
                || remaining.starts_with("NULL")
                || remaining.starts_with("PRIMARY")
                || remaining.starts_with("UNIQUE")
                || remaining.starts_with("CHECK")
                || remaining.starts_with("REFERENCES")
                || remaining.starts_with("CONSTRAINT")
            {
                break;
            }
            if ch == ',' {
                break;
            }
            result.push(ch);
        } else {
            result.push(ch);
        }
    }

    let trimmed = result.trim().to_string();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}

/// Parse all CREATE INDEX statements from SQL content.
fn parse_create_indexes(content: &str, schema: &mut Schema) -> Result<(), DbDiffError> {
    let re = Regex::new(
        r"(?i)CREATE\s+(UNIQUE\s+)?INDEX\s+(?:IF\s+NOT\s+EXISTS\s+)?(\w+)\s+ON\s+(\w+)\s*\(([^)]+)\)",
    )
    .map_err(|e| DbDiffError::SqlParse(e.to_string()))?;

    for cap in re.captures_iter(content) {
        let is_unique = cap.get(1).is_some();
        let index_name = cap[2].to_string();
        let table_name = cap[3].to_string();
        let columns: Vec<String> = cap[4].split(',').map(|c| c.trim().to_string()).collect();

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

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_simple_table() {
        let sql = "CREATE TABLE users (
            id serial NOT NULL,
            email varchar(255) NOT NULL,
            name text,
            created_at timestamptz NOT NULL DEFAULT now()
        );";

        let schema = load(sql).unwrap();
        assert_eq!(schema.tables.len(), 1);

        let table = &schema.tables["users"];
        assert_eq!(table.columns.len(), 4);

        let id = &table.columns["id"];
        assert_eq!(id.data_type, "serial");
        assert!(!id.is_nullable);

        let email = &table.columns["email"];
        assert_eq!(email.data_type, "varchar(255)");
        assert!(!email.is_nullable);

        let name = &table.columns["name"];
        assert_eq!(name.data_type, "text");
        assert!(name.is_nullable);

        let created = &table.columns["created_at"];
        assert_eq!(created.data_type, "timestamptz");
        assert!(!created.is_nullable);
        assert_eq!(created.default.as_deref(), Some("now()"));
    }

    #[test]
    fn parse_table_with_constraints_skipped() {
        let sql = "CREATE TABLE orders (
            id serial NOT NULL,
            user_id integer NOT NULL,
            total numeric(10,2) NOT NULL DEFAULT 0,
            PRIMARY KEY (id),
            FOREIGN KEY (user_id) REFERENCES users(id),
            CONSTRAINT positive_total CHECK (total >= 0)
        );";

        let schema = load(sql).unwrap();
        let table = &schema.tables["orders"];
        assert_eq!(table.columns.len(), 3);
        assert!(table.columns.contains_key("id"));
        assert!(table.columns.contains_key("user_id"));
        assert!(table.columns.contains_key("total"));
    }

    #[test]
    fn parse_indexes() {
        let sql = "CREATE TABLE orders (
            id serial NOT NULL,
            paid_at timestamptz
        );
        CREATE INDEX idx_orders_paid_at ON orders(paid_at);
        CREATE UNIQUE INDEX idx_orders_id ON orders(id);";

        let schema = load(sql).unwrap();
        let table = &schema.tables["orders"];
        assert_eq!(table.indexes.len(), 2);

        let idx = &table.indexes["idx_orders_paid_at"];
        assert!(!idx.is_unique);
        assert_eq!(idx.columns, vec!["paid_at"]);

        let unique_idx = &table.indexes["idx_orders_id"];
        assert!(unique_idx.is_unique);
    }

    #[test]
    fn parse_multiple_tables() {
        let sql = "
        CREATE TABLE users (
            id serial NOT NULL,
            email varchar(255) NOT NULL
        );

        CREATE TABLE orders (
            id serial NOT NULL,
            user_id integer NOT NULL,
            total numeric(10,2) NOT NULL DEFAULT 0
        );

        CREATE INDEX idx_orders_user_id ON orders(user_id);
        ";

        let schema = load(sql).unwrap();
        assert_eq!(schema.tables.len(), 2);
        assert!(schema.tables.contains_key("users"));
        assert!(schema.tables.contains_key("orders"));
        assert_eq!(schema.tables["orders"].indexes.len(), 1);
    }

    #[test]
    fn parse_if_not_exists() {
        let sql = "CREATE TABLE IF NOT EXISTS users (
            id serial NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_users_id ON users(id);";

        let schema = load(sql).unwrap();
        assert_eq!(schema.tables.len(), 1);
        assert_eq!(schema.tables["users"].indexes.len(), 1);
    }

    #[test]
    fn normalize_types() {
        assert_eq!(normalize_sql_type("VARCHAR(255)"), "varchar(255)");
        assert_eq!(normalize_sql_type("INTEGER"), "integer");
        assert_eq!(normalize_sql_type("BIGINT"), "bigint");
        assert_eq!(normalize_sql_type("BOOLEAN"), "bool");
        assert_eq!(normalize_sql_type("TIMESTAMPTZ"), "timestamptz");
        assert_eq!(normalize_sql_type("NUMERIC(10,2)"), "numeric(10,2)");
        assert_eq!(normalize_sql_type("text"), "text");
    }
}
