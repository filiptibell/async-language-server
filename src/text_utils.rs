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

    Easy to copy and match against, unlike `PositionEncodingKind`.
*/
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PositionEncoding {
    #[default]
    UTF8,
    UTF16,
    UTF32,
}

impl From<&LspPositionEncoding> for PositionEncoding {
    fn from(encoding: &LspPositionEncoding) -> Self {
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

impl From<LspPositionEncoding> for PositionEncoding {
    fn from(value: LspPositionEncoding) -> Self {
        Self::from(&value)
    }
}

/**
    Converts a LSP position to a ropey character
    offset using the given position encoding.
*/
pub fn position_to_ropey_char_offset(
    rope: impl AsRef<Rope>,
    position: impl Into<Position>,
    encoding: impl Into<PositionEncoding>,
) -> usize {
    let rope = rope.as_ref();
    let position = position.into();
    let encoding = encoding.into();

    let slice = rope.line(position.line);
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
pub fn position_to_utf8_byte_offset(
    rope: impl AsRef<Rope>,
    position: impl Into<Position>,
    encoding: impl Into<PositionEncoding>,
) -> usize {
    let rope = rope.as_ref();
    let position = position.into();
    let encoding = encoding.into();

    let line_slice = rope.line(position.line);
    let line_byte = rope.line_to_byte(position.line);

    let col_byte = match encoding {
        PositionEncoding::UTF8 => position.col.min(line_slice.len_bytes()),
        PositionEncoding::UTF16 => {
            let capped_column_utf16 = position.col.min(line_slice.len_utf16_cu());
            let char_idx_within_line = line_slice.utf16_cu_to_char(capped_column_utf16);
            line_slice.char_to_byte(char_idx_within_line)
        }
        PositionEncoding::UTF32 => {
            let char_idx_within_line = position.col.min(line_slice.len_chars());
            line_slice.char_to_byte(char_idx_within_line)
        }
    };

    line_byte + col_byte
}
