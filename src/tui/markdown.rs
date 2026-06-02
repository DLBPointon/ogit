//! Converts Markdown to ratatui `Line` values for display in a `Paragraph`.
//!
//! HTML embedded in the Markdown (via `Event::Html` / `Event::InlineHtml`) is
//! handled with best-effort rendering:
//!
//! * Inline tags (`<b>`, `<i>`, `<code>`, `<kbd>`, `<br>`, `<img>`, `<a>` …)
//!   are mapped to ratatui styles or converted to readable placeholders.
//! * Block HTML (`<details>/<summary>`, `<pre>`, `<table>` …) has its tags
//!   stripped and its HTML entities decoded so only readable text remains.
//! * `<details>` gets a ▸ summary line followed by indented body text.

use pulldown_cmark::{Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

/// Convert Markdown `src` into a `Vec<Line<'static>>` suitable for a ratatui `Paragraph`.
/// `width` is the column width available; lines longer than this are word-wrapped.
pub fn markdown_to_lines(src: &str, width: usize) -> Vec<Line<'static>> {
    if src.trim().is_empty() {
        return vec![];
    }
    let opts = Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TABLES | Options::ENABLE_TASKLISTS;
    let mut state = State::new(width);
    for event in Parser::new_ext(src, opts) {
        state.handle(event);
    }
    state.finish()
}

// ── Internal state machine ────────────────────────────────────────────────────

#[derive(Default)]
struct State {
    /// Column width for word-wrapping; 0 means no wrapping.
    width: usize,
    lines: Vec<Line<'static>>,
    spans: Vec<Span<'static>>,
    style_stack: Vec<Style>,
    in_code_block: bool,
    in_blockquote: bool,
    list_stack: Vec<Option<u64>>,
    item_counts: Vec<u64>,
    item_prefix: Option<String>,
    link_url: Option<String>,
}

impl State {
    fn new(width: usize) -> Self {
        Self {
            width,
            ..Default::default()
        }
    }

    fn cur_style(&self) -> Style {
        self.style_stack.last().copied().unwrap_or_default()
    }
    fn push_style(&mut self, new_style: Style) {
        self.style_stack.push(new_style);
    }
    fn pop_style(&mut self) {
        self.style_stack.pop();
    }

    fn flush_line(&mut self) {
        let spans = std::mem::take(&mut self.spans);
        if self.in_blockquote {
            let prefix = Span::styled("▌ ".to_string(), Style::default().fg(Color::DarkGray));
            let inner_w = self.width.saturating_sub(2);
            for mut line in word_wrap_spans(spans, inner_w) {
                line.spans.insert(0, prefix.clone());
                self.lines.push(line);
            }
        } else {
            self.lines.extend(word_wrap_spans(spans, self.width));
        }
    }
    fn flush_if_pending(&mut self) {
        if !self.spans.is_empty() {
            self.flush_line();
        }
    }
    fn blank(&mut self) {
        self.lines.push(Line::default());
    }

    fn add_text(&mut self, text: String) {
        let style = self.cur_style();
        if let Some(prefix) = self.item_prefix.take() {
            if style == Style::default() {
                self.spans.push(Span::raw(prefix));
            } else {
                self.spans.push(Span::styled(prefix, style));
            }
        }
        if style == Style::default() {
            self.spans.push(Span::raw(text));
        } else {
            self.spans.push(Span::styled(text, style));
        }
    }

