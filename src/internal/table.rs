use internal::column::Column;
use internal::streamname;
use internal::stringpool::StringPool;
use internal::value::{Value, ValueRef};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::ops::Index;

// ========================================================================= //

/// A database table.
pub struct Table {
    name: String,
    columns: Vec<Column>,
    long_string_refs: bool,
}

impl Table {
    /// Creates a new table object with the given name and columns.  The
    /// `long_string_refs` argument indicates the size of any encoded string
    /// refs.
    pub fn new(name: String, columns: Vec<Column>, long_string_refs: bool)
               -> Table {
        Table {
            name: name,
            columns: columns,
            long_string_refs: long_string_refs,
        }
    }

    /// Returns the name of the table.
    pub fn name(&self) -> &str { &self.name }

    /// Returns the name of the CFB stream that holds this table's data.
    pub(crate) fn stream_name(&self) -> String {
        streamname::encode(&self.name, true)
    }

    /// Returns the list of columns in this table.
    pub fn columns(&self) -> &[Column] { &self.columns }

    /// Returns the indices of table's primary key columns.
    pub fn primary_key_indices(&self) -> Vec<usize> {
        self.columns
            .iter()
            .enumerate()
            .filter_map(|(index, column)| if column.is_primary_key() {
                            Some(index)
                        } else {
                            None
                        })
            .collect()
    }

    fn index_for_column_name(&self, column_name: &str) -> usize {
        for (index, column) in self.columns.iter().enumerate() {
            if column.name() == column_name {
                return index;
            }
        }
        panic!("Table {:?} has no column named {:?}",
               self.name,
               column_name);
    }

    /// Parses row data from the given data source and returns an interator
    /// over the rows.
    pub(crate) fn read_rows<R: Read + Seek>(
        &self, mut reader: R)
        -> io::Result<Vec<Vec<ValueRef>>> {
        let data_length = reader.seek(SeekFrom::End(0))?;
        reader.seek(SeekFrom::Start(0))?;
        let row_size = self.columns
            .iter()
            .map(|col| col.coltype().width(self.long_string_refs))
            .sum::<u64>();
        let num_columns = self.columns.len();
        let num_rows = if row_size > 0 {
            (data_length / row_size) as usize
        } else {
            0
        };
        let mut rows =
            vec![Vec::<ValueRef>::with_capacity(num_columns); num_rows];
        for column in self.columns.iter() {
            let coltype = column.coltype();
            for row in rows.iter_mut() {
                row.push(coltype
                             .read_value(&mut reader, self.long_string_refs)?);
            }
        }
        Ok(rows)
    }

    pub(crate) fn write_rows<W: Write>(&self, mut writer: W,
                                       rows: Vec<Vec<ValueRef>>)
                                       -> io::Result<()> {
        for (index, column) in self.columns.iter().enumerate() {
            let coltype = column.coltype();
            for row in rows.iter() {
                coltype
                    .write_value(&mut writer,
                                 row[index],
                                 self.long_string_refs)?;
            }
        }
        Ok(())
    }
}

// ========================================================================= //

/// One row from a database table.
pub struct Row<'a> {
    table: &'a Table,
    values: Vec<Value>,
}

impl<'a> Row<'a> {
    pub(crate) fn new(table: &'a Table, values: Vec<Value>) -> Row<'a> {
        Row {
            table: table,
            values: values,
        }
    }

    /// Returns the number of columns in the row.
    pub fn len(&self) -> usize { self.table.columns().len() }
}

impl<'a> Index<usize> for Row<'a> {
    type Output = Value;

    fn index(&self, index: usize) -> &Value { &self.values[index] }
}

impl<'a, 'b> Index<&'b str> for Row<'a> {
    type Output = Value;

    fn index(&self, column_name: &str) -> &Value {
        let index = self.table.index_for_column_name(column_name);
        &self.values[index]
    }
}

// ========================================================================= //

/// An iterator over the rows in a database table.
pub struct Rows<'a> {
    string_pool: &'a StringPool,
    table: &'a Table,
    rows: Vec<Vec<ValueRef>>,
    next_row_index: usize,
}

impl<'a> Rows<'a> {
    pub(crate) fn new(string_pool: &'a StringPool, table: &'a Table,
                      rows: Vec<Vec<ValueRef>>)
                      -> Rows<'a> {
        Rows {
            table: table,
            string_pool: string_pool,
            rows: rows,
            next_row_index: 0,
        }
    }
}

impl<'a> Iterator for Rows<'a> {
    type Item = Row<'a>;

    fn next(&mut self) -> Option<Row<'a>> {
        if self.next_row_index < self.rows.len() {
            let values: Vec<Value> = self.rows[self.next_row_index]
                .iter()
                .map(|value_ref| value_ref.to_value(self.string_pool))
                .collect();
            self.next_row_index += 1;
            Some(Row::new(self.table, values))
        } else {
            None
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        debug_assert!(self.next_row_index <= self.rows.len());
        let remaining_rows = self.rows.len() - self.next_row_index;
        (remaining_rows, Some(remaining_rows))
    }
}

impl<'a> ExactSizeIterator for Rows<'a> {}

// ========================================================================= //
