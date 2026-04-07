use crate::config::IgnoreConfig;
use crate::model::Schema;

/// Remove ignored tables and columns from a schema based on ignore rules.
pub fn apply_ignore(schema: &mut Schema, ignore: &IgnoreConfig) {
    // Remove ignored tables
    for table_name in &ignore.tables {
        schema.tables.remove(table_name);
    }

    // Remove ignored columns
    if !ignore.columns.is_empty() {
        let table_names: Vec<String> = schema.tables.keys().cloned().collect();
        for table_name in &table_names {
            let table = schema.tables.get_mut(table_name).unwrap();
            let col_names: Vec<String> = table.columns.keys().cloned().collect();
            let mut removed_columns: Vec<String> = Vec::new();

            for col_name in &col_names {
                if ignore
                    .columns
                    .iter()
                    .any(|pattern| matches_column_pattern(table_name, col_name, pattern))
                {
                    table.columns.remove(col_name);
                    removed_columns.push(col_name.clone());
                }
            }

            if !removed_columns.is_empty() {
                table.indexes.retain(|_, index| {
                    !index
                        .columns
                        .iter()
                        .any(|index_col| removed_columns.iter().any(|removed| removed == index_col))
                });
            }
        }
    }
}

/// Match a column against a pattern.
///
/// Supported patterns:
/// - `*.column_name` — matches column in all tables
/// - `table_name.*` — matches all columns in a specific table
/// - `table_name.column_name` — matches exact table.column pair
fn matches_column_pattern(table: &str, column: &str, pattern: &str) -> bool {
    let Some((pat_table, pat_column)) = pattern.split_once('.') else {
        return false;
    };

    let table_matches = pat_table == "*" || pat_table == table;
    let column_matches = pat_column == "*" || pat_column == column;

    table_matches && column_matches
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Column, Index, Table};

    fn make_column(name: &str) -> Column {
        Column {
            name: name.to_string(),
            data_type: "text".to_string(),
            is_nullable: true,
            default: None,
        }
    }

    fn make_index(table: &str, name: &str, columns: Vec<&str>) -> Index {
        Index {
            name: name.to_string(),
            table_name: table.to_string(),
            columns: columns
                .into_iter()
                .map(std::string::ToString::to_string)
                .collect(),
            is_unique: false,
        }
    }

    #[test]
    fn wildcard_column_matches_all_tables() {
        assert!(matches_column_pattern(
            "users",
            "created_at",
            "*.created_at"
        ));
        assert!(matches_column_pattern(
            "orders",
            "created_at",
            "*.created_at"
        ));
        assert!(!matches_column_pattern("users", "email", "*.created_at"));
    }

    #[test]
    fn wildcard_table_matches_all_columns() {
        assert!(matches_column_pattern("sessions", "token", "sessions.*"));
        assert!(matches_column_pattern(
            "sessions",
            "expires_at",
            "sessions.*"
        ));
        assert!(!matches_column_pattern("users", "email", "sessions.*"));
    }

    #[test]
    fn exact_match() {
        assert!(matches_column_pattern(
            "users",
            "password_hash",
            "users.password_hash"
        ));
        assert!(!matches_column_pattern(
            "orders",
            "password_hash",
            "users.password_hash"
        ));
    }

    #[test]
    fn invalid_pattern_no_dot() {
        assert!(!matches_column_pattern("users", "email", "no_dot_pattern"));
    }

    #[test]
    fn apply_ignore_removes_tables() {
        let mut schema = Schema::new();
        let mut t = Table::new("users");
        t.columns.insert("id".into(), make_column("id"));
        schema.tables.insert("users".into(), t);

        let mut t = Table::new("_migrations");
        t.columns.insert("id".into(), make_column("id"));
        schema.tables.insert("_migrations".into(), t);

        let ignore = IgnoreConfig {
            tables: vec!["_migrations".into()],
            columns: vec![],
        };

        apply_ignore(&mut schema, &ignore);
        assert_eq!(schema.tables.len(), 1);
        assert!(schema.tables.contains_key("users"));
        assert!(!schema.tables.contains_key("_migrations"));
    }

    #[test]
    fn apply_ignore_removes_columns() {
        let mut schema = Schema::new();
        let mut t = Table::new("users");
        t.columns.insert("id".into(), make_column("id"));
        t.columns
            .insert("created_at".into(), make_column("created_at"));
        t.columns.insert("email".into(), make_column("email"));
        schema.tables.insert("users".into(), t);

        let mut t = Table::new("orders");
        t.columns.insert("id".into(), make_column("id"));
        t.columns
            .insert("created_at".into(), make_column("created_at"));
        schema.tables.insert("orders".into(), t);

        let ignore = IgnoreConfig {
            tables: vec![],
            columns: vec!["*.created_at".into()],
        };

        apply_ignore(&mut schema, &ignore);
        assert_eq!(schema.tables["users"].columns.len(), 2); // id, email
        assert!(!schema.tables["users"].columns.contains_key("created_at"));
        assert_eq!(schema.tables["orders"].columns.len(), 1); // id
        assert!(!schema.tables["orders"].columns.contains_key("created_at"));
    }

    #[test]
    fn apply_ignore_table_wildcard_removes_all_columns() {
        let mut schema = Schema::new();
        let mut t = Table::new("sessions");
        t.columns.insert("id".into(), make_column("id"));
        t.columns.insert("token".into(), make_column("token"));
        schema.tables.insert("sessions".into(), t);

        let ignore = IgnoreConfig {
            tables: vec![],
            columns: vec!["sessions.*".into()],
        };

        apply_ignore(&mut schema, &ignore);
        assert!(schema.tables["sessions"].columns.is_empty());
    }

    #[test]
    fn apply_ignore_removes_indexes_on_ignored_columns() {
        let mut schema = Schema::new();
        let mut t = Table::new("users");
        t.columns.insert("id".into(), make_column("id"));
        t.columns.insert("email".into(), make_column("email"));
        t.columns
            .insert("created_at".into(), make_column("created_at"));
        t.indexes.insert(
            "idx_users_email".into(),
            make_index("users", "idx_users_email", vec!["email"]),
        );
        t.indexes.insert(
            "idx_users_created_at".into(),
            make_index("users", "idx_users_created_at", vec!["created_at"]),
        );
        t.indexes.insert(
            "idx_users_email_created_at".into(),
            make_index(
                "users",
                "idx_users_email_created_at",
                vec!["email", "created_at"],
            ),
        );
        schema.tables.insert("users".into(), t);

        let ignore = IgnoreConfig {
            tables: vec![],
            columns: vec!["*.created_at".into()],
        };

        apply_ignore(&mut schema, &ignore);

        let users = &schema.tables["users"];
        assert!(!users.columns.contains_key("created_at"));
        assert!(users.indexes.contains_key("idx_users_email"));
        assert!(!users.indexes.contains_key("idx_users_created_at"));
        assert!(!users.indexes.contains_key("idx_users_email_created_at"));
    }
}
