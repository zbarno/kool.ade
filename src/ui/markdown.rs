//! Shared Markdown painter: the tree's single pulldown-cmark 0.13 event fold,
//! theme-parameterized by [`Style`] (factored out of the spec viewer's
//! private light-paper fold — CHG-003).
//!
//! Surfaces:
//!   * `crate::ui::spec_viewer` — white paper via [`PAPER`]; the palette and
//!     sizes pin the viewer's incumbent literals byte-for-byte.
//!   * agent reply prose in `crate::ui::chat_pane` / `crate::ui::task_chat`
//!     — dark theme via [`CHAT`], headings sized down for chat density.
//!
//! Contract:
//!   * Every in-flight prefix of a streamed reply parses without panicking;
//!     partially opened emphasis/fences degrade to harmless literal glyphs
//!     until their closers land (the caller's projection withholds the JSON
//!     envelope and unterminated opening fences upstream).
//!   * User-entered text is NEVER routed here (chatlog.rs stance: message
//!     text is plain, "never Markdown-rendered from user input").

use pulldown_cmark::{Event, HeadingLevel, Options, Parser, Tag, TagEnd};

use crate::ui::theme;

/// Palette and sizing for one Markdown surface.
#[derive(Clone, Copy)]
pub struct Style {
    /// Base text; bold runs are also forced onto this ink.
    pub ink: egui::Color32,
    /// Quote prefixes, list markers and strike strokes.
    pub muted: egui::Color32,
    /// Fenced code-block language caption.
    pub code_tag: egui::Color32,
    /// Body, list and paragraph font size.
    pub body: f32,
    /// H1 / H2 / H3 font sizes.
    pub h1: f32,
    pub h2: f32,
    pub h3: f32,
    /// H4 and deeper font size.
    pub h_other: f32,
    /// Fenced code font size.
    pub code: f32,
    /// Table cell font size.
    pub table: f32,
}

/// The spec viewer's incumbent light-paper literals (RGB triples and sizes);
/// painting with `PAPER` reproduces the pre-factor viewer exactly.
pub const PAPER: Style = Style {
    ink: egui::Color32::from_rgb(31, 41, 55),
    muted: egui::Color32::from_rgb(85, 98, 116),
    code_tag: egui::Color32::from_rgb(23, 83, 151),
    body: 15.0,
    h1: 30.0,
    h2: 22.0,
    h3: 18.0,
    h_other: 13.5,
    code: 12.0,
    table: 13.5,
};

/// Dark-theme chat style: ink/muted/caption themed off `theme.rs`, headings
/// sized down for skimmability while the body keeps today's 15px density.
pub const CHAT: Style = Style {
    ink: theme::TEXT,
    muted: theme::TEXT_DIM,
    code_tag: theme::TEXT_DIM,
    body: 15.0,
    h1: 20.0,
    h2: 17.0,
    h3: 15.5,
    h_other: 15.0,
    code: 12.0,
    table: 13.0,
};

/// Parse `md` with the in-tree pulldown-cmark fold and paint it styled.
/// Emits zero-or-more shapes and never panics on partial or undecorated
/// input, so streamed prefixes can be repainted tick by tick.
pub fn paint(ui: &mut egui::Ui, md: &str, style: Style) {
    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_TABLES);
    opts.insert(Options::ENABLE_STRIKETHROUGH);
    let parser = Parser::new_ext(md, opts);

    let mut st = State::fresh(style);
    for ev in parser {
        st.step(ev, ui);
    }
    st.finish(ui);
}

struct Span {
    text: String,
    mono: bool,
    bold: bool,
    italic: bool,
    strike: bool,
    color: Option<egui::Color32>,
    size: f32,
}