    fn finish(mut self) -> Vec<Line<'static>> {
        self.flush_if_pending();
        self.lines
    }

    // ── HTML helpers ──────────────────────────────────────────────────────

    /// Handle a single inline HTML tag (e.g. `<br>`, `<b>`, `</b>`, `<img …>`).
    fn handle_inline_html_tag(&mut self, html: &str) {
        let inner = html
            .trim()
            .trim_start_matches('<')
            .trim_end_matches('>')
            .trim();
        let is_closing = inner.starts_with('/');
        let tag: String = inner
            .trim_start_matches('/')
            .trim()
            .chars()
            .take_while(|tag_char| !tag_char.is_ascii_whitespace() && *tag_char != '/')
            .collect::<String>()
            .to_lowercase();

        if is_closing {
            match tag.as_str() {
                "b" | "strong" | "i" | "em" | "u" | "s" | "del" | "strike" | "code" | "kbd"
                | "samp" | "tt" | "sup" | "sub" | "mark" => {
                    self.pop_style();
                }
                "a" => {
                    if let Some(url) = self.link_url.take() {
                        self.spans.push(Span::styled(
                            format!(" ({})", url),
                            Style::default().fg(Color::DarkGray),
                        ));
                    }
                }
                "p" | "div" | "blockquote" | "li" | "dt" | "dd" => {
                    self.flush_if_pending();
                }
                _ => {}
            }
        } else {
            match tag.as_str() {
                "br" => self.flush_line(),
                "hr" => {
                    self.flush_if_pending();
                    self.lines.push(Line::from(Span::styled(
                        "─".repeat(40),
                        Style::default().fg(Color::DarkGray),
                    )));
                }
                "b" | "strong" => {
                    let updated_style = self.cur_style().add_modifier(Modifier::BOLD);
                    self.push_style(updated_style);
                }
                "i" | "em" => {
                    let updated_style = self.cur_style().add_modifier(Modifier::ITALIC);
                    self.push_style(updated_style);
                }
                "u" | "mark" => {
                    let updated_style = self.cur_style().add_modifier(Modifier::UNDERLINED);
                    self.push_style(updated_style);
                }
                "s" | "del" | "strike" => {
                    let updated_style = self.cur_style().add_modifier(Modifier::CROSSED_OUT);
                    self.push_style(updated_style);
                }
                "code" | "kbd" | "samp" | "tt" => {
                    self.push_style(Style::default().add_modifier(Modifier::REVERSED));
                }
                "sup" | "sub" => {
                    // Can't do super/subscript in a terminal — dim it slightly
                    let updated_style = self.cur_style().fg(Color::DarkGray);
                    self.push_style(updated_style);
                }
                "img" => {
                    let alt = extract_attr(html, "alt")
                        .filter(|alt_text| !alt_text.is_empty())
                        .unwrap_or_else(|| "image".to_string());
                    self.spans.push(Span::styled(
                        format!("[{}]", alt),
                        Style::default().fg(Color::DarkGray),
                    ));
                }
                "a" => {
                    if let Some(href) = extract_attr(html, "href") {
                        self.link_url = Some(href);
                    }
                }
                "p" | "div" | "li" => {
                    self.flush_if_pending();
                }
                _ => {}
            }
        }
    }

    /// Handle a block-level `<details>…</details>` element: show ▸ summary
    /// then the body text indented.
    fn handle_block_details(&mut self, html: &str) {
        let lower = html.to_lowercase();

        // Extract <summary> text
        let summary: Option<String> = lower.find("<summary>").and_then(|summary_idx| {
            let content_start = summary_idx + "<summary>".len();
            lower[content_start..].find("</summary>").map(|end| {
                let raw = &html[content_start..content_start + end];
                html_decode(&html_strip(raw)).trim().to_string()
            })
        });

        if let Some(ref summary_text) = summary {
            if !summary_text.is_empty() {
                self.spans.push(Span::styled(
                    format!("▸ {}", summary_text),
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ));
                self.flush_line();
            }
        }

        // Extract body (between </summary> and </details>)
        let body_start = lower
            .find("</summary>")
            .map(|found_pos| found_pos + "</summary>".len())
            .unwrap_or(0);
        let body_end = lower.rfind("</details>").unwrap_or(html.len());

        if body_start < body_end {
            let body_text = html_decode(&html_strip(&html[body_start..body_end]));
            for line in body_text.lines() {
                let trimmed = line.trim();
                if !trimmed.is_empty() {
                    self.add_text(format!("  {}", trimmed));
                    self.flush_if_pending();
                }
            }
        }

        self.blank();
    }

    // ── Event dispatch ────────────────────────────────────────────────────

    fn handle(&mut self, event: Event<'_>) {
        match event {
            // ── Headings ──────────────────────────────────────────────────
            Event::Start(Tag::Heading { level, .. }) => {
                let style = match level {
                    HeadingLevel::H1 | HeadingLevel::H2 => Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                    _ => Style::default()
                        .fg(Color::DarkGray)
                        .add_modifier(Modifier::BOLD),
                };
                self.push_style(style);
            }
            Event::End(TagEnd::Heading(level)) => {
                self.pop_style();
                let heading_spans = std::mem::take(&mut self.spans);
                self.lines.push(Line::from(heading_spans));
                if level == HeadingLevel::H1 {
                    self.lines.push(Line::from(Span::styled(
                        "═".repeat(60),
                        Style::default().fg(Color::Cyan),
                    )));
                }
                self.blank();
            }

            // ── Paragraphs ────────────────────────────────────────────────
            Event::Start(Tag::Paragraph) => {}
            Event::End(TagEnd::Paragraph) => {
                self.flush_if_pending();
                self.blank();
            }

            // ── Emphasis / Strong / Strikethrough ─────────────────────────
            Event::Start(Tag::Emphasis) => {
                let updated_style = self.cur_style().add_modifier(Modifier::ITALIC);
                self.push_style(updated_style);
            }
            Event::End(TagEnd::Emphasis) => self.pop_style(),

            Event::Start(Tag::Strong) => {
                let updated_style = self.cur_style().add_modifier(Modifier::BOLD);
                self.push_style(updated_style);
            }
            Event::End(TagEnd::Strong) => self.pop_style(),

            Event::Start(Tag::Strikethrough) => {
                let updated_style = self.cur_style().add_modifier(Modifier::CROSSED_OUT);
                self.push_style(updated_style);
            }
            Event::End(TagEnd::Strikethrough) => self.pop_style(),

            // ── Inline code ───────────────────────────────────────────────
            Event::Code(text) => {
                self.spans.push(Span::styled(
                    text.into_string(),
                    Style::default().add_modifier(Modifier::REVERSED),
                ));
            }

            // ── Code blocks ───────────────────────────────────────────────
            Event::Start(Tag::CodeBlock(_)) => {
                self.in_code_block = true;
            }
            Event::End(TagEnd::CodeBlock) => {
                self.in_code_block = false;
                self.blank();
            }

            // ── Block quotes ──────────────────────────────────────────────
            Event::Start(Tag::BlockQuote(_)) => {
                self.in_blockquote = true;
            }
            Event::End(TagEnd::BlockQuote(_)) => {
                self.flush_if_pending();
                self.in_blockquote = false;
                self.blank();
            }

            // ── Lists ─────────────────────────────────────────────────────
            Event::Start(Tag::List(n)) => {
                self.list_stack.push(n);
                self.item_counts.push(n.unwrap_or(1).saturating_sub(1));
            }
            Event::End(TagEnd::List(_)) => {
                self.list_stack.pop();
                self.item_counts.pop();
                if self.list_stack.is_empty() {
                    self.blank();
                }
            }
            Event::Start(Tag::Item) => {
                let depth = self.list_stack.len().saturating_sub(1);
                let indent = "  ".repeat(depth);
                let bullet: String = match self.list_stack.last() {
                    Some(None) => "• ".to_string(),
                    Some(Some(_)) => {
                        if let Some(c) = self.item_counts.last_mut() {
                            *c += 1;
                            format!("{}. ", c)
                        } else {
                            "• ".to_string()
                        }
                    }
                    None => "• ".to_string(),
                };
                self.item_prefix = Some(format!("{}{}", indent, bullet));
            }
            Event::End(TagEnd::Item) => self.flush_if_pending(),

            // ── Links ─────────────────────────────────────────────────────
            Event::Start(Tag::Link { dest_url, .. }) => {
                self.link_url = Some(dest_url.into_string());
            }
            Event::End(TagEnd::Link) => {
                if let Some(url) = self.link_url.take() {
                    self.spans.push(Span::styled(
                        format!(" ({})", url),
                        Style::default().fg(Color::DarkGray),
                    ));
                }
            }

            // ── Images ────────────────────────────────────────────────────
            Event::Start(Tag::Image { .. }) => {
                self.push_style(Style::default().fg(Color::DarkGray));
                self.spans.push(Span::styled(
                    "[Image: ".to_string(),
                    Style::default().fg(Color::DarkGray),
                ));
            }
            Event::End(TagEnd::Image) => {
                self.pop_style();
                self.spans.push(Span::styled(
                    "]".to_string(),
                    Style::default().fg(Color::DarkGray),
                ));
            }

            // ── Horizontal rule ───────────────────────────────────────────
            Event::Rule => {
                self.flush_if_pending();
                self.lines.push(Line::from(Span::styled(
                    "─".repeat(60),
                    Style::default().fg(Color::DarkGray),
                )));
                self.blank();
            }

            // ── Text ──────────────────────────────────────────────────────
            Event::Text(text) => {
                if self.in_code_block {
                    let prefix = if self.in_blockquote { "▌   " } else { "  " };
                    for src_line in text.lines() {
                        self.lines.push(Line::from(Span::styled(
                            format!("{}{}", prefix, src_line),
                            Style::default().fg(Color::Yellow),
                        )));
                    }
                } else {
                    self.add_text(text.into_string());
                }
            }

            // ── Block-level HTML ──────────────────────────────────────────
            //
            // Emitted for raw HTML blocks in the Markdown source, e.g.
            // <details>…</details>, <table>…</table>, <p>…</p>.
            Event::Html(html) => {
                let html_str = html.into_string();
                self.flush_if_pending();
                if html_str.to_lowercase().contains("<details") {
                    self.handle_block_details(&html_str);
                } else {
                    // Generic: strip tags, decode entities, emit non-blank lines.
                    let text = html_decode(&html_strip(&html_str));
                    let mut had_content = false;
                    for line in text.lines() {
                        let trimmed = line.trim();
                        if !trimmed.is_empty() {
                            self.add_text(trimmed.to_string());
                            self.flush_if_pending();
                            had_content = true;
                        }
                    }
                    if had_content {
                        self.blank();
                    }
                }
            }

            // ── Inline HTML ───────────────────────────────────────────────
            //
            // Emitted for individual HTML tags inside a paragraph, e.g.
            // <br>, <b>, </b>, <img alt="…">, <kbd>Ctrl</kbd>.
            Event::InlineHtml(html) => {
                self.handle_inline_html_tag(&html.into_string());
            }

            // ── Breaks ────────────────────────────────────────────────────
            Event::SoftBreak => {
                self.spans.push(Span::raw(" ".to_string()));
            }
            Event::HardBreak => self.flush_line(),

            // ── Task-list checkboxes ───────────────────────────────────────
            Event::TaskListMarker(checked) => {
                let marker = if checked { "☑ " } else { "☐ " };
                self.spans.push(Span::raw(marker.to_string()));
            }

            _ => {}
        }
    }
}

