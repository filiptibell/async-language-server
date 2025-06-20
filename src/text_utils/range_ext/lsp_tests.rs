use async_lsp::lsp_types::{Position as LspPosition, Range as LspRange};

use super::RangeExt;

const fn range(start: LspPosition, end: LspPosition) -> LspRange {
    LspRange { start, end }
}

const fn pos(line: u32, column: u32) -> LspPosition {
    LspPosition {
        line,
        character: column,
    }
}

// Basic happy path tests

#[test]
fn basic_split_at() {
    let (left, right) = range(pos(0, 0), pos(0, 10)).split_at("", pos(0, 5));
    assert_eq!(left, range(pos(0, 0), pos(0, 5)));
    assert_eq!(right, range(pos(0, 5), pos(0, 10)));
}

#[test]
fn basic_split_off_left() {
    let left = range(pos(0, 0), pos(0, 10)).split_off_left("", pos(0, 3));
    assert_eq!(left, range(pos(0, 0), pos(0, 3)));
}

#[test]
fn basic_split_off_right() {
    let right = range(pos(0, 0), pos(0, 10)).split_off_right("", pos(0, 7));
    assert_eq!(right, range(pos(0, 7), pos(0, 10)));
}

#[test]
fn basic_sub() {
    let sub_range = range(pos(0, 0), pos(0, 10)).sub("", pos(0, 2), pos(0, 8));
    assert_eq!(sub_range, range(pos(0, 2), pos(0, 8)));
}

#[test]
fn basic_sub_delimited() {
    let (left, right) = range(pos(0, 0), pos(0, 7)).sub_delimited("one/two", '/');
    assert_eq!(left, Some(range(pos(0, 0), pos(0, 3))));
    assert_eq!(right, Some(range(pos(0, 4), pos(0, 7))));
}

#[test]
fn basic_sub_delimited_tri() {
    let (first, second, third) =
        range(pos(0, 0), pos(0, 13)).sub_delimited_tri("one/two@three", '/', '@');
    assert_eq!(first, Some(range(pos(0, 0), pos(0, 3))));
    assert_eq!(second, Some(range(pos(0, 4), pos(0, 7))));
    assert_eq!(third, Some(range(pos(0, 8), pos(0, 13))));
}

// Edge case tests

#[test]
fn split_at_boundaries() {
    let (left, right) = range(pos(1, 5), pos(1, 15)).split_at("", pos(0, 0));
    assert_eq!(left, range(pos(1, 5), pos(1, 5)));
    assert_eq!(right, range(pos(1, 5), pos(1, 15)));

    let (left, right) = range(pos(1, 5), pos(1, 15)).split_at("", pos(0, 10));
    assert_eq!(left, range(pos(1, 5), pos(1, 15)));
    assert_eq!(right, range(pos(1, 15), pos(1, 15)));
}

#[test]
fn split_at_multiline() {
    let (left, right) = range(pos(0, 0), pos(2, 5)).split_at("", pos(1, 3));
    assert_eq!(left, range(pos(0, 0), pos(1, 3)));
    assert_eq!(right, range(pos(1, 3), pos(2, 5)));
}

#[test]
fn sub_empty_range() {
    let sub_range = range(pos(1, 5), pos(1, 15)).sub("", pos(0, 3), pos(0, 3));
    assert_eq!(sub_range, range(pos(1, 8), pos(1, 8)));
}

#[test]
fn sub_multiline() {
    let sub_range = range(pos(0, 0), pos(2, 10)).sub("", pos(0, 5), pos(1, 3));
    assert_eq!(sub_range, range(pos(0, 5), pos(1, 3)));
}

#[test]
fn sub_delimited_delimiter_at_start() {
    let (left, right) = range(pos(0, 0), pos(0, 4)).sub_delimited("/abc", '/');
    assert_eq!(left, None);
    assert_eq!(right, Some(range(pos(0, 1), pos(0, 4))));
}

#[test]
fn sub_delimited_delimiter_at_end() {
    let (left, right) = range(pos(0, 0), pos(0, 4)).sub_delimited("abc/", '/');
    assert_eq!(left, Some(range(pos(0, 0), pos(0, 3))));
    assert_eq!(right, None);
}

#[test]
fn sub_delimited_no_delimiter() {
    let (left, right) = range(pos(0, 0), pos(0, 3)).sub_delimited("abc", '/');
    assert_eq!(left, Some(range(pos(0, 0), pos(0, 3))));
    assert_eq!(right, None);
}

#[test]
fn sub_delimited_empty_text() {
    let (left, right) = range(pos(0, 0), pos(0, 0)).sub_delimited("", '/');
    assert_eq!(left, None);
    assert_eq!(right, None);
}

#[test]
fn sub_delimited_multiline() {
    let (left, right) = range(pos(0, 0), pos(1, 3)).sub_delimited("abc\ndef", '\n');
    assert_eq!(left, Some(range(pos(0, 0), pos(0, 3))));
    assert_eq!(right, Some(range(pos(1, 0), pos(1, 3))));
}

#[test]
fn sub_delimited_tri_partial() {
    let (first, second, third) = range(pos(0, 0), pos(0, 7)).sub_delimited_tri("one/two", '/', '@');
    assert_eq!(first, Some(range(pos(0, 0), pos(0, 3))));
    assert_eq!(second, Some(range(pos(0, 4), pos(0, 7))));
    assert_eq!(third, None);
}

#[test]
fn sub_delimited_tri_no_delimiters() {
    let (first, second, third) = range(pos(0, 0), pos(0, 3)).sub_delimited_tri("abc", '/', '@');
    assert_eq!(first, Some(range(pos(0, 0), pos(0, 3))));
    assert_eq!(second, None);
    assert_eq!(third, None);
}

#[test]
fn sub_delimited_tri_multiline() {
    let (first, second, third) =
        range(pos(0, 0), pos(2, 3)).sub_delimited_tri("one\ntwo\n@@@", '\n', '@');
    assert_eq!(first, Some(range(pos(0, 0), pos(0, 3))));
    assert_eq!(second, Some(range(pos(1, 0), pos(2, 0))));
    assert_eq!(third, Some(range(pos(2, 1), pos(2, 3))));
}
