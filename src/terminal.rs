use alacritty_terminal::event::VoidListener;
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::term::Config;
use alacritty_terminal::term::test::TermSize;
use alacritty_terminal::vte::ansi;
use alacritty_terminal::Term;

pub struct TerminalBuffer {
    term: Term<VoidListener>,
    parser: ansi::Processor,
}

impl TerminalBuffer {
    pub fn new(rows: usize, cols: usize) -> Self {
        let size = TermSize::new(cols, rows);
        let term = Term::new(Config::default(), &size, VoidListener);
        Self {
            term,
            parser: ansi::Processor::new(),
        }
    }

    pub fn feed(&mut self, data: &[u8]) {
        self.parser.advance(&mut self.term, data);
    }

    pub fn rows(&self) -> usize {
        self.term.screen_lines()
    }

    pub fn resize(&mut self, rows: usize, cols: usize) {
        let size = TermSize::new(cols, rows);
        self.term.resize(size);
    }

    pub fn screen_text(&self) -> String {
        let grid = self.term.grid();
        let mut lines = Vec::new();
        for row in 0..grid.screen_lines() {
            let row_idx = alacritty_terminal::index::Line(row as i32);
            let mut line = String::new();
            for col in 0..grid.columns() {
                let cell = &grid[row_idx][alacritty_terminal::index::Column(col)];
                line.push(cell.c);
            }
            lines.push(line.trim_end().to_string());
        }
        lines.join("\n")
    }
}
