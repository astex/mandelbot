//! Markdown parser for the plan-review overlay.
//!
//! Walks the `pulldown-cmark` event stream and produces a flat list of
//! [`RenderedLine`]s. Each line is a sequence of styled spans plus a
//! [`BlockKind`] describing the surrounding block. The widget consumes this
//! list directly — there is no intermediate AST.

use pulldown_cmark::{Event, HeadingLevel, Options, Parser, Tag, TagEnd};

/// Inline span style flags.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SpanStyle {
    pub bold: bool,
    pub italic: bool,
    pub code: bool,
    pub link: bool,
}

/// One styled run of text inside a line.
#[derive(Clone, Debug)]
pub struct Span {
    pub text: String,
    pub style: SpanStyle,
}

/// What kind of block a line belongs to. Drives sizing, indentation, and
/// any block-level decoration (bg, gutter bar, etc.).
#[derive(Clone, Debug)]
pub enum BlockKind {
    Paragraph,
    Heading(u8),
    /// Fenced code block. `lang` is the info string after the opening fence,
    /// or `None` for an unlabeled fence.
    CodeBlock { lang: Option<String> },
    BlockQuote,
    HorizontalRule,
}

/// One rendered line ready for the widget to lay out and paint.
#[derive(Clone, Debug)]
pub struct RenderedLine {
    pub block: BlockKind,
    /// Indentation level (in list nesting steps). Code blocks and quotes
    /// inherit from their containing list.
    pub indent: usize,
    /// Per-line marker shown in the gutter (e.g. "• " or "1. "). Empty
    /// for blocks that don't need one.
    pub marker: String,
    pub spans: Vec<Span>,
}

impl RenderedLine {
    fn empty(block: BlockKind, indent: usize) -> Self {
        Self { block, indent, marker: String::new(), spans: Vec::new() }
    }

}

/// Parse `source` markdown into a flat list of rendered lines.
pub fn parse(source: &str) -> Vec<RenderedLine> {
    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_STRIKETHROUGH);
    opts.insert(Options::ENABLE_TABLES);
    let parser = Parser::new_ext(source, opts);

    let mut walker = Walker::new();
    for event in parser {
        walker.handle(event);
    }
    walker.finish();
    walker.lines
}

#[derive(Default)]
struct Walker {
    lines: Vec<RenderedLine>,
    /// Currently-open block stack. The innermost block decides the
    /// `BlockKind` of newly-flushed text.
    blocks: Vec<BlockState>,
    /// Current style applied to inline text.
    style: SpanStyle,
    /// Buffer of spans for the line currently being built.
    current: Vec<Span>,
    /// Indent level (counts open lists).
    indent: usize,
    /// List-item marker queued for the next flushed line.
    pending_marker: Option<String>,
}

#[derive(Debug)]
enum BlockState {
    Paragraph,
    Heading(u8),
    BlockQuote,
    List { ordered: Option<u64> },
    Item, // marker for the current list item
    CodeBlock { lang: Option<String> },
}

impl Walker {
    fn new() -> Self {
        Self::default()
    }

    fn current_block_kind(&self) -> BlockKind {
        // Decorated containers (heading, code, quote) take precedence over a
        // wrapping paragraph so a `> para` line is reported as BlockQuote.
        for state in self.blocks.iter().rev() {
            match state {
                BlockState::Heading(level) => return BlockKind::Heading(*level),
                BlockState::CodeBlock { lang } => {
                    return BlockKind::CodeBlock { lang: lang.clone() };
                }
                BlockState::BlockQuote => return BlockKind::BlockQuote,
                _ => continue,
            }
        }
        BlockKind::Paragraph
    }

    /// Push the current span buffer as one finished line.
    fn flush_line(&mut self, marker: String) {
        let block = self.current_block_kind();
        let spans = std::mem::take(&mut self.current);
        self.lines.push(RenderedLine {
            block,
            indent: self.indent,
            marker,
            spans,
        });
    }

    fn push_text(&mut self, text: &str) {
        // Split on newlines so each rendered line is a single visual row.
        let mut iter = text.split('\n').peekable();
        while let Some(chunk) = iter.next() {
            if !chunk.is_empty() {
                self.current.push(Span { text: chunk.to_string(), style: self.style });
            }
            if iter.peek().is_some() {
                self.flush_line(String::new());
            }
        }
    }

