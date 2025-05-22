use ropey::Rope;

use super::{encoding::Encoding, position::Position};

/**
    Converts a position from using one encoding to another.
*/
pub fn position_to_encoding<P>(
    contents: &Rope,
    position: P,
    encoding_source: impl Into<Encoding>,
    encoding_target: impl Into<Encoding>,
) -> P
where
    P: Into<Position>,
    P: From<Position>,
{
    let encoding_source = encoding_source.into();
    let encoding_target = encoding_target.into();
    if encoding_target == encoding_source {
        return position;
    }

    let position = position.into();
    let slice = contents.line(position.line);
    let column = match (encoding_source, encoding_target) {
        // To UTF8
        (Encoding::UTF16, Encoding::UTF8) => {
            let capped_column_utf16 = position.col.min(slice.len_utf16_cu());
            let char_idx_within_line = slice.utf16_cu_to_char(capped_column_utf16);
            slice.char_to_byte(char_idx_within_line)
        }
        (Encoding::UTF32, Encoding::UTF8) => {
            let char_idx_within_line = position.col.min(slice.len_chars());
            slice.char_to_byte(char_idx_within_line)
        }
        // To UTF16
        (Encoding::UTF8, Encoding::UTF16) => {
            let column_bytes = position.col.min(slice.len_bytes());
            let char_idx_within_line = slice.byte_to_char(column_bytes);
            slice.char_to_utf16_cu(char_idx_within_line)
        }
        (Encoding::UTF32, Encoding::UTF16) => {
            let char_idx_within_line = position.col.min(slice.len_chars());
            slice.char_to_utf16_cu(char_idx_within_line)
        }
        // To UTF32
        (Encoding::UTF8, Encoding::UTF32) => {
            let column_bytes = position.col.min(slice.len_bytes());
            slice.byte_to_char(column_bytes)
        }
        (Encoding::UTF16, Encoding::UTF32) => {
            let column_utf16 = position.col.min(slice.len_utf16_cu());
            slice.utf16_cu_to_char(column_utf16)
        }
        // Same encoding
        _ => unreachable!(),
    };

    let pos = Position {
        line: position.line,
        col: column,
    };

    pos.into()
}
