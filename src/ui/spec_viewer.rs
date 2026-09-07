//! Live-rendered specification view (SPECIFICATION.md §6, §21–§22).
//!
//! READ-ONLY: mutations travel exclusively through planning turns.
//! Implementation notes:
//!   * pulldown-cmark 0.13 streaming events folded into egui text spans
//!   * styled runs composed via [`egui::LayoutJob`] (real bold/mono/color)
//!   * tables drawn as even-width [`egui::Grid`] rows
//!   * item headers like "**Q CLR-001** · Priority…" render slightly larger

use pulldown_cmark::{Event, HeadingLevel, Options, Parser, Tag, TagEnd};

use crate::ui::theme;

const NO_CONTENT_HINT: &str = "Nothing has been planned yet — describe the product in the chat and the living specification grows here.";

/// Render the current specification markdown (or a friendly placeholder).
pub fn render(ui: &mut egui::Ui, spec: Option<&str>) {
    let Some(md) = spec.filter(|s| !s.trim().is_empty()) else {
        ui.vertical_centered_justified(|ui| {
            ui.add_space(60.0);
            ui.label(
                egui::RichText::new(NO_CONTENT_HINT)
                    .weak()
                    .size(13.0)
                    .color(theme::TEXT_DIM),
            );
            ui.add_space(30.0);
        });
        return;
    };

    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_TABLES);
    opts.insert(Options::ENABLE_STRIKETHROUGH);
    let parser = Parser::new_ext(md, opts);

    let mut st = State::default();
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
    fn fmt(&self) -> egui::text::TextFormat {
        let mut f = egui::text::TextFormat::default();
        let size = self.size;
        f.font_id = if self.mono {
            egui::FontId::monospace(size)
        } else {
            egui::FontId::proportional(size)
        };
        let mut color = self.color.unwrap_or(theme::TEXT);
        if self.bold {
            color = egui::Color32::from_rgb(245, 247, 250);
            f.extra_letter_spacing = 0.01 * size;
        }
        f.color = color;
        f.line_height = Some(size * 1.5);
        f.italics = self.italic;
        f.strikethrough = if self.strike {
            egui::epaint::Stroke::new(1.0, theme::TEXT_DIM)
        } else {
            egui::epaint::Stroke::NONE
        };
        f
    }
}

#[derive(Default)]
struct State {
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
    fn base_span(&self) -> Span {
        Span {
            text: String::new(),
            mono: false,
            bold: self.strong > 0,
            italic: self.emph > 0,
            strike: self.strike > 0,
            color: None,
            size: 15.0,
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
        let size = size_override.unwrap_or(15.0);
        let mut job = egui::text::LayoutJob::default();
        for r in self.runs.drain(..) {
            let mut fmt = r.fmt();
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
                    self.runs.push(prefix("> "));
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
                    let size = heading_size(&level);
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
                if self.code_buf.is_some() {
                    self.code_buf.as_mut().unwrap().push_str(&t);
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
            let mut fmt = r.fmt();
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
            Some((true, n)) => format!("{}. ", n),
            _ => "• ".to_string(),
        };
        self.runs.push(prefix_raw(indent + &marker));
    }

    fn begin_code_block(&mut self, ui: &mut egui::Ui, lang: &str) {
        ui.add_space(4.0);
        if !lang.is_empty() {
            ui.label(
                egui::RichText::new(lang)
                    .monospace()
                    .size(10.5)
                    .weak()
                    .color(theme::ACCENT),
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
                    ui.label(egui::RichText::new(s).monospace().size(12.0));
                }
            });
        ui.add_space(6.0);
    }

    fn draw_table_row(&mut self, ui: &mut egui::Ui, mut row: Vec<String>, is_head: bool) {
        while row.len() < self.ncols {
            row.push(String::new());
        }
        ui.horizontal(|ui| {
            let _w = ui.available_width() / row.len().max(1) as f32 - 6.0;
            for cell in row {
                let rt = egui::RichText::new(cell).size(12.5);
                let rt = if is_head { rt.strong() } else { rt };
                ui.set_min_width(30.0);
                ui.add(egui::Label::new(rt));
            }
        });
    }

    fn finish(&mut self, ui: &mut egui::Ui) {
        if !self.runs.is_empty() {
            self.flush_runs(ui, None, 0.0);
        }
    }
}

fn heading_size(level: &HeadingLevel) -> f32 {
    match level {
        HeadingLevel::H1 => 30.0,
        HeadingLevel::H2 => 22.0,
        HeadingLevel::H3 => 18.0,
        _ => 13.5,
    }
}

fn prefix(txt: &str) -> Span {
    let s = Span {
        text: txt.to_string(),
        mono: false,
        bold: false,
        italic: false,
        strike: false,
        color: Some(theme::TEXT_DIM),
        size: 15.0,
    };
    s
}

fn prefix_raw(txt: String) -> Span {
    let mut s = prefix("");
    s.text = txt;
    s
}
