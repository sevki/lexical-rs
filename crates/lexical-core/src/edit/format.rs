//! Inline formatting and reading the selection back (format, plain text).

use crate::error::Result;
use crate::format::TextFormat;
use crate::selection::Point;
use crate::state::EditorState;

impl EditorState {
    pub fn format_text(&mut self, flag: TextFormat) -> Result<()> {
        let sel = self.require_selection()?;
        if sel.is_collapsed() {
            let s = self.selection.as_mut().unwrap();
            s.format = s.format.toggled(flag);
            return Ok(());
        }
        let backward = self.is_backward();
        let (start, end) = self.ordered_points()?;
        let (a, b) = self.split_range(&start, &end);
        let texts: Vec<_> = self
            .leaves_between(&a, &b)
            .into_iter()
            .filter(|&k| self.node(k).is_text())
            .collect();
        if texts.is_empty() {
            return Ok(());
        }
        let all_have = texts
            .iter()
            .all(|&k| self.node(k).text_format().contains(flag));
        for &k in &texts {
            let f = self.node(k).text_format();
            let nf = if all_have {
                f - flag
            } else {
                f.union(flag).toggled_exclusive(flag)
            };
            self.set_text_format(k, nf);
        }
        let (first, last) = (texts[0], *texts.last().unwrap());
        let (sp, ep) = (
            Point::text(first, 0),
            Point::text(last, self.node(last).text_len()),
        );
        let s = self.selection.as_mut().unwrap();
        let fmt = self.nodes[&first].text_format();
        if backward {
            s.anchor = ep;
            s.focus = sp;
        } else {
            s.anchor = sp;
            s.focus = ep;
        }
        s.format = fmt;
        Ok(())
    }

    /// Common formatting of the selected text (or pending format when collapsed).
    pub fn selection_format(&self) -> TextFormat {
        let Some(sel) = &self.selection else {
            return TextFormat::empty();
        };
        if sel.is_collapsed() {
            return sel.format;
        }
        let Ok((s, e)) = self.ordered_points() else {
            return TextFormat::empty();
        };
        let mut me = self.clone();
        let (a, b) = me.split_range(&s, &e);
        let mut acc: Option<TextFormat> = None;
        for k in me.leaves_between(&a, &b) {
            if me.node(k).is_text() {
                let f = me.node(k).text_format();
                acc = Some(acc.map_or(f, |x| x & f));
            }
        }
        acc.unwrap_or_default()
    }

    pub fn selected_text(&self) -> String {
        let Ok((s, e)) = self.ordered_points() else {
            return String::new();
        };
        let blocks = self.line_blocks();
        let (Some(bs), Some(be)) = (
            self.line_block_of(self.inline_point(&s).key),
            self.line_block_of(self.inline_point(&e).key),
        ) else {
            return String::new();
        };
        let (Some(i), Some(j)) = (
            blocks.iter().position(|&b| b == bs),
            blocks.iter().position(|&b| b == be),
        ) else {
            return String::new();
        };
        let mut out = vec![];
        for (n, &b) in blocks[i..=j].iter().enumerate() {
            let c = self.block_content(b);
            let from = if n == 0 {
                self.block_offset(&c, &self.inline_point(&s))
            } else {
                0
            };
            let to = if b == be {
                self.block_offset(&c, &self.inline_point(&e))
            } else {
                c.len
            };
            out.push(
                c.text
                    .chars()
                    .skip(from)
                    .take(to.saturating_sub(from))
                    .collect::<String>(),
            );
        }
        out.join("\n")
    }
}

impl TextFormat {
    /// After setting `flag`, drop its mutually-exclusive partner.
    pub(crate) fn toggled_exclusive(self, flag: TextFormat) -> TextFormat {
        let mut out = self;
        if flag == TextFormat::SUBSCRIPT {
            out.remove(TextFormat::SUPERSCRIPT);
        } else if flag == TextFormat::SUPERSCRIPT {
            out.remove(TextFormat::SUBSCRIPT);
        }
        out
    }
}
