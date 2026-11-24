//! Schema-bearing streaming of record rows between pipeline elements.

use crate::{PipelineData, ShellError, Signals, Span, Table, Type, Value, ValueIterator};

/// A potentially infinite, interruptible stream of rows with a fixed ordered schema.
///
/// Like [`crate::ListStream`], observing rows drains the stream. Unlike a list stream, the
/// columns are available before the first row is consumed.
#[derive(derive_more::Debug)]
#[debug("TableStream")]
pub struct TableStream {
    columns: Vec<String>,
    stream: ValueIterator,
    span: Span,
    caller_spans: Vec<Span>,
}

impl TableStream {
    /// Create a table stream and validate its ordered schema.
    ///
    /// Rows are validated lazily. Record rows may omit declared columns, but undeclared columns
    /// and non-record rows are converted to error values.
    pub fn new(
        columns: Vec<String>,
        iter: impl Iterator<Item = Value> + Send + 'static,
        span: Span,
        signals: Signals,
    ) -> Result<Self, ShellError> {
        // Reuse the materialized table's schema validation.
        let _ = Table::new(columns.clone(), Vec::new(), span)?;
        let validation_columns = columns.clone();
        let iter = iter.map(move |row| match row {
            Value::Error { .. } => row,
            row => {
                let row_span = row.span();
                match Table::normalize_row(row, &validation_columns, span) {
                    Ok(row) => row,
                    Err(err) => Value::error(err, row_span),
                }
            }
        });

        Ok(Self {
            columns,
            stream: Box::new(InterruptIter::new(iter, signals)),
            span,
            caller_spans: Vec::new(),
        })
    }

    /// Create a stream from an already validated materialized table.
    pub fn from_table(table: Table, span: Span, signals: Signals) -> Self {
        let columns = table.columns().to_vec();
        // `Table` guarantees that these rows conform to its columns.
        Self {
            columns,
            stream: Box::new(InterruptIter::new(table.into_iter(), signals)),
            span,
            caller_spans: Vec::new(),
        }
    }

    /// Ordered table columns.
    pub fn columns(&self) -> &[String] {
        &self.columns
    }

    /// A structural table type whose column values are not known without consuming rows.
    pub fn get_type(&self) -> Type {
        Type::Table(
            self.columns
                .iter()
                .map(|column| (column.clone(), Type::Any))
                .collect(),
        )
    }

    pub fn span(&self) -> Span {
        self.span
    }

    pub fn with_span(mut self, span: Span) -> Self {
        self.span = span;
        self
    }

    pub fn push_caller_span(&mut self, span: Span) {
        if span != self.span {
            self.caller_spans.push(span);
        }
    }

    pub fn get_caller_spans(&self) -> &Vec<Span> {
        &self.caller_spans
    }

    pub fn into_inner(self) -> ValueIterator {
        self.stream
    }

    pub fn next_value(&mut self) -> Option<Value> {
        self.stream.next()
    }

    /// Collect all rows into a materialized table, promoting error values to [`ShellError`].
    pub fn into_value(self) -> Result<Value, ShellError> {
        let rows = self
            .stream
            .map(Value::unwrap_error)
            .collect::<Result<Vec<_>, _>>()?;
        Value::table_with_columns(self.columns, rows, self.span)
    }

    pub fn drain(self) -> Result<(), ShellError> {
        for row in self {
            if let Value::Error { error, .. } = row {
                return Err(*error);
            }
        }
        Ok(())
    }

    /// Modify the row iterator while retaining and revalidating the schema.
    pub fn modify<I>(self, f: impl FnOnce(ValueIterator) -> I) -> Result<Self, ShellError>
    where
        I: Iterator<Item = Value> + Send + 'static,
    {
        let caller_spans = self.caller_spans;
        let mut stream = Self::new(self.columns, f(self.stream), self.span, Signals::empty())?;
        stream.caller_spans = caller_spans;
        Ok(stream)
    }

    /// Modify the row iterator and replace the ordered schema.
    ///
    /// This is intended for lazy row transforms such as renaming or adding a column. Existing
    /// rows are validated against the old schema before `f` runs, and transformed rows are then
    /// validated against `columns`.
    pub fn modify_with_columns<I>(
        self,
        columns: Vec<String>,
        f: impl FnOnce(ValueIterator) -> I,
    ) -> Result<Self, ShellError>
    where
        I: Iterator<Item = Value> + Send + 'static,
    {
        let caller_spans = self.caller_spans;
        let mut stream = Self::new(columns, f(self.stream), self.span, Signals::empty())?;
        stream.caller_spans = caller_spans;
        Ok(stream)
    }
}

impl IntoIterator for TableStream {
    type Item = Value;
    type IntoIter = IntoIter;

    fn into_iter(self) -> Self::IntoIter {
        IntoIter {
            stream: self.into_inner(),
        }
    }
}

impl From<TableStream> for PipelineData {
    fn from(stream: TableStream) -> Self {
        Self::table_stream(stream, None)
    }
}

pub struct IntoIter {
    stream: ValueIterator,
}

impl Iterator for IntoIter {
    type Item = Value;

    fn next(&mut self) -> Option<Self::Item> {
        self.stream.next()
    }
}

struct InterruptIter<I: Iterator> {
    iter: I,
    signals: Signals,
}

impl<I: Iterator> InterruptIter<I> {
    fn new(iter: I, signals: Signals) -> Self {
        Self { iter, signals }
    }
}

impl<I: Iterator> Iterator for InterruptIter<I> {
    type Item = I::Item;

    fn next(&mut self) -> Option<Self::Item> {
        if self.signals.interrupted() {
            None
        } else {
            self.iter.next()
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.iter.size_hint()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::record;

    #[test]
    fn exposes_schema_and_collects_to_table() {
        let span = Span::test_data();
        let stream = TableStream::new(
            vec!["a".into(), "b".into()],
            vec![Value::test_record(record! { "a" => Value::test_int(1) })].into_iter(),
            span,
            Signals::empty(),
        )
        .expect("valid table stream");

        assert_eq!(stream.columns(), &["a", "b"]);
        let value = stream.into_value().expect("collect table stream");
        let table = value.as_table().expect("table value");
        assert_eq!(table.columns(), &["a", "b"]);
        assert_eq!(table.len(), 1);
    }

    #[test]
    fn invalid_rows_become_errors() {
        let span = Span::test_data();
        let stream = TableStream::new(
            vec!["a".into()],
            vec![Value::test_record(record! { "b" => Value::test_int(1) })].into_iter(),
            span,
            Signals::empty(),
        )
        .expect("valid schema");

        assert!(stream.into_value().is_err());
    }
}
