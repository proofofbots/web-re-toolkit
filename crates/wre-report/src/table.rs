#[derive(Debug, Clone, Default)]
pub struct Table {
    headers: Vec<String>,
    rows: Vec<Vec<String>>,
}

impl Table {
    pub fn new(headers: &[&str]) -> Self {
        Self {
            headers: headers.iter().map(|value| (*value).to_string()).collect(),
            rows: Vec::new(),
        }
    }

    pub fn row(&mut self, cells: &[String]) -> &mut Self {
        let mut row: Vec<String> = cells.iter().map(|cell| escape(cell)).collect();
        row.resize(self.headers.len(), String::new());
        self.rows.push(row);
        self
    }

    pub fn push(&mut self, cells: Vec<String>) -> &mut Self {
        self.row(&cells)
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    pub fn sort_by_column(&mut self, index: usize) -> &mut Self {
        self.rows.sort_by(|left, right| {
            let a = left.get(index).map(String::as_str).unwrap_or_default();
            let b = right.get(index).map(String::as_str).unwrap_or_default();
            a.cmp(b)
        });
        self
    }

    pub fn render(&self) -> String {
        if self.headers.is_empty() {
            return String::new();
        }

        let mut out = String::new();

        out.push_str("| ");
        out.push_str(&self.headers.join(" | "));
        out.push_str(" |\n");

        out.push('|');
        for _ in &self.headers {
            out.push_str(" --- |");
        }
        out.push('\n');

        for row in &self.rows {
            out.push_str("| ");
            out.push_str(&row.join(" | "));
            out.push_str(" |\n");
        }

        out
    }
}

fn escape(cell: &str) -> String {
    cell.replace('|', "\\|").replace('\n', " ")
}

pub fn heading(level: usize, text: &str) -> String {
    format!("{} {text}\n\n", "#".repeat(level.clamp(1, 6)))
}

pub fn code_block(language: &str, body: &str) -> String {
    format!("```{language}\n{}\n```\n\n", body.trim_end())
}

pub fn list(items: &[String]) -> String {
    let mut out = String::new();
    for item in items {
        out.push_str(&format!("- {item}\n"));
    }
    out.push('\n');
    out
}

pub fn quote(text: &str) -> String {
    let mut out = String::new();
    for line in text.lines() {
        out.push_str(&format!("> {line}\n"));
    }
    out.push('\n');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_a_markdown_table() {
        let mut table = Table::new(&["signal", "source", "note"]);
        table.push(vec!["s17".into(), "canvas".into(), "rendering".into()]);
        table.push(vec!["s2".into(), "screen".into(), "geometry".into()]);

        let rendered = table.render();
        assert!(rendered.starts_with("| signal | source | note |\n| --- | --- | --- |\n"));
        assert!(rendered.contains("| s17 | canvas | rendering |"));
    }

    #[test]
    fn escapes_pipes_and_newlines() {
        let mut table = Table::new(&["a"]);
        table.push(vec!["x | y\nz".into()]);
        assert!(table.render().contains("x \\| y z"));
    }

    #[test]
    fn pads_short_rows() {
        let mut table = Table::new(&["a", "b", "c"]);
        table.push(vec!["one".into()]);
        assert!(table.render().contains("| one |  |  |"));
    }

    #[test]
    fn sorts_by_a_column() {
        let mut table = Table::new(&["name"]);
        table.push(vec!["b".into()]);
        table.push(vec!["a".into()]);
        table.sort_by_column(0);
        assert!(table.render().find("| a |").unwrap() < table.render().find("| b |").unwrap());
    }
}
