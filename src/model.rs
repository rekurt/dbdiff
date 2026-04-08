use std::collections::BTreeMap;

use serde::Serialize;

/// A complete database schema — one side of a comparison.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Schema {
    pub tables: BTreeMap<String, Table>,
    pub views: BTreeMap<String, View>,
    pub enums: BTreeMap<String, EnumType>,
    pub sequences: BTreeMap<String, Sequence>,
}

impl Schema {
    pub fn new() -> Self {
        Self::default()
    }
}

/// A single database table with its columns, indexes, and constraints.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Table {
    pub name: String,
    pub columns: BTreeMap<String, Column>,
    pub indexes: BTreeMap<String, Index>,
    pub constraints: BTreeMap<String, Constraint>,
}

impl Table {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            columns: BTreeMap::new(),
            indexes: BTreeMap::new(),
            constraints: BTreeMap::new(),
        }
    }
}

/// A table column with its type, nullability, and default value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, serde::Deserialize)]
pub struct Column {
    pub name: String,
    pub data_type: String,
    pub is_nullable: bool,
    pub default: Option<String>,
}

impl Column {
    /// Format column definition as SQL fragment: `name type [NOT NULL] [DEFAULT expr]`
    pub fn definition(&self) -> String {
        let mut def = format!("{} {}", self.name, self.data_type);
        if !self.is_nullable {
            def.push_str(" NOT NULL");
        }
        if let Some(ref default) = self.default {
            def.push_str(&format!(" DEFAULT {default}"));
        }
        def
    }
}

/// A table index.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, serde::Deserialize)]
pub struct Index {
    pub name: String,
    pub table_name: String,
    pub columns: Vec<String>,
    pub is_unique: bool,
}

impl Index {
    /// Format index definition: `idx_name ON table(col1, col2)`
    pub fn definition(&self) -> String {
        let cols = self.columns.join(", ");
        format!("{} ON {}({})", self.name, self.table_name, cols)
    }
}

/// A table constraint (PK, FK, unique, check).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, serde::Deserialize)]
pub struct Constraint {
    pub name: String,
    pub table_name: String,
    pub kind: ConstraintKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, serde::Deserialize)]
pub enum ConstraintKind {
    PrimaryKey {
        columns: Vec<String>,
    },
    ForeignKey {
        columns: Vec<String>,
        ref_table: String,
        ref_columns: Vec<String>,
        on_delete: Option<String>,
        on_update: Option<String>,
    },
    Unique {
        columns: Vec<String>,
    },
    Check {
        expression: String,
    },
}

impl Constraint {
    pub fn definition(&self) -> String {
        match &self.kind {
            ConstraintKind::PrimaryKey { columns } => {
                format!("PRIMARY KEY ({})", columns.join(", "))
            }
            ConstraintKind::ForeignKey {
                columns,
                ref_table,
                ref_columns,
                ..
            } => {
                format!(
                    "FK ({}) -> {}({})",
                    columns.join(", "),
                    ref_table,
                    ref_columns.join(", ")
                )
            }
            ConstraintKind::Unique { columns } => {
                format!("UNIQUE ({})", columns.join(", "))
            }
            ConstraintKind::Check { expression } => {
                format!("CHECK ({expression})")
            }
        }
    }
}

/// A database view.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, serde::Deserialize)]
pub struct View {
    pub name: String,
    pub definition: String,
}

/// A PostgreSQL enum type.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, serde::Deserialize)]
pub struct EnumType {
    pub name: String,
    pub values: Vec<String>,
}

/// A database sequence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, serde::Deserialize)]
pub struct Sequence {
    pub name: String,
    pub data_type: String,
    pub start_value: i64,
    pub increment: i64,
    pub min_value: i64,
    pub max_value: i64,
}

/// Deserializable schema for snapshot import.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, serde::Deserialize)]
pub struct SchemaSnapshot {
    pub tables: BTreeMap<String, TableSnapshot>,
    #[serde(default)]
    pub views: BTreeMap<String, View>,
    #[serde(default)]
    pub enums: BTreeMap<String, EnumType>,
    #[serde(default)]
    pub sequences: BTreeMap<String, Sequence>,
}

/// Snapshot of a table for JSON serialization/deserialization.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, serde::Deserialize)]
pub struct TableSnapshot {
    pub name: String,
    pub columns: BTreeMap<String, Column>,
    #[serde(default)]
    pub indexes: BTreeMap<String, Index>,
    #[serde(default)]
    pub constraints: BTreeMap<String, Constraint>,
}

impl From<SchemaSnapshot> for Schema {
    fn from(snap: SchemaSnapshot) -> Self {
        let mut schema = Schema::new();
        for (name, ts) in snap.tables {
            schema.tables.insert(
                name,
                Table {
                    name: ts.name,
                    columns: ts.columns,
                    indexes: ts.indexes,
                    constraints: ts.constraints,
                },
            );
        }
        schema.views = snap.views;
        schema.enums = snap.enums;
        schema.sequences = snap.sequences;
        schema
    }
}

impl From<&Schema> for SchemaSnapshot {
    fn from(schema: &Schema) -> Self {
        let mut tables = BTreeMap::new();
        for (name, t) in &schema.tables {
            tables.insert(
                name.clone(),
                TableSnapshot {
                    name: t.name.clone(),
                    columns: t.columns.clone(),
                    indexes: t.indexes.clone(),
                    constraints: t.constraints.clone(),
                },
            );
        }
        SchemaSnapshot {
            tables,
            views: schema.views.clone(),
            enums: schema.enums.clone(),
            sequences: schema.sequences.clone(),
        }
    }
}
