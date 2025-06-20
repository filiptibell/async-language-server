use std::any::type_name;

mod bytes;
mod lsp;

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