// ── Word-wrap helpers ───────────────────────────────────────────────────────

/// Word-wrap a list of styled spans to `width` columns.
/// Returns one [`Line`] per screen row. If `width` is 0 the spans are returned
/// as-is in a single line.
fn word_wrap_spans(spans: Vec<Span<'static>>, width: usize) -> Vec<Line<'static>> {
    if width == 0 || spans.is_empty() {
        return vec![Line::from(spans)];
    }
    // Fast-path: total chars fit on one line.
    let total: usize = spans.iter().map(|span| span.content.chars().count()).sum();
    if total <= width {
        return vec![Line::from(spans)];
    }

    // Flatten to (char, Style) so word boundaries can cross span edges.
    let styled: Vec<(char, Style)> = spans
        .iter()
        .flat_map(|span| span.content.chars().map(move |ch| (ch, span.style)))
        .collect();

    let mut lines: Vec<Line<'static>> = Vec::new();
    let mut pos = 0;

    while pos < styled.len() {
        let end = (pos + width).min(styled.len());

        // If there is more text after this chunk, step back to the last space
        // so we don't break mid-word.
        let break_at = if end < styled.len() {
            let mut break_pos = end;
            while break_pos > pos && styled[break_pos - 1].0 != ' ' {
                break_pos -= 1;
            }
            if break_pos == pos { end } else { break_pos } // hard-break if no space found
        } else {
            end
        };

        lines.push(chars_to_line(&styled[pos..break_at]));

        // Advance, skipping the space we broke on.
        pos = break_at;
        if pos < styled.len() && styled[pos].0 == ' ' {
            pos += 1;
        }
    }

    if lines.is_empty() {
        lines.push(Line::default());
    }
    lines
}

/// Re-group a slice of `(char, Style)` pairs back into a [`Line`] by merging
/// consecutive characters that share the same style into a single [`Span`].
fn chars_to_line(chars: &[(char, Style)]) -> Line<'static> {
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut char_idx = 0;
    while char_idx < chars.len() {
        let style = chars[char_idx].1;
        let mut text = String::new();
        while char_idx < chars.len() && chars[char_idx].1 == style {
            text.push(chars[char_idx].0);
            char_idx += 1;
        }
        if !text.is_empty() {
            spans.push(if style == Style::default() {
                Span::raw(text)
            } else {
                Span::styled(text, style)
            });
        }
    }
    Line::from(spans)
}

