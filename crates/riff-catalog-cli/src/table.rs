//! Minimal column-aligned table writer.

pub struct Table {
    headers: Vec<String>,
    rows: Vec<Vec<String>>,
}

impl Table {
    pub fn new(headers: &[&str]) -> Self {
        Self {
            headers: headers.iter().map(|h| h.to_string()).collect(),
            rows: Vec::new(),
        }
    }

    pub fn row(&mut self, cells: Vec<String>) {
        self.rows.push(cells);
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    pub fn print(&self) {
        let columns = self.headers.len();
        let mut widths: Vec<usize> = self.headers.iter().map(String::len).collect();
        for row in &self.rows {
            for (index, cell) in row.iter().enumerate().take(columns) {
                widths[index] = widths[index].max(cell.len());
            }
        }
        let print_row = |cells: &[String]| {
            let line = cells
                .iter()
                .enumerate()
                .map(|(index, cell)| {
                    format!(
                        "{cell:<width$}",
                        width = widths.get(index).copied().unwrap_or(0)
                    )
                })
                .collect::<Vec<_>>()
                .join("  ");
            println!("{}", line.trim_end());
        };
        print_row(&self.headers);
        println!(
            "{}",
            widths
                .iter()
                .map(|width| "-".repeat(*width))
                .collect::<Vec<_>>()
                .join("  ")
        );
        for row in &self.rows {
            print_row(row);
        }
    }

    pub fn to_json(&self) -> serde_json::Value {
        let rows: Vec<serde_json::Value> = self
            .rows
            .iter()
            .map(|row| {
                self.headers
                    .iter()
                    .zip(row)
                    .map(|(header, cell)| (header.clone(), serde_json::Value::String(cell.clone())))
                    .collect::<serde_json::Map<_, _>>()
                    .into()
            })
            .collect();
        serde_json::Value::Array(rows)
    }
}