impl Span {
    fn fmt(&self, style: Style) -> egui::text::TextFormat {
        let mut f = egui::text::TextFormat::default();
        let size = self.size;
        f.font_id = if self.mono {
            egui::FontId::monospace(size)
        } else {
            egui::FontId::proportional(size)
        };
        let mut color = self.color.unwrap_or(style.ink);
        if self.bold {
            color = style.ink;
            f.extra_letter_spacing = 0.01 * size;
        }
        f.color = color;
        f.line_height = Some(size * 1.5);
        f.italics = self.italic;
        f.strikethrough = if self.strike {
            egui::epaint::Stroke::new(1.0, style.muted)
        } else {
            egui::epaint::Stroke::NONE
        };
        f
    }
}

struct State {
    style: Style,
    strong: usize,
    emph: usize,
    strike: usize,
    runs: Vec<Span>, // current inline flow
    in_heading: Option<HeadingLevel>,
    code_buf: Option<String>,
    quote: usize,
    list: Vec<(bool, u64)>, // (ordered, next_number)
    // table machinery
    in_table: bool,
    in_head: bool,
    cells: Vec<String>,
    cur_row: Vec<String>,
    ncols: usize,
}

impl State {
    fn fresh(style: Style) -> Self {
        State {
            style,
            strong: 0,
            emph: 0,
            strike: 0,
            runs: Vec::new(),
            in_heading: None,
            code_buf: None,
            quote: 0,
            list: Vec::new(),
            in_table: false,
            in_head: false,
            cells: Vec::new(),
            cur_row: Vec::new(),
            ncols: 0,
        }
    }

    fn base_span(&self) -> Span {
        Span {
            text: String::new(),
            mono: false,
            bold: self.strong > 0,
            italic: self.emph > 0,
            strike: self.strike > 0,
            color: None,
            size: self.style.body,
        }
    }

    fn push_text(&mut self, txt: &str, mono: bool) {
        let mut s = self.base_span();
        s.mono |= mono;
        s.text = txt.to_string();
        self.runs.push(s);
    }

    /// Emit buffered inline runs as one wrapped label.
    fn flush_runs(&mut self, ui: &mut egui::Ui, size_override: Option<f32>, gap: f32) {
        if self.runs.is_empty() {
            return;
        }
        let size = size_override.unwrap_or(self.style.body);
        let mut job = egui::text::LayoutJob::default();
        for r in self.runs.drain(..) {
            let mut fmt = r.fmt(self.style);
            if size_override.is_some() && fmt.font_id.size != size {
                fmt.font_id = egui::FontId::new(size, fmt.font_id.family);
                fmt.line_height = Some(size * 1.4);
            }
            job.append(&r.text, 0.0, fmt);
        }
        job.wrap.max_width = ui.available_width();
        let galley = ui.painter().layout_job(job);
        ui.add(egui::Label::new(galley));
        if gap > 0.0 {
            ui.add_space(gap);
        }
    }