// ── HTML utility functions ────────────────────────────────────────────────────

/// Remove every `<…>` tag from `s`, leaving only the text content.
fn html_strip(html_input: &str) -> String {
    let mut out = String::with_capacity(html_input.len());
    let mut depth: usize = 0;
    for cur_char in html_input.chars() {
        match cur_char {
            '<' => depth += 1,
            '>' if depth > 0 => depth -= 1,
            _ if depth == 0 => out.push(cur_char),
            _ => {}
        }
    }
    out
}

/// Decode common named HTML entities and all numeric entities (`&#NNN;` / `&#xHH;`).
fn html_decode(encoded_str: &str) -> String {
    // Named entities — cover the ones most common in code/tech writing.
    let decoded = encoded_str
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&#39;", "'")
        .replace("&nbsp;", " ")
        .replace("&#160;", " ")
        .replace("&mdash;", "—")
        .replace("&ndash;", "–")
        .replace("&hellip;", "…")
        .replace("&laquo;", "«")
        .replace("&raquo;", "»")
        .replace("&copy;", "©")
        .replace("&reg;", "®")
        .replace("&trade;", "™")
        .replace("&euro;", "€")
        .replace("&pound;", "£")
        .replace("&yen;", "¥")
        .replace("&cent;", "¢")
        .replace("&lsquo;", "\u{2018}")
        .replace("&rsquo;", "\u{2019}")
        .replace("&ldquo;", "\u{201C}")
        .replace("&rdquo;", "\u{201D}")
        .replace("&zwj;", "")
        .replace("&zwnj;", "");

    decode_numeric_entities(&decoded)
}

