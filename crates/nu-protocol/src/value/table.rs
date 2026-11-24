//! Materialized, schema-bearing Nushell tables.

use crate::{Record, ShellError, Span, Type, Value, shell_error::generic::GenericError};
use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as _};
use std::{
    collections::HashSet,
    hash::{Hash, Hasher},
    slice, vec,
};

/// An owned table with an ordered schema and record rows.
///
/// Rows may omit columns from the schema, but cannot introduce columns outside it. All present
/// fields are normalized to schema order. Use [`Table::from_rows`] to infer a schema from rows in
/// stable, first-seen order.
#[derive(Clone, Debug)]
pub struct Table {
    columns: Vec<String>,
    rows: Vec<Value>,
}

impl Table {
    /// Create a table with an explicit ordered schema.
    pub fn new(columns: Vec<String>, rows: Vec<Value>, span: Span) -> Result<Self, ShellError> {
        validate_columns(&columns, span)?;
        let rows = rows
            .into_iter()
            .map(|row| normalize_row(row, &columns, span))
            .collect::<Result<_, _>>()?;
        Ok(Self { columns, rows })
    }

    /// Create a table by inferring an ordered union of all row columns.
    pub fn from_rows(rows: Vec<Value>, span: Span) -> Result<Self, ShellError> {
        let mut columns = Vec::new();
        let mut seen = HashSet::new();
        for row in &rows {
            let record = row.as_record()?;
            for column in record.columns() {
                if seen.insert(column.clone()) {
                    columns.push(column.clone());
                }
            }
        }
        Self::new(columns, rows, span)
    }

    /// Ordered table columns.
    pub fn columns(&self) -> &[String] {
        &self.columns
    }

    /// Materialized record rows.
    pub fn rows(&self) -> &[Value] {
        &self.rows
    }

    /// Mutably access rows for schema-aware cell-path operations.
    pub fn rows_mut(&mut self) -> &mut [Value] {
        &mut self.rows
    }

    /// Declare a newly inserted column after all existing columns.
    pub(crate) fn push_column(&mut self, column: String) {
        self.columns.push(column);
    }

    /// Consume the table and return its record rows.
    pub fn into_rows(self) -> Vec<Value> {
        self.rows
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    pub fn get(&self, index: usize) -> Option<&Value> {
        self.rows.get(index)
    }

    pub fn iter(&self) -> slice::Iter<'_, Value> {
        self.rows.iter()
    }

    /// Visit the values stored in row fields without exposing mutable row wrappers.
    ///
    /// Keeping the record wrappers private prevents generic recursive transforms from replacing a
    /// row with a non-record value and breaking the table's schema invariant.
    pub(crate) fn recurse_mut<E>(
        &mut self,
        f: &mut impl FnMut(&mut Value) -> Result<(), E>,
    ) -> Result<(), E> {
        for row in &mut self.rows {
            if let Value::Record { val, .. } = row {
                for (_, cell) in val.to_mut().iter_mut() {
                    cell.recurse_mut(f)?;
                }
            }
        }
        Ok(())
    }

    /// Normalize and validate one row against an ordered table schema.
    pub(crate) fn normalize_row(
        row: Value,
        columns: &[String],
        table_span: Span,
    ) -> Result<Value, ShellError> {
        normalize_row(row, columns, table_span)
    }

    /// Calculate the structural table type, retaining `any` columns for an empty table.
    pub fn get_type(&self) -> Type {
        let columns =
            self.columns
                .iter()
                .map(|column| {
                    let ty =
                        Type::supertype_of(self.rows.iter().filter_map(|row| {
                            row.as_record().ok()?.get(column).map(Value::get_type)
                        }))
                        .unwrap_or(Type::Any);
                    (column.clone(), ty)
                })
                .collect();
        Type::Table(columns)
    }
}

impl PartialEq for Table {
    fn eq(&self, other: &Self) -> bool {
        self.columns == other.columns
            && self.rows.len() == other.rows.len()
            && self
                .rows
                .iter()
                .zip(&other.rows)
                .all(|(left, right)| left.strict_eq(right))
    }
}

impl IntoIterator for Table {
    type Item = Value;
    type IntoIter = vec::IntoIter<Value>;

    fn into_iter(self) -> Self::IntoIter {
        self.rows.into_iter()
    }
}

