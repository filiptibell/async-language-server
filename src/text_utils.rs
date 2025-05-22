use async_lsp::lsp_types::{Position as LspPosition, PositionEncodingKind as LspPositionEncoding};
use ropey::Rope;

/**
    A position including a line and column.

    May be cheaply copied, as well as converted
    to / from language server positions.
*/
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Position {
    pub line: usize,
    pub col: usize,
}

impl From<LspPosition> for Position {
    fn from(position: LspPosition) -> Self {
        Self {
            line: position.line as usize,
            col: position.character as usize,
        }
    }
}

impl From<&LspPosition> for Position {
    fn from(position: &LspPosition) -> Self {
        Self {
            line: position.line as usize,
            col: position.character as usize,
        }
    }
}

impl From<Position> for LspPosition {
    fn from(position: Position) -> Self {
        #[allow(clippy::cast_possible_truncation)]
        Self {
            line: position.line as u32,
            character: position.col as u32,
        }
    }
}

#[cfg(feature = "tree-sitter")]
mod position_tree_sitter {
    use tree_sitter::Point;

    use super::Position;

    impl From<Point> for Position {
        fn from(point: Point) -> Self {
            Self {
                line: point.row as usize,
                col: point.column as usize,
            }
        }
    }

    impl From<&Point> for Position {
        fn from(point: &Point) -> Self {
            Self {
                line: point.row as usize,
                col: point.column as usize,
            }
        }
    }
}

/**
    A position encoding supported by this library.

    Easy to copy and match against, unlike `PositionEncodingKind`, and
    contains several similar utilities, that are additionally `const`.
*/
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PositionEncoding {
    UTF8,
    UTF16,
    UTF32,
}

impl PositionEncoding {
    #[must_use]
    pub const fn default() -> Self {
        // Default according to LSP specification - will be supported by all clients
        // that do not specify which position encoding they prefer and / or support
        Self::UTF16
    }

    #[must_use]
    pub const fn into_lsp(self) -> LspPositionEncoding {
        match self {
            Self::UTF8 => LspPositionEncoding::UTF8,
            Self::UTF16 => LspPositionEncoding::UTF16,
            Self::UTF32 => LspPositionEncoding::UTF32,
        }
    }

    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::UTF8 => "utf-8",
            Self::UTF16 => "utf-16",
            Self::UTF32 => "utf-32",
        }
    }

    #[allow(clippy::missing_panics_doc)]
    #[must_use]
    pub fn from_lsp(encoding: &LspPositionEncoding) -> Self {
        if encoding == &LspPositionEncoding::UTF8 {
            Self::UTF8
        } else if encoding == &LspPositionEncoding::UTF16 {
            Self::UTF16
        } else if encoding == &LspPositionEncoding::UTF32 {
            Self::UTF32
        } else {
            panic!("unsupported position encoding kind: {encoding:?}")
        }
    }
}

impl From<&LspPositionEncoding> for PositionEncoding {
    fn from(encoding: &LspPositionEncoding) -> Self {
        Self::from_lsp(encoding)
    }
}

impl From<LspPositionEncoding> for PositionEncoding {
    fn from(value: LspPositionEncoding) -> Self {
        Self::from_lsp(&value)
    }
}

impl From<&PositionEncoding> for LspPositionEncoding {
    fn from(value: &PositionEncoding) -> Self {
        value.into_lsp()
    }
}

impl From<PositionEncoding> for LspPositionEncoding {
    fn from(value: PositionEncoding) -> Self {
        value.into_lsp()
    }
}

impl Default for PositionEncoding {
    fn default() -> Self {
        Self::default()
    }
}

/**
    Converts a LSP position to a character
    offset using the given position encoding.
*/
pub fn position_to_char_offset(
    contents: &Rope,
    position: impl Into<Position>,
    encoding: impl Into<PositionEncoding>,
) -> usize {
    let position = position.into();
    let encoding = encoding.into();

    let slice = contents.line(position.line);
    match encoding {
        PositionEncoding::UTF8 => {
            let column_bytes = position.col.min(slice.len_bytes());
            slice.byte_to_char(column_bytes)
        }
        PositionEncoding::UTF16 => {
            let column_utf16 = position.col.min(slice.len_utf16_cu());
            slice.utf16_cu_to_char(column_utf16)
        }
        PositionEncoding::UTF32 => position.col.min(slice.len_chars()),
    }
}

/**
    Converts a LSP position to a utf8 byte
    offset using the given position encoding.
*/
pub fn position_to_byte_offset(
    contents: &Rope,
    position: impl Into<Position>,
    encoding: impl Into<PositionEncoding>,
) -> usize {
    let position = position.into();
    let encoding = encoding.into();

    let slice = contents.line(position.line);
    match encoding {
        PositionEncoding::UTF8 => position.col.min(slice.len_bytes()),
        PositionEncoding::UTF16 => {
            let capped_column_utf16 = position.col.min(slice.len_utf16_cu());
            let char_idx_within_line = slice.utf16_cu_to_char(capped_column_utf16);
            slice.char_to_byte(char_idx_within_line)
        }
        PositionEncoding::UTF32 => {
            let char_idx_within_line = position.col.min(slice.len_chars());
            slice.char_to_byte(char_idx_within_line)
        }
    }
}
