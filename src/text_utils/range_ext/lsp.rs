use async_lsp::lsp_types::{Position as LspPosition, Range as LspRange};

impl super::RangeExt for LspRange {
    type Position = LspPosition;

    fn split_at(self, _text: &str, at: Self::Position) -> (Self, Self) {
        let at_absolute = LspPosition {
            line: self.start.line + at.line,
            character: if at.line == 0 {
                self.start.character + at.character
            } else {
                at.character
            },
        };

        assert!(at_absolute >= self.start && at_absolute <= self.end);

        let left = LspRange {
            start: self.start,
            end: at_absolute,
        };
        let right = LspRange {
            start: at_absolute,
            end: self.end,
        };

        (left, right)
    }

    fn sub(self, _text: &str, from: Self::Position, to: Self::Position) -> Self {
        assert!(from <= to);

        let from_absolute = LspPosition {
            line: self.start.line + from.line,
            character: if from.line == 0 {
                self.start.character + from.character
            } else {
                from.character
            },
        };

        let to_absolute = LspPosition {
            line: self.start.line + to.line,
            character: if to.line == 0 {
                self.start.character + to.character
            } else {
                to.character
            },
        };

        // sanity check
        assert!(from_absolute >= self.start && from_absolute <= self.end);
        assert!(to_absolute >= self.start && to_absolute <= self.end);

        LspRange {
            start: from_absolute,
            end: to_absolute,
        }
    }

    fn sub_delimited(self, text: &str, delim: char) -> (Option<Self>, Option<Self>) {
        assert_eq!(
            delim.len_utf8(),
            1,
            "delim must be a single-byte UTF8 character"
        );

        if text.is_empty() {
            return (None, None);
        }

        if let Some(offset) = text.find(delim) {
            // Find relative position of delimiter from start
            let mut line_num = 0u32;
            let mut line_char = 0;
            for (i, ch) in text.char_indices() {
                if i >= offset {
                    break;
                }
                if ch == '\n' {
                    line_num += 1;
                    line_char = i + 1;
                }
            }

            #[allow(clippy::cast_possible_truncation)]
            let character = text[line_char..offset].chars().count() as u32;
            let delim_pos = LspPosition {
                line: line_num,
                character,
            };

            let left = if offset == 0 {
                None // delimiter is the first character
            } else {
                Some(self.split_off_left(text, delim_pos))
            };

            let right = if offset + 1 >= text.len() {
                None // delimiter is the last character
            } else {
                let after_delim_pos = if text[offset..].starts_with('\n') {
                    LspPosition {
                        line: line_num + 1,
                        character: 0,
                    }
                } else {
                    LspPosition {
                        line: line_num,
                        character: character + 1,
                    }
                };
                Some(self.split_off_right(text, after_delim_pos))
            };

            (left, right)
        } else {
            (Some(self), None)
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

        let (first, remainder) = self.sub_delimited(text, delim0);

        if let Some(remainder) = remainder {
            // Extract the text corresponding to the remainder range
            let delim0_offset = text.find(delim0).expect("delim0 was found");
            let remainder_start = delim0_offset + 1;
            let remainder_text = &text[remainder_start..];

            // Split the remainder on the second delimiter
            let (second, third) = remainder.sub_delimited(remainder_text, delim1);
            (first, second, third)
        } else {
            (first, None, None)
        }
    }
}