    fn handle(&mut self, event: Event<'_>) {
        match event {
            Event::Start(tag) => self.start(tag),
            Event::End(tag) => self.end(tag),
            Event::Text(text) => self.push_text(&text),
            Event::Code(text) => {
                let prev = self.style;
                self.style.code = true;
                self.current.push(Span { text: text.into_string(), style: self.style });
                self.style = prev;
            }
            Event::Html(html) | Event::InlineHtml(html) => {
                self.push_text(&html);
            }
            Event::SoftBreak => {
                self.current.push(Span { text: " ".into(), style: self.style });
            }
            Event::HardBreak => {
                self.flush_line(String::new());
            }
            Event::Rule => {
                self.lines.push(RenderedLine::empty(BlockKind::HorizontalRule, self.indent));
            }
            Event::FootnoteReference(_)
            | Event::TaskListMarker(_)
            | Event::InlineMath(_)
            | Event::DisplayMath(_) => {}
        }
    }

    fn start(&mut self, tag: Tag<'_>) {
        match tag {
            Tag::Paragraph => self.blocks.push(BlockState::Paragraph),
            Tag::Heading { level, .. } => {
                let lvl = match level {
                    HeadingLevel::H1 => 1,
                    HeadingLevel::H2 => 2,
                    HeadingLevel::H3 => 3,
                    HeadingLevel::H4 => 4,
                    HeadingLevel::H5 => 5,
                    HeadingLevel::H6 => 6,
                };
                self.blocks.push(BlockState::Heading(lvl));
            }
            Tag::BlockQuote(_) => self.blocks.push(BlockState::BlockQuote),
            Tag::CodeBlock(kind) => {
                let lang = match kind {
                    pulldown_cmark::CodeBlockKind::Fenced(s) => {
                        let s = s.into_string();
                        if s.is_empty() { None } else { Some(s) }
                    }
                    pulldown_cmark::CodeBlockKind::Indented => None,
                };
                self.blocks.push(BlockState::CodeBlock { lang });
            }
            Tag::List(start) => {
                self.blocks.push(BlockState::List { ordered: start });
                self.indent += 1;
            }
            Tag::Item => {
                self.blocks.push(BlockState::Item);
                // Determine the marker from the enclosing list.
                let marker = match self.parent_list_mut() {
                    Some(BlockState::List { ordered: Some(n) }) => {
                        let m = format!("{}. ", *n);
                        *n += 1;
                        m
                    }
                    _ => "• ".to_string(),
                };
                // Items always start a fresh paragraph implicitly; we let the
                // child Paragraph tag emit lines, but stash the marker for
                // the first line of this item.
                self.pending_marker = Some(marker);
            }
            Tag::Emphasis => self.style.italic = true,
            Tag::Strong => self.style.bold = true,
            Tag::Strikethrough => {} // not styled distinctly yet
            Tag::Link { .. } => self.style.link = true,
            Tag::Image { .. } => self.style.link = true,
            _ => {}
        }
    }

