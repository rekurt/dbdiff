use std::collections::BTreeMap;

use serde::Serialize;

/// A complete database schema — one side of a comparison.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Schema {
    pub tables: BTreeMap<String, Table>,
}

impl Schema {
    pub fn new() -> Self {
        Self::default()
    }
}

/// A single database table with its columns and indexes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Table {
    pub name: String,
    pub columns: BTreeMap<String, Column>,
    pub indexes: BTreeMap<String, Index>,
}

impl Table {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            columns: BTreeMap::new(),
            indexes: BTreeMap::new(),
        }
    }
}

/// A table column with its type, nullability, and default value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
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
