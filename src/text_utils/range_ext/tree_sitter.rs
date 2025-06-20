use tree_sitter::{Point as TsPosition, Range as TsRange};

impl super::RangeExt for TsRange {
    type Position = TsPosition;

    fn split_at(self, text: &str, at: Self::Position) -> (Self, Self) {
        assert_eq!(
            text.len(),
            self.end_byte - self.start_byte,
            "text and range must be the same length"
        );

        let at_absolute = TsPosition {
            row: self.start_point.row + at.row,
            column: if at.row == 0 {
                self.start_point.column + at.column
            } else {
                at.column
            },
        };

        // Find byte offset for the relative position
        let mut current_row = 0;
        let mut current_col = 0;
        let mut at_byte = self.start_byte;
        let mut found = false;

        for (i, ch) in text.char_indices() {
            if current_row == at.row && current_col == at.column {
                at_byte = self.start_byte + i;
                found = true;
                break;
            }
            if ch == '\n' {
                current_row += 1;
                current_col = 0;
            } else {
                current_col += ch.len_utf8();
            }
        }

        // Handle end-of-text case if position wasn't found in loop
        if !found && current_row == at.row && current_col == at.column {
            at_byte = self.end_byte;
        }

        let left = TsRange {
            start_byte: self.start_byte,
            end_byte: at_byte,
            start_point: self.start_point,
            end_point: at_absolute,
        };
        let right = TsRange {
            start_byte: at_byte,
            end_byte: self.end_byte,
            start_point: at_absolute,
            end_point: self.end_point,
        };

        (left, right)
    }

    fn sub(self, text: &str, from: Self::Position, to: Self::Position) -> Self {
        assert!(from <= to);

        assert_eq!(
            text.len(),
            self.end_byte - self.start_byte,
            "text and range must be the same length"
        );

        let from_absolute = TsPosition {
            row: self.start_point.row + from.row,
            column: if from.row == 0 {
                self.start_point.column + from.column
            } else {
                from.column
            },
        };

        let to_absolute = TsPosition {
            row: self.start_point.row + to.row,
            column: if to.row == 0 {
                self.start_point.column + to.column
            } else {
                to.column
            },
        };

        // Find byte offsets for both positions
        let mut current_row = 0;
        let mut current_col = 0;
        let mut from_byte = self.start_byte;
        let mut to_byte = self.start_byte;
        let mut found_from = false;

        for (i, ch) in text.char_indices() {
            if !found_from && current_row == from.row && current_col == from.column {
                from_byte = self.start_byte + i;
                found_from = true;
            }
            if current_row == to.row && current_col == to.column {
                to_byte = self.start_byte + i;
                break;
            }
            if ch == '\n' {
                current_row += 1;
                current_col = 0;
            } else {
                current_col += ch.len_utf8();
            }
        }

        TsRange {
            start_byte: from_byte,
            end_byte: to_byte,
            start_point: from_absolute,
            end_point: to_absolute,
        }
    }

    fn sub_delimited(self, text: &str, delim: char) -> (Option<Self>, Option<Self>) {
        assert_eq!(
            text.len(),
            self.end_byte - self.start_byte,
            "text and range must be the same length"
        );
        assert_eq!(
            delim.len_utf8(),
            1,
            "delim must be a single-byte UTF8 character"
        );

        if let Some(offset) = text.find(delim) {
            // Find point position of delimiter
            let mut row_offset = 0;
            let mut current_line_start = 0;

            for (i, ch) in text.char_indices() {
                if i >= offset {
                    break;
                }
                if ch == '\n' {
                    row_offset += 1;
                    current_line_start = i + 1;
                }
            }

            let col_offset = offset - current_line_start;
            let delim_point = TsPosition {
                row: self.start_point.row + row_offset,
                column: if row_offset == 0 {
                    self.start_point.column + col_offset
                } else {
                    col_offset
                },
            };

            let delim_byte = self.start_byte + offset;

            let left = if offset == 0 {
                None // delimiter is the first character
            } else {
                Some(TsRange {
                    start_byte: self.start_byte,
                    end_byte: delim_byte,
                    start_point: self.start_point,
                    end_point: delim_point,
                })
            };

            let right = if offset + 1 >= text.len() {
                None // delimiter is the last character
            } else {
                let after_delim_point = if text[offset..].starts_with('\n') {
                    TsPosition {
                        row: delim_point.row + 1,
                        column: 0,
                    }
                } else {
                    TsPosition {
                        row: delim_point.row,
                        column: delim_point.column + 1,
                    }
                };

                Some(TsRange {
                    start_byte: delim_byte + 1,
                    end_byte: self.end_byte,
                    start_point: after_delim_point,
                    end_point: self.end_point,
                })
            };

            (left, right)
        } else if !text.is_empty() {
            (Some(self), None)
        } else {
            (None, None)
        }
    }

    fn sub_delimited_tri(
        self,
        text: &str,
        delim0: char,
        delim1: char,
    ) -> (Option<Self>, Option<Self>, Option<Self>) {
        assert_eq!(
            delim0.len_utf8(),
            1,
            "delim0 must be a single-byte UTF8 character"
        );
        assert_eq!(
            delim1.len_utf8(),
            1,
            "delim1 must be a single-byte UTF8 character"
        );

        if text.is_empty() {
            return (None, None, None);
        }

        assert_eq!(
            text.len(),
            self.end_byte - self.start_byte,
            "text and range must be the same length"
        );

        let (first, remainder) = self.sub_delimited(text, delim0);

        if let Some(remainder) = remainder {
            let remainder_start = remainder.start_byte - self.start_byte;
            let remainder_text = &text[remainder_start..];

            let (second, third) = remainder.sub_delimited(remainder_text, delim1);
            (first, second, third)
        } else {
            (first, None, None)
        }
    }
}