    fn step(&mut self, ev: Event, ui: &mut egui::Ui) {
        match ev {
            Event::Start(tag) => match tag {
                Tag::Heading { level, .. } => {
                    self.runs.clear();
                    self.in_heading = Some(level);
                }
                Tag::Paragraph => {}
                Tag::BlockQuote(_) => {
                    self.quote += 1;
                    self.runs.push(prefix("> ", self.style));
                }
                Tag::CodeBlock(kind) => {
                    let lang = match kind {
                        pulldown_cmark::CodeBlockKind::Indented => String::new(),
                        pulldown_cmark::CodeBlockKind::Fenced(l) => l.to_string(),
                    };
                    self.begin_code_block(ui, &lang);
                }
                Tag::Strong => self.strong += 1,
                Tag::Emphasis => self.emph += 1,
                Tag::Strikethrough => self.strike += 1,
                Tag::Item => {
                    self.flush_runs(ui, None, 4.0);
                    self.push_list_marker(ui);
                }
                Tag::List(list_info) => {
                    let ordered = list_info.is_some();
                    let start = list_info.map_or(1, |n| if n == 0 { 1 } else { n });
                    self.list.push((ordered, start));
                }
                Tag::Table(cols) => {
                    self.in_table = true;
                    self.ncols = cols.len();
                }
                Tag::TableHead => self.in_head = true,
                Tag::TableRow => {
                    self.cur_row = Vec::new();
                }
                Tag::TableCell => {
                    self.cells.push(String::new());
                }
                _ => {}
            },
            Event::End(tag) => match tag {
                TagEnd::Heading(level) => {
                    ui.add_space(match level {
                        HeadingLevel::H1 => 16.0,
                        HeadingLevel::H2 => 12.0,
                        _ => 8.0,
                    });
                    let size = match level {
                        HeadingLevel::H1 => self.style.h1,
                        HeadingLevel::H2 => self.style.h2,
                        HeadingLevel::H3 => self.style.h3,
                        _ => self.style.h_other,
                    };
                    self.in_heading = None;
                    let runs = std::mem::take(&mut self.runs);
                    self.render_runs_at(ui, runs, size, 6.0);
                }
                TagEnd::Paragraph => {
                    self.flush_runs(ui, None, 6.0);
                }
                TagEnd::BlockQuote(_) => {
                    self.quote = self.quote.saturating_sub(1);
                }
                TagEnd::CodeBlock => {
                    if let Some(buf) = self.code_buf.take() {
                        self.end_code_block(ui, buf);
                    }
                }
                TagEnd::Strong => self.strong -= 1,
                TagEnd::Emphasis => self.emph -= 1,
                TagEnd::Strikethrough => self.strike -= 1,
                TagEnd::Item => {
                    self.flush_runs(ui, None, 4.0);
                    if let Some(l) = self.list.last_mut() {
                        l.1 += 1;
                    }
                }
                TagEnd::List(_) => {
                    self.list.pop();
                }
                TagEnd::TableCell => {
                    if let Some(c) = self.cells.pop() {
                        self.cur_row.push(c);
                    }
                }
                TagEnd::TableRow => {
                    let row = std::mem::take(&mut self.cur_row);
                    self.draw_table_row(ui, row, false);
                    self.in_head = false;
                }
                TagEnd::TableHead => {
                    let row = std::mem::take(&mut self.cur_row);
                    self.draw_table_row(ui, row, true);
                    ui.add_space(2.0);
                    ui.separator();
                    ui.add_space(2.0);
                }
                TagEnd::Table => {
                    self.in_table = false;
                    self.ncols = 0;
                    ui.add_space(6.0);
                }
                _ => {}
            },
            Event::Text(t) => {
                if let Some(buf) = self.code_buf.as_mut() {
                    buf.push_str(&t);
                } else if self.in_table {
                    if let Some(cell) = self.cells.last_mut() {
                        cell.push_str(&t);
                    }
                } else {
                    self.push_text(&t, false);
                }
            }
            Event::Code(c) => self.push_text(&c, true),
            Event::SoftBreak => {
                self.push_text("\n", false);
            }
            Event::HardBreak => {
                self.push_text("\n\n", false);
            }
            Event::Rule => {
                ui.add_space(6.0);
                ui.separator();
                ui.add_space(6.0);
            }
            _ => {}
        }
    }

    /// Render pre-built runs with every span forced to `size`.
    fn render_runs_at(&mut self, ui: &mut egui::Ui, runs: Vec<Span>, size: f32, gap: f32) {
        if runs.is_empty() {
            return;
        }
        let mut job = egui::text::LayoutJob::default();
        for r in runs {
            let mut fmt = r.fmt(self.style);
            fmt.font_id = egui::FontId::new(size, fmt.font_id.family);
            fmt.line_height = Some(size * 1.4);
            job.append(&r.text, 0.0, fmt);
        }
        job.wrap.max_width = ui.available_width();
        let galley = ui.painter().layout_job(job);
        ui.add(egui::Label::new(galley));
        if gap > 0.0 {
            ui.add_space(gap);
        }
    }

    fn push_list_marker(&mut self, _ui: &mut egui::Ui) {
        let depth = self.list.len().saturating_sub(1);
        let indent = "   ".repeat(depth.min(4));
        let marker = match self.list.last() {
            Some((true, n)) => format!("{n}. "),
            _ => "• ".to_string(),
        };
        self.runs.push(prefix_raw(indent + &marker, self.style));
    }

