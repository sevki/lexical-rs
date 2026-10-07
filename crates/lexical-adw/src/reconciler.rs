//! Applies a [`Layout`] to a `GtkTextBuffer`: minimal text diff, then styling tags.

use adw::gtk;
use adw::gtk::pango;
use adw::gtk::prelude::*;
use lexical_core::{Align, BlockStyle, HeadingTag, Layout, TextFormat};

const INDENT_PX: i32 = 24;

/// Tag palette shared by every view on a buffer.
pub struct Tags {
    bold: gtk::TextTag,
    italic: gtk::TextTag,
    underline: gtk::TextTag,
    strike: gtk::TextTag,
    code: gtk::TextTag,
    sub: gtk::TextTag,
    sup: gtk::TextTag,
    highlight: gtk::TextTag,
    link: gtk::TextTag,
    quote: gtk::TextTag,
    code_block: gtk::TextTag,
    marker: gtk::TextTag,
    headings: [gtk::TextTag; 6],
    center: gtk::TextTag,
    right: gtk::TextTag,
    justify: gtk::TextTag,
}

impl Tags {
    pub fn install(buffer: &gtk::TextBuffer) -> Tags {
        let t = |name: &str, props: &[(&str, &dyn ToValue)]| {
            buffer.create_tag(Some(name), props).expect("duplicate tag name")
        };
        let heading = |n: usize, scale: f64| {
            t(
                &format!("lexical-h{n}"),
                &[
                    ("weight", &700i32),
                    ("scale", &scale),
                    ("pixels-above-lines", &8i32),
                    ("pixels-below-lines", &4i32),
                ],
            )
        };
        Tags {
            bold: t("lexical-bold", &[("weight", &700i32)]),
            italic: t("lexical-italic", &[("style", &pango::Style::Italic)]),
            underline: t("lexical-underline", &[("underline", &pango::Underline::Single)]),
            strike: t("lexical-strike", &[("strikethrough", &true)]),
            code: t(
                "lexical-code",
                &[("family", &"monospace"), ("background", &"rgba(127,127,127,0.18)")],
            ),
            sub: t("lexical-sub", &[("rise", &(-3000i32)), ("scale", &0.8f64)]),
            sup: t("lexical-sup", &[("rise", &6000i32), ("scale", &0.8f64)]),
            highlight: t("lexical-highlight", &[("background", &"rgba(246,211,45,0.45)")]),
            link: t(
                "lexical-link",
                &[("foreground", &"#3584e4"), ("underline", &pango::Underline::Single)],
            ),
            quote: t(
                "lexical-quote",
                &[
                    ("left-margin", &(INDENT_PX)),
                    ("style", &pango::Style::Italic),
                    ("foreground", &"#77767b"),
                ],
            ),
            code_block: t(
                "lexical-code-block",
                &[
                    ("family", &"monospace"),
                    ("paragraph-background", &"rgba(127,127,127,0.14)"),
                    ("left-margin", &12i32),
                ],
            ),
            marker: t("lexical-marker", &[("foreground", &"#77767b")]),
            headings: [
                heading(1, 2.0),
                heading(2, 1.6),
                heading(3, 1.35),
                heading(4, 1.2),
                heading(5, 1.1),
                heading(6, 1.0),
            ],
            center: t("lexical-center", &[("justification", &gtk::Justification::Center)]),
            right: t("lexical-right", &[("justification", &gtk::Justification::Right)]),
            justify: t("lexical-justify", &[("justification", &gtk::Justification::Fill)]),
        }
    }

    fn margin_tag(&self, buffer: &gtk::TextBuffer, left: i32, hang: i32) -> gtk::TextTag {
        let name = format!("lexical-margin-{left}-{hang}");
        buffer.tag_table().lookup(&name).unwrap_or_else(|| {
            buffer
                .create_tag(Some(&name), &[("left-margin", &left), ("indent", &(-hang))])
                .expect("margin tag")
        })
    }
}

