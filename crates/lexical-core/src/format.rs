//! Text and element formatting flags (bit values match Lexical's JSON format).

use bitflags::bitflags;

bitflags! {
    /// Inline text formatting. The bit values are identical to Lexical's
    /// `TextFormatType` so serialized documents are interchangeable.
    #[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
    pub struct TextFormat: u32 {
        const BOLD = 1;
        const ITALIC = 1 << 1;
        const STRIKETHROUGH = 1 << 2;
        const UNDERLINE = 1 << 3;
        const CODE = 1 << 4;
        const SUBSCRIPT = 1 << 5;
        const SUPERSCRIPT = 1 << 6;
        const HIGHLIGHT = 1 << 7;
    }
}

impl TextFormat {
    /// Toggle `flag`, keeping subscript and superscript mutually exclusive.
    pub fn toggled(self, flag: TextFormat) -> TextFormat {
        let mut out = self ^ flag;
        if out.contains(flag) {
            if flag == TextFormat::SUBSCRIPT {
                out.remove(TextFormat::SUPERSCRIPT);
            } else if flag == TextFormat::SUPERSCRIPT {
                out.remove(TextFormat::SUBSCRIPT);
            }
        }
        out
    }
}

/// Horizontal alignment of an element.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub enum Align {
    #[default]
    Start,
    Left,
    Center,
    Right,
    Justify,
}

impl Align {
    pub fn as_str(self) -> &'static str {
        match self {
            Align::Start => "",
            Align::Left => "left",
            Align::Center => "center",
            Align::Right => "right",
            Align::Justify => "justify",
        }
    }

    pub fn parse(s: &str) -> Align {
        match s {
            "left" => Align::Left,
            "center" => Align::Center,
            "right" => Align::Right,
            "justify" => Align::Justify,
            _ => Align::Start,
        }
    }
}
