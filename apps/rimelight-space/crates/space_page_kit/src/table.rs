//! A list's table: the plain rows and columns of the table component. The window glue (`table_glue!`) turns them into the
//! generated `TableColumn`, `TableRow` and id model, in the app crate where those types are in scope.

/// Expands, in the app crate, `Table` (the columns, the rows and the id of each row) and `table`, which builds a `Table`
/// from the columns (title and width) and the rows (id, selected, cells). The row ids are in the same order as the rows.
#[macro_export]
macro_rules! table_glue {
    ($TableColumn:ident, $TableRow:ident) => {
        /// A list's table: its columns, its rows, and the id of each row (in the same order as the rows).
        pub struct Table {
            pub columns: $crate::__slint::ModelRc<$TableColumn>,
            pub rows: $crate::__slint::ModelRc<$TableRow>,
            pub ids: $crate::__slint::ModelRc<$crate::__slint::SharedString>,
        }

        /// Builds a [`Table`] from the columns (title and width) and the rows (id, selected, cells).
        pub fn table(columns: &[(&str, f32)], rows: Vec<(String, bool, Vec<String>)>) -> Table {
            let columns: Vec<$TableColumn> = columns
                .iter()
                .map(|(title, width)| $TableColumn {
                    title: (*title).into(),
                    width: *width,
                    sortable: false,
                })
                .collect();
            let mut ids = Vec::with_capacity(rows.len());
            let mut table_rows = Vec::with_capacity(rows.len());
            for (id, selected, cells) in rows {
                ids.push($crate::__slint::SharedString::from(id));
                table_rows.push($TableRow {
                    cells: $crate::__slint::ModelRc::new($crate::__slint::VecModel::from(
                        cells
                            .into_iter()
                            .map($crate::__slint::SharedString::from)
                            .collect::<Vec<_>>(),
                    )),
                    selected,
                });
            }
            Table {
                columns: $crate::__slint::ModelRc::new($crate::__slint::VecModel::from(columns)),
                rows: $crate::__slint::ModelRc::new($crate::__slint::VecModel::from(table_rows)),
                ids: $crate::__slint::ModelRc::new($crate::__slint::VecModel::from(ids)),
            }
        }
    };
}