impl<'a> IntoIterator for &'a Table {
    type Item = &'a Value;
    type IntoIter = slice::Iter<'a, Value>;

    fn into_iter(self) -> Self::IntoIter {
        self.rows.iter()
    }
}

fn validate_columns(columns: &[String], span: Span) -> Result<(), ShellError> {
    let mut seen = HashSet::with_capacity(columns.len());
    for column in columns {
        if !seen.insert(column) {
            return Err(ShellError::ColumnDefinedTwice {
                col_name: column.clone(),
                second_use: span,
                first_use: span,
                help: None,
            });
        }
    }
    Ok(())
}

fn normalize_row(row: Value, columns: &[String], table_span: Span) -> Result<Value, ShellError> {
    let row_span = row.span();
    let mut record = row.into_record()?;
    if let Some(extra) = record
        .columns()
        .find(|name| !columns.contains(name))
        .cloned()
    {
        return Err(ShellError::Generic(GenericError::new(
            "Table row contains an undeclared column",
            format!("column '{extra}' is not part of the table schema"),
            row_span,
        )));
    }

    let mut normalized = Record::with_capacity(record.len());
    for column in columns {
        if let Some(value) = record.remove(column) {
            normalized.push(column.clone(), value);
        }
    }
    Ok(Value::record(
        normalized,
        if row_span == Span::unknown() {
            table_span
        } else {
            row_span
        },
    ))
}

impl Hash for Table {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.columns.hash(state);
        self.rows.hash(state);
    }
}

#[derive(Serialize, Deserialize)]
struct SerializedTable {
    columns: Vec<String>,
    rows: Vec<Value>,
}

impl Serialize for Table {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        SerializedTable {
            columns: self.columns.clone(),
            rows: self.rows.clone(),
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Table {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let table = SerializedTable::deserialize(deserializer)?;
        Self::new(table.columns, table.rows, Span::unknown()).map_err(D::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::record;

    #[test]
    fn inferred_columns_are_stable_and_rows_are_normalized() {
        let table = Table::from_rows(
            vec![
                Value::test_record(record! {
                    "b" => Value::test_int(2),
                    "a" => Value::test_int(1),
                }),
                Value::test_record(record! {
                    "c" => Value::test_int(3),
                    "a" => Value::test_int(4),
                }),
            ],
            Span::test_data(),
        )
        .expect("valid table");

        assert_eq!(table.columns(), &["b", "a", "c"]);
        assert_eq!(
            table.rows()[1]
                .as_record()
                .expect("record row")
                .columns()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            vec!["a", "c"]
        );
    }

    #[test]
    fn explicit_schema_allows_missing_but_rejects_extra_columns() {
        let span = Span::test_data();
        let missing = Table::new(
            vec!["a".into(), "b".into()],
            vec![Value::test_record(record! { "a" => Value::test_int(1) })],
            span,
        );
        assert!(missing.is_ok());

        let extra = Table::new(
            vec!["a".into()],
            vec![Value::test_record(record! { "b" => Value::test_int(1) })],
            span,
        );
        assert!(extra.is_err());
    }

    #[test]
    fn constructors_reject_duplicate_columns_and_non_record_rows() {
        let span = Span::test_data();
        assert!(Table::new(vec!["a".into(), "a".into()], vec![], span).is_err());
        assert!(Table::from_rows(vec![Value::test_int(1)], span).is_err());
    }

    #[test]
    fn empty_table_retains_schema_and_type() {
        let table =
            Table::new(vec!["a".into()], vec![], Span::test_data()).expect("valid empty table");
        assert_eq!(table.columns(), &["a"]);
        assert_eq!(
            table.get_type(),
            Type::Table(vec![("a".into(), Type::Any)].into())
        );
    }

    #[test]
    fn recursive_mutation_cannot_replace_row_wrappers() {
        let span = Span::test_data();
        let mut value = Value::table_with_columns(
            vec!["a".into()],
            vec![Value::test_record(record! { "a" => Value::test_int(1) })],
            span,
        )
        .expect("valid table");

        value
            .recurse_mut(&mut |value| {
                if matches!(value, Value::Record { .. }) {
                    *value = Value::test_int(99);
                }
                Ok::<_, std::convert::Infallible>(())
            })
            .expect("infallible traversal");

        let table = value.as_table().expect("table preserved");
        assert!(matches!(table.rows(), [Value::Record { .. }]));
    }
}
