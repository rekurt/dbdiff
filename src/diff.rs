use serde::Serialize;

use crate::model::{Column, Constraint, EnumType, Index, Schema, Sequence, Table, View};

/// The result of comparing two schemas.
#[derive(Debug, Clone, Serialize)]
pub struct SchemaDiff {
    pub added_tables: Vec<Table>,
    pub removed_tables: Vec<Table>,
    pub modified_tables: Vec<TableDiff>,
    pub unchanged_tables: Vec<String>,
    pub added_views: Vec<View>,
    pub removed_views: Vec<View>,
    pub modified_views: Vec<ViewDiff>,
    pub added_enums: Vec<EnumType>,
    pub removed_enums: Vec<EnumType>,
    pub modified_enums: Vec<EnumDiff>,
    pub added_sequences: Vec<Sequence>,
    pub removed_sequences: Vec<Sequence>,
    pub modified_sequences: Vec<SequenceDiff>,
}

impl SchemaDiff {
    /// Returns `true` if there are no differences between the two schemas.
    pub fn is_empty(&self) -> bool {
        self.added_tables.is_empty()
            && self.removed_tables.is_empty()
            && self.modified_tables.is_empty()
            && self.added_views.is_empty()
            && self.removed_views.is_empty()
            && self.modified_views.is_empty()
            && self.added_enums.is_empty()
            && self.removed_enums.is_empty()
            && self.modified_enums.is_empty()
            && self.added_sequences.is_empty()
            && self.removed_sequences.is_empty()
            && self.modified_sequences.is_empty()
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
    pub added_constraints: Vec<Constraint>,
    pub removed_constraints: Vec<Constraint>,
}

impl TableDiff {
    pub fn is_empty(&self) -> bool {
        self.added_columns.is_empty()
            && self.removed_columns.is_empty()
            && self.modified_columns.is_empty()
            && self.added_indexes.is_empty()
            && self.removed_indexes.is_empty()
            && self.added_constraints.is_empty()
            && self.removed_constraints.is_empty()
    }
}

/// A column that exists in both schemas but with different definitions.
#[derive(Debug, Clone, Serialize)]
pub struct ColumnDiff {
    pub old: Column,
    pub new: Column,
}

/// A view that changed its definition.
#[derive(Debug, Clone, Serialize)]
pub struct ViewDiff {
    pub name: String,
    pub old_definition: String,
    pub new_definition: String,
}

/// An enum type that changed its values.
#[derive(Debug, Clone, Serialize)]
pub struct EnumDiff {
    pub name: String,
    pub added_values: Vec<String>,
    pub removed_values: Vec<String>,
}

/// A sequence that changed its properties.
#[derive(Debug, Clone, Serialize)]
pub struct SequenceDiff {
    pub name: String,
    pub old: Sequence,
    pub new: Sequence,
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

    // Tables only in right -> added
    for (name, table) in &right.tables {
        if !left.tables.contains_key(name) {
            added_tables.push(table.clone());
        }
    }

    // Tables only in left -> removed
    for (name, table) in &left.tables {
        if !right.tables.contains_key(name) {
            removed_tables.push(table.clone());
        }
    }

    // Tables in both -> compare
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

    // Views
    let (added_views, removed_views, modified_views) = diff_views(&left.views, &right.views);

    // Enums
    let (added_enums, removed_enums, modified_enums) = diff_enums(&left.enums, &right.enums);

    // Sequences
    let (added_sequences, removed_sequences, modified_sequences) =
        diff_sequences(&left.sequences, &right.sequences);

    SchemaDiff {
        added_tables,
        removed_tables,
        modified_tables,
        unchanged_tables,
        added_views,
        removed_views,
        modified_views,
        added_enums,
        removed_enums,
        modified_enums,
        added_sequences,
        removed_sequences,
        modified_sequences,
    }
}

fn diff_tables(name: &str, left: &Table, right: &Table) -> TableDiff {
    let mut added_columns = Vec::new();
    let mut removed_columns = Vec::new();
    let mut modified_columns = Vec::new();
    let mut unchanged_columns = Vec::new();

    for (col_name, col) in &right.columns {
        if !left.columns.contains_key(col_name) {
            added_columns.push(col.clone());
        }
    }

    for (col_name, col) in &left.columns {
        if !right.columns.contains_key(col_name) {
            removed_columns.push(col.clone());
        }
    }

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
            removed_indexes.push(left.indexes[idx_name].clone());
            added_indexes.push(idx.clone());
        }
    }

    for (idx_name, idx) in &left.indexes {
        if !right.indexes.contains_key(idx_name) {
            removed_indexes.push(idx.clone());
        }
    }

    // Constraints
    let mut added_constraints = Vec::new();
    let mut removed_constraints = Vec::new();

    for (c_name, c) in &right.constraints {
        if !left.constraints.contains_key(c_name) {
            added_constraints.push(c.clone());
        } else if left.constraints.get(c_name) != Some(c) {
            removed_constraints.push(left.constraints[c_name].clone());
            added_constraints.push(c.clone());
        }
    }

    for (c_name, c) in &left.constraints {
        if !right.constraints.contains_key(c_name) {
            removed_constraints.push(c.clone());
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
        added_constraints,
        removed_constraints,
    }
}