    fn begin_code_block(&mut self, ui: &mut egui::Ui, lang: &str) {
        ui.add_space(4.0);
        if !lang.is_empty() {
            ui.label(
                egui::RichText::new(lang)
                    .monospace()
                    .size(10.5)
                    .weak()
                    .color(self.style.code_tag),
            );
        }
        self.code_buf = Some(String::new());
    }

    fn end_code_block(&mut self, ui: &mut egui::Ui, code: String) {
        egui::ScrollArea::horizontal()
            .auto_shrink(egui::Vec2b::new(true, false))
            .show(ui, |ui| {
                for line in code.lines().chain(std::iter::once("")) {
                    let s = if line.is_empty() {
                        " ".to_string()
                    } else {
                        line.to_string()
                    };
                    ui.label(egui::RichText::new(s).monospace().size(self.style.code));
                }
            });
        ui.add_space(6.0);
    }

    fn draw_table_row(&mut self, ui: &mut egui::Ui, mut row: Vec<String>, is_head: bool) {
        while row.len() < self.ncols {
            row.push(String::new());
        }
        ui.columns(row.len().max(1), |columns| {
            for (column, cell) in columns.iter_mut().zip(row) {
                let text = egui::RichText::new(cell).size(self.style.table);
                column.add(egui::Label::new(if is_head { text.strong() } else { text }).wrap());
            }
        });
    }

    fn finish(&mut self, ui: &mut egui::Ui) {
        if !self.runs.is_empty() {
            self.flush_runs(ui, None, 0.0);
        }
    }
}

fn prefix(txt: &str, style: Style) -> Span {
    Span {
        text: txt.to_string(),
        mono: false,
        bold: false,
        italic: false,
        strike: false,
        color: Some(style.muted),
        size: style.body,
    }
}

