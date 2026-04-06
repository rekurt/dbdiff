use serde::Serialize;

use crate::model::{Column, Index, Schema, Table};

/// The result of comparing two schemas.
#[derive(Debug, Clone, Serialize)]
pub struct SchemaDiff {
    pub added_tables: Vec<Table>,
    pub removed_tables: Vec<Table>,
    pub modified_tables: Vec<TableDiff>,
    pub unchanged_tables: Vec<String>,
}

impl SchemaDiff {
    /// Returns `true` if there are no differences between the two schemas.
    pub fn is_empty(&self) -> bool {
        self.added_tables.is_empty()
            && self.removed_tables.is_empty()
            && self.modified_tables.is_empty()
    }
}

/// The diff for a single table that exists in both schemas.
#[derive(Debug, Clone, Serialize)]
pub struct TableDiff {
    pub table_name: String,
    pub added_columns: Vec<Column>,
    pub removed_columns: Vec<Column>,
    pub modified_columns: Vec<ColumnDiff>,
    pub unchanged_columns: Vec<String>,
    pub added_indexes: Vec<Index>,
    pub removed_indexes: Vec<Index>,
}

impl TableDiff {
    pub fn is_empty(&self) -> bool {
        self.added_columns.is_empty()
            && self.removed_columns.is_empty()
            && self.modified_columns.is_empty()
            && self.added_indexes.is_empty()
            && self.removed_indexes.is_empty()
    }
}

/// A column that exists in both schemas but with different definitions.
#[derive(Debug, Clone, Serialize)]
pub struct ColumnDiff {
    pub old: Column,
    pub new: Column,
}

/// Compare two schemas and produce a diff.
///
/// `left` is the current state (e.g. production).
/// `right` is the desired state (e.g. staging or .sql file).
/// The generated migration will bring `left` to match `right`.
pub fn diff_schemas(left: &Schema, right: &Schema) -> SchemaDiff {
    let mut added_tables = Vec::new();
    let mut removed_tables = Vec::new();
    let mut modified_tables = Vec::new();
    let mut unchanged_tables = Vec::new();

    // Tables only in right → added
    for (name, table) in &right.tables {
        if !left.tables.contains_key(name) {
            added_tables.push(table.clone());
        }
    }

    // Tables only in left → removed
    for (name, table) in &left.tables {
        if !right.tables.contains_key(name) {
            removed_tables.push(table.clone());
        }
    }

    // Tables in both → compare
    for (name, left_table) in &left.tables {
        if let Some(right_table) = right.tables.get(name) {
            let table_diff = diff_tables(name, left_table, right_table);
            if table_diff.is_empty() {
                unchanged_tables.push(name.clone());
            } else {
                modified_tables.push(table_diff);
            }
        }
    }

    SchemaDiff {
        added_tables,
        removed_tables,
        modified_tables,
        unchanged_tables,
    }
}

