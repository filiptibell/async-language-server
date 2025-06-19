type Range = std::ops::Range<usize>;

/**
    Splits the given range into two parts at the specified index.
*/
#[must_use]
pub const fn splitrange(range: Range, at: usize) -> (Range, Range) {
    assert!(at <= range.end - range.start);
    (
        range.start..(range.start + at),
        (range.start + at)..range.end,
    )
}

/**
    Splits the given range into two parts at the specified index,
    and returns the left part.
*/
#[must_use]
pub const fn splitrange_left(range: Range, at: usize) -> Range {
    splitrange(range, at).0
}

/**
    Splits the given range into two parts at the specified index,
    and returns the right part.
*/
#[must_use]
pub const fn splitrange_right(range: Range, at: usize) -> Range {
    splitrange(range, at).1
}

/**
    Returns a subrange of the given range, starting at `from` and ending at `to`.
*/
#[must_use]
pub const fn subrange(range: Range, from: usize, to: usize) -> Range {
    assert!(from <= range.end - range.start);
    assert!(to <= range.end - range.start);
    assert!(from <= to);
    (range.start + from)..(range.start + to)
}

/**
    Splits the given range into two optional subranges, using the given delimiter.

    The range should be the exact range for the given text.

    # Example Usage

    ```rust no_run
    const D: char = '/';

    subrange_delimited("one/two", 0..7, D);
    // --> (Some(0..3), Some(4..7))

    subrange_delimited("/two", 0..4, D);
    // --> (None, Some(1..4))

    subrange_delimited("one/", 0..4, D);
    // --> (Some(0..3), None)

    subrange_delimited("one", 0..3, D);
    // --> (Some(0..3), None)

    subrange_delimited("", 0..0, D);
    // --> (None, None)
    ```

    # Panics

    - Panics if the text and range are not the exact same length.
    - Panics if the delimiter is not a single-byte UTF8 character.
*/
#[must_use]
pub fn subrange_delimited(text: &str, range: Range, delim: char) -> (Option<Range>, Option<Range>) {
    assert_eq!(
        text.len(),
        range.end - range.start,
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
                Some(splitrange_left(range.clone(), offset))
            },
            if offset + 1 >= text.len() {
                None // delimiter is the last character
            } else {
                Some(splitrange_right(range.clone(), offset + 1))
            },
        )
    } else if !text.is_empty() {
        (Some(range.clone()), None)
    } else {
        (None, None)
    }
}

/**
    Splits the given range into _three_ optional subranges,
    using the two given delimiters, consecutively.

    The range should be the exact range corresponding to the given text.

    # Example Usage

    ```rust no_run
    const D0: char = '/';
    const D1: char = '@';

    subrange_delimited_tri("one/two@three", 0..13, D0, D1);
    // --> (Some(0..3), Some(4..7), Some(8..13))

    subrange_delimited_tri("one/two", 0..7, D0, D1);
    // --> (Some(0..3), Some(4..7), None)

    subrange_delimited_tri("one", 0..3, D0, D1);
    // --> (Some(0..3), None, None)

    subrange_delimited_tri("", 0..0, D0, D1);
    // --> (None, None, None)
    ```

    # Panics

    - Panics if the text and range are not the exact same length.
    - Panics if any delimiter is not a single-byte UTF8 character.
*/
#[must_use]
pub fn subrange_delimited_tri(
    text: &str,
    range: Range,
    delim0: char,
    delim1: char,
) -> (Option<Range>, Option<Range>, Option<Range>) {
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
        range.end - range.start,
        "text and range must be the same length"
    );

    let (first, remainder) = subrange_delimited(text, range.clone(), delim0);

    if let Some(remainder) = remainder {
        // Extract the text corresponding to the remainder range
        let remainder_start = remainder.start - range.start;
        let remainder_end = remainder.end - range.start;
        let remainder_text = &text[remainder_start..remainder_end];

        // Split the remainder on the second delimiter
        let (second, third) = subrange_delimited(remainder_text, remainder, delim1);
        (first, second, third)
    } else {
        (first, None, None)
    }
}
