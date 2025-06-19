use std::{any::type_name, ops};

use async_lsp::lsp_types::{Position as LspPosition, Range as LspRange};

/**
    Extension trait for different kinds of ranges:

    1. Byte ranges
    2. LSP protocol ranges
    3. Tree-sitter ranges

    Provides methods for splitting ranges into parts and creating
    subranges based on positions and/or string delimiters.
*/
pub trait RangeExt: Sized {
    type Position;

    /**
        Splits the given range into two parts at the specified position.

        Note that the `at` position is _relative_ to the start of the range.
    */
    #[must_use]
    fn split_at(self, at: Self::Position) -> (Self, Self);

    /**
        Splits the given range into two parts at the specified position,
        and returns the left part.

        Note that the `at` position is _relative_ to the start of the range.
    */
    #[must_use]
    fn split_off_left(self, at: Self::Position) -> Self {
        let (left, _) = self.split_at(at);
        left
    }

    /**
        Splits the given range into two parts at the specified position,
        and returns the right part.

        Note that the `at` position is _relative_ to the start of the range.
    */
    #[must_use]
    fn split_off_right(self, at: Self::Position) -> Self {
        let (_, right) = self.split_at(at);
        right
    }

    /**
        Returns a subrange of the range, starting at `from` and ending at `to`.

        Note that both positions are _relative_ to the start of the range,
        and that the range itself should be an absolute range.
    */
    #[must_use]
    fn sub(self, from: Self::Position, to: Self::Position) -> Self;

    /**
        Splits the given range into two optional subranges, using the given delimiter.

        The range should be the exact range for the given text.

        # Example Usage

        ```rust no_run
        const D: char = '/';

        (0..7).sub_delimited("one/two", D);
        // --> (Some(0..3), Some(4..7))

        (0..4).sub_delimited("/two", D);
        // --> (None, Some(1..4))

        (0..4).sub_delimited("one/", D);
        // --> (Some(0..3), None)

        (0..3).sub_delimited("one", D);
        // --> (Some(0..3), None)

        (0..0).sub_delimited("", D);
        // --> (None, None)
        ```

        # Panics

        - Panics if the text and range are not the exact same length.
        - Panics if the delimiter is not a single-byte UTF8 character.
    */
    #[allow(unused_variables)]
    #[must_use]
    fn sub_delimited(self, text: &str, delimiter: char) -> (Option<Self>, Option<Self>) {
        unimplemented!(
            "sub_delimited is not implemented for {}",
            type_name::<Self>()
        )
    }

    /**
        Splits the given range into _three_ optional subranges,
        using the two given delimiters, consecutively.

        The range should be the exact range corresponding to the given text.

        # Example Usage

        ```rust no_run
        const D0: char = '/';
        const D1: char = '@';

        (0..13).sub_delimited_tri("one/two@three", D0, D1);
        // --> (Some(0..3), Some(4..7), Some(8..13))

        (0..7).sub_delimited_tri("one/two", D0, D1);
        // --> (Some(0..3), Some(4..7), None)

        (0..3).sub_delimited_tri("one", D0, D1);
        // --> (Some(0..3), None, None)

        (0..0).sub_delimited_tri("", D0, D1);
        // --> (None, None, None)
        ```

        # Panics

        - Panics if the text and range are not the exact same length.
        - Panics if any delimiter is not a single-byte UTF8 character.
    */
    #[allow(unused_variables)]
    #[must_use]
    fn sub_delimited_tri(
        self,
        text: &str,
        delim0: char,
        delim1: char,
    ) -> (Option<Self>, Option<Self>, Option<Self>) {
        unimplemented!(
            "sub_delimited_tri is not implemented for {}",
            type_name::<Self>()
        )
    }
}

// Byte range implementation

impl RangeExt for ops::Range<usize> {
    type Position = usize;

    fn split_at(self, at: Self::Position) -> (Self, Self) {
        assert!(at <= self.end - self.start);
        let left = self.start..(self.start + at);
        let right = (self.start + at)..self.end;
        (left, right)
    }

    fn sub(self, from: Self::Position, to: Self::Position) -> Self {
        assert!(from <= self.end - self.start);
        assert!(to <= self.end - self.start);
        assert!(from <= to);
        (self.start + from)..(self.start + to)
    }

    fn sub_delimited(self, text: &str, delim: char) -> (Option<Self>, Option<Self>) {
        assert_eq!(
            text.len(),
            self.end - self.start,
            "text and range must be the same length"
        );
        assert_eq!(
            delim.len_utf8(),
            1,
            "delim must be a single-byte UTF8 character"
        );

        if let Some(offset) = text.find(delim) {
            (
                if offset == 0 {
                    None // delimiter is the first character
                } else {
                    Some(self.clone().split_off_left(offset))
                },
                if offset + 1 >= text.len() {
                    None // delimiter is the last character
                } else {
                    Some(self.clone().split_off_right(offset + 1))
                },
            )
        } else if !text.is_empty() {
            (Some(self.clone()), None)
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
            self.end - self.start,
            "text and range must be the same length"
        );

        let (first, remainder) = self.clone().sub_delimited(text, delim0);

        if let Some(remainder) = remainder {
            // Extract the text corresponding to the remainder range
            let remainder_start = remainder.start - self.start;
            let remainder_end = remainder.end - self.start;
            let remainder_text = &text[remainder_start..remainder_end];

            // Split the remainder on the second delimiter
            let (second, third) = remainder.sub_delimited(remainder_text, delim1);
            (first, second, third)
        } else {
            (first, None, None)
        }
    }
}

// LSP range implementation

impl RangeExt for LspRange {
    type Position = LspPosition;

    fn split_at(self, at: Self::Position) -> (Self, Self) {
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

    fn sub(self, from: Self::Position, to: Self::Position) -> Self {
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
}