fn diff_tables(name: &str, left: &Table, right: &Table) -> TableDiff {
    let mut added_columns = Vec::new();
    let mut removed_columns = Vec::new();
    let mut modified_columns = Vec::new();
    let mut unchanged_columns = Vec::new();

    // Columns only in right → added
    for (col_name, col) in &right.columns {
        if !left.columns.contains_key(col_name) {
            added_columns.push(col.clone());
        }
    }

    // Columns only in left → removed
    for (col_name, col) in &left.columns {
        if !right.columns.contains_key(col_name) {
            removed_columns.push(col.clone());
        }
    }

    // Columns in both → compare
    for (col_name, left_col) in &left.columns {
        if let Some(right_col) = right.columns.get(col_name) {
            if left_col != right_col {
                modified_columns.push(ColumnDiff {
                    old: left_col.clone(),
                    new: right_col.clone(),
                });
            } else {
                unchanged_columns.push(col_name.clone());
            }
        }
    }

    // Indexes
    let mut added_indexes = Vec::new();
    let mut removed_indexes = Vec::new();

    for (idx_name, idx) in &right.indexes {
        if !left.indexes.contains_key(idx_name) {
            added_indexes.push(idx.clone());
        } else if left.indexes.get(idx_name) != Some(idx) {
            // Changed index: drop old + create new
            removed_indexes.push(left.indexes[idx_name].clone());
            added_indexes.push(idx.clone());
        }
    }

    for (idx_name, idx) in &left.indexes {
        if !right.indexes.contains_key(idx_name) {
            removed_indexes.push(idx.clone());
        }
    }

    TableDiff {
        table_name: name.to_string(),
        added_columns,
        removed_columns,
        modified_columns,
        unchanged_columns,
        added_indexes,
        removed_indexes,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Column, Index, Schema, Table};

    fn make_column(name: &str, data_type: &str, nullable: bool, default: Option<&str>) -> Column {
        Column {
            name: name.to_string(),
            data_type: data_type.to_string(),
            is_nullable: nullable,
            default: default.map(String::from),
        }
    }

    fn make_index(name: &str, table: &str, columns: &[&str], unique: bool) -> Index {
        Index {
            name: name.to_string(),
            table_name: table.to_string(),
            columns: columns.iter().map(|s| s.to_string()).collect(),
            is_unique: unique,
        }
    }

    #[test]
    fn identical_schemas_produce_empty_diff() {
        let mut table = Table::new("users");
        table
            .columns
            .insert("id".into(), make_column("id", "integer", false, None));
        table.columns.insert(
            "email".into(),
            make_column("email", "varchar(255)", false, None),
        );

        let mut schema = Schema::new();
        schema.tables.insert("users".into(), table);

        let diff = diff_schemas(&schema, &schema);
        assert!(diff.is_empty());
        assert_eq!(diff.unchanged_tables, vec!["users"]);
    }

    #[test]
    fn added_table_detected() {
        let left = Schema::new();

        let mut right = Schema::new();
        let mut table = Table::new("orders");
        table
            .columns
            .insert("id".into(), make_column("id", "integer", false, None));
        right.tables.insert("orders".into(), table);

        let diff = diff_schemas(&left, &right);
        assert_eq!(diff.added_tables.len(), 1);
        assert_eq!(diff.added_tables[0].name, "orders");
    }

    #[test]
    fn removed_table_detected() {
        let mut left = Schema::new();
        let mut table = Table::new("legacy");
        table
            .columns
            .insert("id".into(), make_column("id", "integer", false, None));
        left.tables.insert("legacy".into(), table);

        let right = Schema::new();

        let diff = diff_schemas(&left, &right);
        assert_eq!(diff.removed_tables.len(), 1);
        assert_eq!(diff.removed_tables[0].name, "legacy");
    }

    #[test]
    fn added_column_detected() {
        let mut left_table = Table::new("users");
        left_table
            .columns
            .insert("id".into(), make_column("id", "integer", false, None));

        let mut right_table = Table::new("users");
        right_table
            .columns
            .insert("id".into(), make_column("id", "integer", false, None));
        right_table.columns.insert(
            "email".into(),
            make_column("email", "varchar(255)", false, None),
        );

        let mut left = Schema::new();
        left.tables.insert("users".into(), left_table);
        let mut right = Schema::new();
        right.tables.insert("users".into(), right_table);

        let diff = diff_schemas(&left, &right);
        assert_eq!(diff.modified_tables.len(), 1);
        assert_eq!(diff.modified_tables[0].added_columns.len(), 1);
        assert_eq!(diff.modified_tables[0].added_columns[0].name, "email");
    }

    #[test]
    fn removed_column_detected() {
        let mut left_table = Table::new("users");
        left_table
            .columns
            .insert("id".into(), make_column("id", "integer", false, None));
        left_table.columns.insert(
            "legacy_field".into(),
            make_column("legacy_field", "text", true, None),
        );

        let mut right_table = Table::new("users");
        right_table
            .columns
            .insert("id".into(), make_column("id", "integer", false, None));

        let mut left = Schema::new();
        left.tables.insert("users".into(), left_table);
        let mut right = Schema::new();
        right.tables.insert("users".into(), right_table);

        let diff = diff_schemas(&left, &right);
        assert_eq!(diff.modified_tables.len(), 1);
        assert_eq!(diff.modified_tables[0].removed_columns.len(), 1);
        assert_eq!(
            diff.modified_tables[0].removed_columns[0].name,
            "legacy_field"
        );
    }

    #[test]
    fn modified_column_detected() {
        let mut left_table = Table::new("users");
        left_table.columns.insert(
            "email".into(),
            make_column("email", "varchar(100)", false, None),
        );

        let mut right_table = Table::new("users");
        right_table.columns.insert(
            "email".into(),
            make_column("email", "varchar(255)", false, None),
        );

        let mut left = Schema::new();
        left.tables.insert("users".into(), left_table);
        let mut right = Schema::new();
        right.tables.insert("users".into(), right_table);

        let diff = diff_schemas(&left, &right);
        assert_eq!(diff.modified_tables.len(), 1);
        assert_eq!(diff.modified_tables[0].modified_columns.len(), 1);
        assert_eq!(
            diff.modified_tables[0].modified_columns[0].old.data_type,
            "varchar(100)"
        );
        assert_eq!(
            diff.modified_tables[0].modified_columns[0].new.data_type,
            "varchar(255)"
        );
    }

    #[test]
    fn added_index_detected() {
        let mut left_table = Table::new("orders");
        left_table
            .columns
            .insert("id".into(), make_column("id", "integer", false, None));

        let mut right_table = Table::new("orders");
        right_table
            .columns
            .insert("id".into(), make_column("id", "integer", false, None));
        right_table.indexes.insert(
            "idx_orders_id".into(),
            make_index("idx_orders_id", "orders", &["id"], false),
        );

        let mut left = Schema::new();
        left.tables.insert("orders".into(), left_table);
        let mut right = Schema::new();
        right.tables.insert("orders".into(), right_table);

        let diff = diff_schemas(&left, &right);
        assert_eq!(diff.modified_tables.len(), 1);
        assert_eq!(diff.modified_tables[0].added_indexes.len(), 1);
        assert_eq!(
            diff.modified_tables[0].added_indexes[0].name,
            "idx_orders_id"
        );
    }

    #[test]
    fn complex_diff_scenario() {
        // Left: users(id, email, payment_date), orders(id)
        // Right: users(id, email, deleted_at), orders(id, paid_at) + index
        let mut left = Schema::new();
        {
            let mut users = Table::new("users");
            users
                .columns
                .insert("id".into(), make_column("id", "integer", false, None));
            users.columns.insert(
                "email".into(),
                make_column("email", "varchar(255)", false, None),
            );
            users.columns.insert(
                "payment_date".into(),
                make_column("payment_date", "varchar(32)", true, None),
            );
            left.tables.insert("users".into(), users);

            let mut orders = Table::new("orders");
            orders
                .columns
                .insert("id".into(), make_column("id", "integer", false, None));
            left.tables.insert("orders".into(), orders);
        }

        let mut right = Schema::new();
        {
            let mut users = Table::new("users");
            users
                .columns
                .insert("id".into(), make_column("id", "integer", false, None));
            users.columns.insert(
                "email".into(),
                make_column("email", "varchar(255)", false, None),
            );
            users.columns.insert(
                "deleted_at".into(),
                make_column("deleted_at", "timestamptz", true, None),
            );
            right.tables.insert("users".into(), users);

            let mut orders = Table::new("orders");
            orders
                .columns
                .insert("id".into(), make_column("id", "integer", false, None));
            orders.columns.insert(
                "paid_at".into(),
                make_column("paid_at", "timestamptz", false, Some("now()")),
            );
            orders.indexes.insert(
                "idx_orders_paid_at".into(),
                make_index("idx_orders_paid_at", "orders", &["paid_at"], false),
            );
            right.tables.insert("orders".into(), orders);
        }

        let diff = diff_schemas(&left, &right);
        assert!(!diff.is_empty());
        assert_eq!(diff.modified_tables.len(), 2);

        // Users: +deleted_at, -payment_date
        let users_diff = diff
            .modified_tables
            .iter()
            .find(|d| d.table_name == "users")
            .unwrap();
        assert_eq!(users_diff.added_columns.len(), 1);
        assert_eq!(users_diff.added_columns[0].name, "deleted_at");
        assert_eq!(users_diff.removed_columns.len(), 1);
        assert_eq!(users_diff.removed_columns[0].name, "payment_date");

        // Orders: +paid_at, +idx_orders_paid_at
        let orders_diff = diff
            .modified_tables
            .iter()
            .find(|d| d.table_name == "orders")
            .unwrap();
        assert_eq!(orders_diff.added_columns.len(), 1);
        assert_eq!(orders_diff.added_columns[0].name, "paid_at");
        assert_eq!(orders_diff.added_indexes.len(), 1);
        assert_eq!(orders_diff.added_indexes[0].name, "idx_orders_paid_at");
    }
}