/// Decode `&#DDD;` and `&#xHH;` numeric character references.
fn decode_numeric_entities(raw_input: &str) -> String {
    let mut out = String::with_capacity(raw_input.len());
    let mut chars = raw_input.chars().peekable();

    while let Some(cur_char) = chars.next() {
        if cur_char != '&' {
            out.push(cur_char);
            continue;
        }
        if chars.peek() != Some(&'#') {
            out.push('&');
            continue;
        }
        chars.next(); // consume '#'

        let hex = matches!(chars.peek(), Some(&'x') | Some(&'X'));
        if hex {
            chars.next(); // consume 'x'/'X'
        }

        let mut digits = String::new();
        while let Some(&digit_char) = chars.peek() {
            if digit_char == ';' {
                chars.next();
                break;
            }
            if (hex && digit_char.is_ascii_hexdigit()) || (!hex && digit_char.is_ascii_digit()) {
                digits.push(digit_char);
                chars.next();
            } else {
                break;
            }
        }

        let code: Option<u32> = if hex {
            u32::from_str_radix(&digits, 16).ok()
        } else {
            digits.parse().ok()
        };

        match code.and_then(char::from_u32) {
            Some(decoded) => out.push(decoded),
            None => {
                // Not a valid entity — put it back verbatim.
                out.push('&');
                out.push('#');
                if hex {
                    out.push('x');
                }
                out.push_str(&digits);
                out.push(';');
            }
        }
    }

    out
}

/// Extract the value of `attr` from a raw HTML tag string.
/// Handles both `attr="value"` and `attr='value'`.
fn extract_attr(html: &str, attr: &str) -> Option<String> {
    let lower = html.to_lowercase();
    let attr_lower = attr.to_lowercase();

    let double_quote_needle = format!("{}=\"", attr_lower);
    let single_quote_needle = format!("{}='", attr_lower);

    let (byte_start, quote_char) = if let Some(found_idx) = lower.find(&double_quote_needle) {
        (found_idx + double_quote_needle.len(), '"')
    } else if let Some(found_idx) = lower.find(&single_quote_needle) {
        (found_idx + single_quote_needle.len(), '\'')
    } else {
        return None;
    };

    html[byte_start..]
        .find(quote_char)
        .map(|end| html[byte_start..byte_start + end].to_string())
}