fn prefix_raw(txt: String, style: Style) -> Span {
    let mut s = prefix("", style);
    s.text = txt;
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Exercises every construct the engine yields: H1–H4, all emphasis
    /// flavors, inline code, nested bullets + ordered list, quote, rule,
    /// a 3-column table and a fenced block.
    const FIXTURE: &str = "# Alpha one\n\
## Alpha two\n\
### Alpha three\n\
#### Alpha four\n\
\n\
Mix of **boldmix**, *italicmix*, ~~struckmix~~ and `codemix` words.\n\
\n\
- outer bullet\n\
  - nested bullet\n\
1. first ordered\n\
2. second ordered\n\
\n\
> quoted line\n\
\n\
---\n\
\n\
| HeadA | HeadB | HeadC |\n\
| ----- | ----- | -----\n\
| cell1 | cell2 | cell3 |\n\
\n\
```rust\n\
let sentinel = 1;\n\
```";

    fn paint_doc(doc: &str, style: Style) -> egui::FullOutput {
        let ctx = egui::Context::default();
        ctx.set_visuals(theme::packet_visuals());
        ctx.run_ui(egui::RawInput::default(), |ui| {
            egui::CentralPanel::default().show(ui, |ui| paint(ui, doc, style));
        })
    }

    fn galleys(output: &egui::FullOutput) -> Vec<&egui::Galley> {
        output
            .shapes
            .iter()
            .filter_map(|clipped| match &clipped.shape {
                egui::Shape::Text(shape) => Some(&*shape.galley),
                _ => None,
            })
            .collect()
    }

    fn galley_texts(output: &egui::FullOutput) -> Vec<String> {
        galleys(output)
            .iter()
            .map(|galley| galley.text().to_owned())
            .collect()
    }

    fn line_count(output: &egui::FullOutput) -> usize {
        output
            .shapes
            .iter()
            .filter(|clipped| matches!(clipped.shape, egui::Shape::LineSegment { .. }))
            .count()
    }

    fn section_text<'a>(galley: &'a egui::Galley, section: &egui::text::LayoutSection) -> &'a str {
        &galley.job.text[section.byte_range.start.0..section.byte_range.end.0]
    }

    fn find_section<'a>(
        galley: &'a egui::Galley,
        needle: &str,
    ) -> Option<&'a egui::text::LayoutSection> {
        galley
            .job
            .sections
            .iter()
            .find(|section| section_text(galley, section) == needle)
    }

    /// Raw-syntax scrub check: no galley of the fully formed fixture may
    /// retain emphasis, heading, code or fence markers.
    #[test]
    fn formatted_fixture_carries_no_raw_markers() {
        let mut output = paint_doc(FIXTURE, CHAT);
        let texts = galley_texts(&output);
        for text in &texts {
            for marker in ["**", "~~", "###", "```", "`", "#"] {
                assert!(
                    !text.contains(marker),
                    "galley {text:?} still carries {marker:?}"
                );
            }
        }
        // Every construct actually landed somewhere in the painted output.
        for needle in [
            "Alpha one",
            "Alpha two",
            "Alpha three",
            "Alpha four",
            "outer bullet",
            "nested bullet",
            "first ordered",
            "quoted line",
            "HeadA",
            "cell3",
            "let sentinel = 1;",
            "rust",
        ] {
            assert!(
                texts.iter().any(|t| t.contains(needle)),
                "fixture construct {needle:?} missing from {texts:?}"
            );
        }
        // Rule + table head both emit line separators.
        assert!(line_count(&output) >= 2);
        output.textures_delta.clear();
    }

    /// H1 scales down to `CHAT.h1` and renders in theme ink.
    #[test]
    fn chat_h1_sizes_to_theme_ink() {
        let mut output = paint_doc(FIXTURE, CHAT);
        let heading = galleys(&output)
            .into_iter()
            .find(|galley| galley.text() == "Alpha one")
            .expect("H1 galley");
        for section in &heading.job.sections {
            assert!(
                (section.format.font_id.size - CHAT.h1).abs() < 1e-6,
                "H1 size {} != {}",
                section.format.font_id.size,
                CHAT.h1
            );
            assert_eq!(section.format.color, theme::TEXT);
        }
        output.textures_delta.clear();
    }

    /// Bold carries its own TextFormat (forced ink + letter spacing),
    /// italic toggles, struck spans stroke in the muted tone.
    #[test]
    fn emphasis_sections_carry_distinct_formatted_styles() {
        let mut output = paint_doc(FIXTURE, CHAT);
        let body = galleys(&output)
            .into_iter()
            .find(|galley| galley.text().contains("boldmix"))
            .expect("body paragraph galley");
        let bold_section = find_section(body, "boldmix").expect("bold section");
        let plain_section = find_section(body, "Mix of ").expect("plain lead section");
        assert_ne!(
            bold_section.format, plain_section.format,
            "bold differs from plain"
        );
        assert!(bold_section.format.extra_letter_spacing.abs() > 1e-9);
        assert!(plain_section.format.extra_letter_spacing.abs() < f32::EPSILON);
        assert_eq!(bold_section.format.color, theme::TEXT);
        let italic_section = find_section(body, "italicmix").expect("italic section");
        assert!(italic_section.format.italics);
        let strike_section = find_section(body, "struckmix").expect("strikethrough section");
        assert!(strike_section.format.strikethrough.width.abs() > 1e-9);
        assert_eq!(strike_section.format.strikethrough.color, theme::TEXT_DIM);
        output.textures_delta.clear();
    }

    /// Inline code and fenced lines lay out monospace at their style sizes;
    /// the fence carries its language caption.
    #[test]
    fn code_spans_and_fence_use_monospace_regime() {
        let mut output = paint_doc(FIXTURE, CHAT);
        let inline = galleys(&output)
            .into_iter()
            .find(|galley| galley.text().contains("codemix"))
            .expect("inline code paragraph");
        let inline_section = find_section(inline, "codemix").expect("inline code section");
        assert_eq!(
            inline_section.format.font_id.family,
            egui::FontFamily::Monospace
        );
        let fence_line = galleys(&output)
            .into_iter()
            .find(|galley| galley.text() == "let sentinel = 1;")
            .expect("fenced code line");
        assert!(
            fence_line
                .job
                .sections
                .iter()
                .all(|section| section.format.font_id.family == egui::FontFamily::Monospace)
        );
        assert!(
            fence_line
                .job
                .sections
                .iter()
                .all(|section| (section.format.font_id.size - CHAT.code).abs() < 1e-6)
        );
        let caption = galleys(&output)
            .into_iter()
            .find(|galley| galley.text() == "rust")
            .expect("language caption");
        assert_eq!(
            caption.job.sections.first().unwrap().format.font_id.family,
            egui::FontFamily::Monospace
        );
        output.textures_delta.clear();
    }

    /// Tables land as one label per cell at the table size.
    #[test]
    fn table_cells_lay_out_individually() {
        let mut output = paint_doc(FIXTURE, CHAT);
        let texts = galley_texts(&output);
        for cell in ["HeadA", "cell1", "cell3"] {
            assert!(
                texts.iter().any(|t| t.contains(cell)),
                "{cell:?} missing from {texts:?}"
            );
        }
        for text in &texts {
            assert!(!text.contains('|'), "pipe glyph leaked into {text:?}");
        }
        output.textures_delta.clear();
    }

    /// A lone `---` is a thematic break and nothing else.
    #[test]
    fn lone_rule_is_one_separator_line() {
        let mut output = paint_doc("---", CHAT);
        assert_eq!(line_count(&output), 1);
        assert!(galley_texts(&output).is_empty());
        output.textures_delta.clear();
    }

    /// Degenerate bodies must never panic and must emit exactly their
    /// expected shape census.
    #[test]
    fn degenerate_bodies_emit_expected_shape_censuses() {
        // The fence pads with one trailing spacer line, hence the unclosed
        // fence tallies 3 = language caption + code line + spacer.
        for (doc, want_texts) in [
            ("", 0usize),
            ("\n   \n\t ", 0),
            ("solo word", 1),
            ("```rust\nlet value;", 3),
        ] {
            let mut output = paint_doc(doc, CHAT);
            let texts = galley_texts(&output);
            assert_eq!(texts.len(), want_texts, "doc {doc:?} gave {texts:?}");
            assert!(
                texts.iter().all(|t| !t.contains("```")),
                "unclosed fence leaked glyphs for {doc:?}"
            );
            output.textures_delta.clear();
        }
    }

    /// PARSER-LEVEL PIN: the paper constants equal the viewer's incumbent
    /// RGB triples and sizes byte-for-byte.
    #[test]
    fn paper_pins_the_incumbent_light_palette_literals() {
        assert_eq!(PAPER.ink, egui::Color32::from_rgb(31, 41, 55));
        assert_eq!(PAPER.muted, egui::Color32::from_rgb(85, 98, 116));
        assert_eq!(PAPER.code_tag, egui::Color32::from_rgb(23, 83, 151));
        let sizes = [
            PAPER.body,
            PAPER.h1,
            PAPER.h2,
            PAPER.h3,
            PAPER.h_other,
            PAPER.code,
            PAPER.table,
        ];
        let expected = [15.0f32, 30.0, 22.0, 18.0, 13.5, 12.0, 13.5];
        for (actual, want) in sizes.iter().zip(expected) {
            assert!(
                (actual - want).abs() < 1e-6,
                "PAPER size {actual} != {want}"
            );
        }
    }

    /// The chat palette themes off `theme.rs` with chat-density sizes.
    /// Beyond the constant pins, PAPER paints the full fixture with the
    /// incumbent sizes, colors, monospace regimes, and letter spacing.
    #[test]
    fn paper_style_preserves_the_fixture_structurally() {
        let mut output = paint_doc(FIXTURE, PAPER);
        let galls = galleys(&output);
        let heading = galls
            .iter()
            .find(|g| g.text() == "Alpha one")
            .expect("h1 galley");
        for section in &heading.job.sections {
            assert!((section.format.font_id.size - PAPER.h1).abs() < 1e-6);
            assert_eq!(section.format.color, PAPER.ink);
        }
        let para = galls
            .iter()
            .find(|g| g.text().contains("boldmix"))
            .expect("mixed paragraph");
        let bold = find_section(para, "boldmix").expect("bold section");
        let expected_spacing = 0.01 * PAPER.body;
        assert!(
            (bold.format.extra_letter_spacing - expected_spacing).abs() < expected_spacing * 1e-3
        );
        assert!((bold.format.font_id.size - PAPER.body).abs() < 1e-6);
        assert_eq!(bold.format.color, PAPER.ink);
        let plain = find_section(para, "Mix of ").expect("plain section");
        assert!(plain.format.extra_letter_spacing.abs() < f32::EPSILON);
        assert!((plain.format.font_id.size - PAPER.body).abs() < 1e-6);
        let fence_line = galls
            .iter()
            .find(|g| g.text() == "let sentinel = 1;")
            .expect("fence code line");
        assert!(
            fence_line
                .job
                .sections
                .iter()
                .all(|s| s.format.font_id.family == egui::FontFamily::Monospace)
        );
        assert!((fence_line.job.sections[0].format.font_id.size - PAPER.code).abs() < 1e-6);
        let header_cell = galls
            .iter()
            .find(|g| g.text() == "HeadA")
            .expect("table header cell");
        assert!(
            header_cell
                .job
                .sections
                .iter()
                .all(|s| s.format.font_id.family == egui::FontFamily::Proportional)
        );
        assert!((header_cell.job.sections[0].format.font_id.size - PAPER.table).abs() < 1e-6);
        output.textures_delta.clear();
    }

    #[test]
    fn chat_themes_off_the_workspace_palette() {
        assert_eq!(CHAT.ink, theme::TEXT);
        assert_eq!(CHAT.muted, theme::TEXT_DIM);
        assert_eq!(CHAT.code_tag, theme::TEXT_DIM);
        let sizes = [
            CHAT.body,
            CHAT.h1,
            CHAT.h2,
            CHAT.h3,
            CHAT.h_other,
            CHAT.code,
            CHAT.table,
        ];
        let expected = [15.0f32, 20.0, 17.0, 15.5, 15.0, 12.0, 13.0];
        for (actual, want) in sizes.iter().zip(expected) {
            assert!((actual - want).abs() < 1e-6, "CHAT size {actual} != {want}");
        }
    }

    /// Streaming sweep: every sampled in-flight prefix parses and repaints
    /// without panicking; the complete input formats fully.
    #[test]
    fn streaming_prefixes_never_panic_and_the_complete_input_formats() {
        let full = FIXTURE.len();
        let mut painted = 0usize;
        for len in 1..=full {
            // Varied strides: dense early cuts plus sparse long sweeps.
            if len < full && !matches!(len % 7, 0 | 1 | 3) && len % 11 != 0 {
                continue;
            }
            let mut out = paint_doc(&FIXTURE[..len], CHAT);
            out.textures_delta.clear();
            painted += 1;
        }
        assert!(
            painted > full / 3,
            "sweep covered {painted}/{full} prefixes"
        );
        let mut output = paint_doc(FIXTURE, CHAT);
        assert!(
            !galley_texts(&output).is_empty(),
            "complete reply must paint"
        );
        output.textures_delta.clear();
    }

    /// Degrade-not-drop contract: a prefix stranded mid-**bold** keeps the
    /// literal star(s) until the closer arrives; the closed form is clean.
    #[test]
    fn half_finished_bold_degrades_to_literal_glyphs() {
        let open = FIXTURE.find("**boldmix**").expect("fixture bold open");
        let prefix_end = open + 3; // ends inside the opener: "**b"
        let mut open_output = paint_doc(&FIXTURE[..prefix_end], CHAT);
        assert!(
            galley_texts(&open_output)
                .iter()
                .any(|text| text.contains('*')),
            "mid-emphasis prefix must keep literal stars, got {:?}",
            galley_texts(&open_output)
        );
        let mut closed = paint_doc(FIXTURE, CHAT);
        assert!(!galley_texts(&closed).iter().any(|text| text.contains('*')));
        open_output.textures_delta.clear();
        closed.textures_delta.clear();
    }
}