    fn end(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::Paragraph => {
                self.flush_with_marker();
                self.pop(BlockState::Paragraph);
                self.blank_line();
            }
            TagEnd::Heading(_) => {
                self.flush_with_marker();
                if let Some(BlockState::Heading(_)) = self.blocks.last() {
                    self.blocks.pop();
                }
                self.blank_line();
            }
            TagEnd::BlockQuote => {
                self.pop(BlockState::BlockQuote);
                self.blank_line();
            }
            TagEnd::CodeBlock => {
                // Trailing newline from the parser produced an empty line we
                // don't want; drop it if present and mark the last code line.
                if let Some(last) = self.lines.last() {
                    if matches!(last.block, BlockKind::CodeBlock { .. }) && last.spans.is_empty() {
                        self.lines.pop();
                    }
                }
                if let Some(BlockState::CodeBlock { .. }) = self.blocks.last() {
                    self.blocks.pop();
                }
                self.blank_line();
            }
            TagEnd::List(_) => {
                if let Some(BlockState::List { .. }) = self.blocks.last() {
                    self.blocks.pop();
                }
                self.indent = self.indent.saturating_sub(1);
                if self.indent == 0 {
                    self.blank_line();
                }
            }
            TagEnd::Item => {
                self.flush_with_marker();
                if let Some(BlockState::Item) = self.blocks.last() {
                    self.blocks.pop();
                }
            }
            TagEnd::Emphasis => self.style.italic = false,
            TagEnd::Strong => self.style.bold = false,
            TagEnd::Link | TagEnd::Image => self.style.link = false,
            _ => {}
        }
    }

    fn parent_list_mut(&mut self) -> Option<&mut BlockState> {
        self.blocks
            .iter_mut()
            .rev()
            .find(|b| matches!(b, BlockState::List { .. }))
    }

    fn pop(&mut self, expected: BlockState) {
        if let Some(top) = self.blocks.last() {
            if std::mem::discriminant(top) == std::mem::discriminant(&expected) {
                self.blocks.pop();
            }
        }
    }

    /// Push a blank line as a paragraph spacer if the previous line is not
    /// already blank. Used between blocks.
    fn blank_line(&mut self) {
        if matches!(self.lines.last(), Some(l) if l.spans.is_empty()
            && matches!(l.block, BlockKind::Paragraph)) {
            return;
        }
        self.lines
            .push(RenderedLine::empty(BlockKind::Paragraph, self.indent));
    }

    /// Flush the current line, attaching the pending list-item marker if any.
    fn flush_with_marker(&mut self) {
        if self.current.is_empty() && self.pending_marker.is_none() {
            return;
        }
        let marker = self.pending_marker.take().unwrap_or_default();
        self.flush_line(marker);
    }

    fn finish(&mut self) {
        if !self.current.is_empty() {
            self.flush_with_marker();
        }
        // Trim trailing blank lines.
        while matches!(self.lines.last(), Some(l) if l.spans.is_empty()
            && matches!(l.block, BlockKind::Paragraph)) {
            self.lines.pop();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_heading_and_paragraph() {
        let lines = parse("# Title\n\nHello **world**\n");
        assert!(matches!(lines[0].block, BlockKind::Heading(1)));
        assert_eq!(lines[0].spans[0].text, "Title");
        let para = lines.iter().find(|l| matches!(l.block, BlockKind::Paragraph) && !l.spans.is_empty()).unwrap();
        assert!(para.spans.iter().any(|s| s.text == "world" && s.style.bold));
    }

    #[test]
    fn parses_bullet_list() {
        let lines = parse("- one\n- two\n");
        let items: Vec<_> = lines.iter().filter(|l| !l.spans.is_empty()).collect();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].marker, "• ");
        assert_eq!(items[0].spans[0].text, "one");
    }

    #[test]
    fn parses_numbered_list() {
        let lines = parse("1. one\n2. two\n");
        let items: Vec<_> = lines.iter().filter(|l| !l.spans.is_empty()).collect();
        assert_eq!(items[0].marker, "1. ");
        assert_eq!(items[1].marker, "2. ");
    }

    #[test]
    fn parses_code_fence_with_lang() {
        let lines = parse("```rust\nfn main() {}\n```\n");
        let code: Vec<_> = lines
            .iter()
            .filter(|l| matches!(l.block, BlockKind::CodeBlock { .. }))
            .collect();
        assert!(!code.is_empty());
        let BlockKind::CodeBlock { lang, .. } = &code[0].block else { unreachable!() };
        assert_eq!(lang.as_deref(), Some("rust"));
    }

    #[test]
    fn parses_horizontal_rule() {
        let lines = parse("a\n\n---\n\nb\n");
        assert!(lines.iter().any(|l| matches!(l.block, BlockKind::HorizontalRule)));
    }

    #[test]
    fn parses_block_quote() {
        let lines = parse("> quoted\n");
        assert!(lines.iter().any(|l| matches!(l.block, BlockKind::BlockQuote)));
    }

    #[test]
    fn parses_inline_code() {
        let lines = parse("use `Vec::new`\n");
        let line = lines.iter().find(|l| !l.spans.is_empty()).unwrap();
        assert!(line.spans.iter().any(|s| s.text == "Vec::new" && s.style.code));
    }
}