fn diff_views(
    left: &std::collections::BTreeMap<String, View>,
    right: &std::collections::BTreeMap<String, View>,
) -> (Vec<View>, Vec<View>, Vec<ViewDiff>) {
    let mut added = Vec::new();
    let mut removed = Vec::new();
    let mut modified = Vec::new();

    for (name, view) in right {
        if !left.contains_key(name) {
            added.push(view.clone());
        }
    }

    for (name, view) in left {
        if !right.contains_key(name) {
            removed.push(view.clone());
        }
    }

    for (name, left_view) in left {
        if let Some(right_view) = right.get(name) {
            let left_norm = normalize_whitespace(&left_view.definition);
            let right_norm = normalize_whitespace(&right_view.definition);
            if left_norm != right_norm {
                modified.push(ViewDiff {
                    name: name.clone(),
                    old_definition: left_view.definition.clone(),
                    new_definition: right_view.definition.clone(),
                });
            }
        }
    }

    (added, removed, modified)
}

fn diff_enums(
    left: &std::collections::BTreeMap<String, EnumType>,
    right: &std::collections::BTreeMap<String, EnumType>,
) -> (Vec<EnumType>, Vec<EnumType>, Vec<EnumDiff>) {
    let mut added = Vec::new();
    let mut removed = Vec::new();
    let mut modified = Vec::new();

    for (name, e) in right {
        if !left.contains_key(name) {
            added.push(e.clone());
        }
    }

    for (name, e) in left {
        if !right.contains_key(name) {
            removed.push(e.clone());
        }
    }

    for (name, left_enum) in left {
        if let Some(right_enum) = right.get(name) {
            if left_enum.values != right_enum.values {
                let added_values: Vec<String> = right_enum
                    .values
                    .iter()
                    .filter(|v| !left_enum.values.contains(v))
                    .cloned()
                    .collect();
                let removed_values: Vec<String> = left_enum
                    .values
                    .iter()
                    .filter(|v| !right_enum.values.contains(v))
                    .cloned()
                    .collect();
                modified.push(EnumDiff {
                    name: name.clone(),
                    added_values,
                    removed_values,
                });
            }
        }
    }

    (added, removed, modified)
}

fn diff_sequences(
    left: &std::collections::BTreeMap<String, Sequence>,
    right: &std::collections::BTreeMap<String, Sequence>,
) -> (Vec<Sequence>, Vec<Sequence>, Vec<SequenceDiff>) {
    let mut added = Vec::new();
    let mut removed = Vec::new();
    let mut modified = Vec::new();

    for (name, s) in right {
        if !left.contains_key(name) {
            added.push(s.clone());
        }
    }

    for (name, s) in left {
        if !right.contains_key(name) {
            removed.push(s.clone());
        }
    }

    for (name, left_seq) in left {
        if let Some(right_seq) = right.get(name) {
            if left_seq != right_seq {
                modified.push(SequenceDiff {
                    name: name.clone(),
                    old: left_seq.clone(),
                    new: right_seq.clone(),
                });
            }
        }
    }

    (added, removed, modified)
}

/// Normalize whitespace for view definition comparison.
fn normalize_whitespace(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Column, Constraint, ConstraintKind, Index, Schema, Table};

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

        let users_diff = diff
            .modified_tables
            .iter()
            .find(|d| d.table_name == "users")
            .unwrap();
        assert_eq!(users_diff.added_columns.len(), 1);
        assert_eq!(users_diff.added_columns[0].name, "deleted_at");
        assert_eq!(users_diff.removed_columns.len(), 1);
        assert_eq!(users_diff.removed_columns[0].name, "payment_date");

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

    #[test]
    fn constraint_diff_detected() {
        let mut left_table = Table::new("orders");
        left_table
            .columns
            .insert("id".into(), make_column("id", "integer", false, None));
        left_table.columns.insert(
            "user_id".into(),
            make_column("user_id", "integer", false, None),
        );

        let mut right_table = left_table.clone();
        right_table.constraints.insert(
            "fk_orders_user".into(),
            Constraint {
                name: "fk_orders_user".into(),
                table_name: "orders".into(),
                kind: ConstraintKind::ForeignKey {
                    columns: vec!["user_id".into()],
                    ref_table: "users".into(),
                    ref_columns: vec!["id".into()],
                    on_delete: Some("CASCADE".into()),
                    on_update: None,
                },
            },
        );

        let mut left = Schema::new();
        left.tables.insert("orders".into(), left_table);
        let mut right = Schema::new();
        right.tables.insert("orders".into(), right_table);

        let diff = diff_schemas(&left, &right);
        assert_eq!(diff.modified_tables.len(), 1);
        assert_eq!(diff.modified_tables[0].added_constraints.len(), 1);
        assert_eq!(
            diff.modified_tables[0].added_constraints[0].name,
            "fk_orders_user"
        );
    }

    #[test]
    fn view_diff_detected() {
        let mut left = Schema::new();
        left.views.insert(
            "active_users".into(),
            View {
                name: "active_users".into(),
                definition: "SELECT * FROM users WHERE active = true".into(),
            },
        );

        let mut right = Schema::new();
        right.views.insert(
            "active_users".into(),
            View {
                name: "active_users".into(),
                definition: "SELECT * FROM users WHERE active = true AND deleted_at IS NULL".into(),
            },
        );

        let diff = diff_schemas(&left, &right);
        assert_eq!(diff.modified_views.len(), 1);
        assert_eq!(diff.modified_views[0].name, "active_users");
    }

    #[test]
    fn enum_diff_detected() {
        let mut left = Schema::new();
        left.enums.insert(
            "status".into(),
            EnumType {
                name: "status".into(),
                values: vec!["active".into(), "inactive".into()],
            },
        );

        let mut right = Schema::new();
        right.enums.insert(
            "status".into(),
            EnumType {
                name: "status".into(),
                values: vec!["active".into(), "inactive".into(), "suspended".into()],
            },
        );

        let diff = diff_schemas(&left, &right);
        assert_eq!(diff.modified_enums.len(), 1);
        assert_eq!(diff.modified_enums[0].added_values, vec!["suspended"]);
        assert!(diff.modified_enums[0].removed_values.is_empty());
    }
}
