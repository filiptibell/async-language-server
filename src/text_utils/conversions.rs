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

    let mut position = position.into();
    position.line = position.line.min(contents.len_lines().saturating_sub(1));

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

#[cfg(test)]
mod tests {
    use ropey::Rope;

    use super::{Encoding, Position, position_to_encoding};

    #[test]
    fn converts_utf8_columns_to_utf16() {
        let text = Rope::from_str("a🙂b");
        let position = Position { line: 0, col: 5 };

        let converted = position_to_encoding(&text, position, Encoding::UTF8, Encoding::UTF16);

        assert_eq!(converted, Position { line: 0, col: 3 });
    }

    #[test]
    fn caps_lines_before_converting_columns() {
        let text = Rope::from_str("first\n🙂");
        let position = Position { line: 99, col: 4 };

        let converted = position_to_encoding(&text, position, Encoding::UTF8, Encoding::UTF16);

        assert_eq!(converted, Position { line: 1, col: 2 });
    }
}