fn heading_tag(tags: &Tags, h: HeadingTag) -> &gtk::TextTag {
    &tags.headings[h.level() as usize - 1]
}

fn char_prefix_len(a: &str, b: &str) -> usize {
    a.chars().zip(b.chars()).take_while(|(x, y)| x == y).count()
}

fn char_suffix_len(a: &str, b: &str, max: usize) -> usize {
    a.chars().rev().zip(b.chars().rev()).take(max).take_while(|(x, y)| x == y).count()
}

/// Bring `buffer` in line with `layout`. Edits only the differing middle of the text.
pub fn apply(buffer: &gtk::TextBuffer, tags: &Tags, layout: &Layout) {
    let (s, e) = buffer.bounds();
    let old = buffer.text(&s, &e, false).to_string();
    if old != layout.text {
        let (ol, nl) = (old.chars().count(), layout.text.chars().count());
        let p = char_prefix_len(&old, &layout.text);
        let sfx = char_suffix_len(&old, &layout.text, ol.min(nl) - p);
        if ol - sfx > p {
            let mut a = buffer.iter_at_offset(p as i32);
            let mut b = buffer.iter_at_offset((ol - sfx) as i32);
            buffer.delete(&mut a, &mut b);
        }
        if nl - sfx > p {
            let ins: String = layout.text.chars().skip(p).take(nl - sfx - p).collect();
            let mut at = buffer.iter_at_offset(p as i32);
            buffer.insert(&mut at, &ins);
        }
    }
    let (s, e) = buffer.bounds();
    buffer.remove_all_tags(&s, &e);

    let range = |a: usize, b: usize| (buffer.iter_at_offset(a as i32), buffer.iter_at_offset(b as i32));
    let tag = |t: &gtk::TextTag, a: usize, b: usize| {
        let (x, y) = range(a, b);
        buffer.apply_tag(t, &x, &y);
    };
    for line in &layout.lines {
        // Tag the newline too so paragraph-level properties cover the whole line.
        let end = (line.end + 1).min(layout.char_len());
        let extra = i32::try_from(line.indent).unwrap_or(0) * INDENT_PX;
        match &line.style {
            BlockStyle::Heading(h) => tag(heading_tag(tags, *h), line.start, end),
            BlockStyle::Quote => tag(&tags.quote, line.start, end),
            BlockStyle::Code => tag(&tags.code_block, line.start, end),
            BlockStyle::ListItem { depth, .. } => {
                let left = INDENT_PX * (*depth as i32 + 1) + extra;
                tag(&tags.margin_tag(buffer, left, 18), line.start, end);
                tag(&tags.marker, line.start, line.content_start);
            }
            BlockStyle::Paragraph => {
                if extra > 0 {
                    tag(&tags.margin_tag(buffer, extra, 0), line.start, end);
                }
            }
        }
        match line.align {
            Align::Center => tag(&tags.center, line.start, end),
            Align::Right => tag(&tags.right, line.start, end),
            Align::Justify => tag(&tags.justify, line.start, end),
            Align::Start | Align::Left => {}
        }
    }
    for run in &layout.runs {
        let f = run.format;
        for (flag, t) in [
            (TextFormat::BOLD, &tags.bold),
            (TextFormat::ITALIC, &tags.italic),
            (TextFormat::UNDERLINE, &tags.underline),
            (TextFormat::STRIKETHROUGH, &tags.strike),
            (TextFormat::CODE, &tags.code),
            (TextFormat::SUBSCRIPT, &tags.sub),
            (TextFormat::SUPERSCRIPT, &tags.sup),
            (TextFormat::HIGHLIGHT, &tags.highlight),
        ] {
            if f.contains(flag) {
                tag(t, run.start, run.end);
            }
        }
        if run.link.is_some() {
            tag(&tags.link, run.start, run.end);
        }
    }
}
