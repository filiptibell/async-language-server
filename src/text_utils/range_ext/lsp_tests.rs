use async_lsp::lsp_types::{Position as LspPosition, Range as LspRange};

use super::RangeExt;

const T: &str = ""; // LSP range & position do not need text information
const LF: char = '\n';
const D1: char = '/';
const D2: char = '@';

const fn r(start: LspPosition, end: LspPosition) -> LspRange {
    LspRange { start, end }
}

const fn p(line: u32, column: u32) -> LspPosition {
    LspPosition {
        line,
        character: column,
    }
}

// Basic happy path tests

#[test]
fn basic_split_at() {
    let (left, right) = r(p(0, 0), p(0, 10)).split_at(T, p(0, 5));
    assert_eq!(left, r(p(0, 0), p(0, 5)));
    assert_eq!(right, r(p(0, 5), p(0, 10)));
}

#[test]
fn basic_split_off_left() {
    let left = r(p(0, 0), p(0, 10)).split_off_left(T, p(0, 3));
    assert_eq!(left, r(p(0, 0), p(0, 3)));
}

#[test]
fn basic_split_off_right() {
    let right = r(p(0, 0), p(0, 10)).split_off_right(T, p(0, 7));
    assert_eq!(right, r(p(0, 7), p(0, 10)));
}

#[test]
fn basic_shrink() {
    let shrunk = r(p(0, 0), p(0, 5)).shrink(1, 2);
    assert_eq!(shrunk, r(p(0, 1), p(0, 3)));
}

#[test]
fn basic_sub() {
    let sub_range = r(p(0, 0), p(0, 10)).sub(T, p(0, 2), p(0, 8));
    assert_eq!(sub_range, r(p(0, 2), p(0, 8)));
}

#[test]
fn basic_sub_delimited() {
    let (left, right) = r(p(0, 0), p(0, 7)).sub_delimited("one/two", D1);
    assert_eq!(left, Some(r(p(0, 0), p(0, 3))));
    assert_eq!(right, Some(r(p(0, 4), p(0, 7))));
}

#[test]
fn basic_sub_delimited_tri() {
    let (first, second, third) = r(p(0, 0), p(0, 13)).sub_delimited_tri("one/two@three", D1, D2);
    assert_eq!(first, Some(r(p(0, 0), p(0, 3))));
    assert_eq!(second, Some(r(p(0, 4), p(0, 7))));
    assert_eq!(third, Some(r(p(0, 8), p(0, 13))));
}

// Edge case tests

#[test]
fn split_at_boundaries() {
    let (left, right) = r(p(1, 5), p(1, 15)).split_at(T, p(0, 0));
    assert_eq!(left, r(p(1, 5), p(1, 5)));
    assert_eq!(right, r(p(1, 5), p(1, 15)));

    let (left, right) = r(p(1, 5), p(1, 15)).split_at(T, p(0, 10));
    assert_eq!(left, r(p(1, 5), p(1, 15)));
    assert_eq!(right, r(p(1, 15), p(1, 15)));
}

#[test]
fn split_at_multiline() {
    let (left, right) = r(p(0, 0), p(2, 5)).split_at(T, p(1, 3));
    assert_eq!(left, r(p(0, 0), p(1, 3)));
    assert_eq!(right, r(p(1, 3), p(2, 5)));
}

#[test]
fn sub_empty_range() {
    let sub_range = r(p(1, 5), p(1, 15)).sub(T, p(0, 3), p(0, 3));
    assert_eq!(sub_range, r(p(1, 8), p(1, 8)));
}

#[test]
fn sub_multiline() {
    let sub_range = r(p(0, 0), p(2, 10)).sub(T, p(0, 5), p(1, 3));
    assert_eq!(sub_range, r(p(0, 5), p(1, 3)));
}

#[test]
fn sub_delimited_delimiter_at_start() {
    let (left, right) = r(p(0, 0), p(0, 4)).sub_delimited("/abc", D1);
    assert_eq!(left, None);
    assert_eq!(right, Some(r(p(0, 1), p(0, 4))));
}

#[test]
fn sub_delimited_delimiter_at_end() {
    let (left, right) = r(p(0, 0), p(0, 4)).sub_delimited("abc/", D1);
    assert_eq!(left, Some(r(p(0, 0), p(0, 3))));
    assert_eq!(right, None);
}

#[test]
fn sub_delimited_no_delimiter() {
    let (left, right) = r(p(0, 0), p(0, 3)).sub_delimited("abc", D1);
    assert_eq!(left, Some(r(p(0, 0), p(0, 3))));
    assert_eq!(right, None);
}

#[test]
fn sub_delimited_empty_text() {
    let (left, right) = r(p(0, 0), p(0, 0)).sub_delimited(T, D1);
    assert_eq!(left, None);
    assert_eq!(right, None);
}

#[test]
fn sub_delimited_multiline() {
    let (left, right) = r(p(0, 0), p(1, 3)).sub_delimited("abc\ndef", LF);
    assert_eq!(left, Some(r(p(0, 0), p(0, 3))));
    assert_eq!(right, Some(r(p(1, 0), p(1, 3))));
}

#[test]
fn sub_delimited_tri_partial() {
    let (first, second, third) = r(p(0, 0), p(0, 7)).sub_delimited_tri("one/two", D1, D2);
    assert_eq!(first, Some(r(p(0, 0), p(0, 3))));
    assert_eq!(second, Some(r(p(0, 4), p(0, 7))));
    assert_eq!(third, None);
}

#[test]
fn sub_delimited_tri_no_delimiters() {
    let (first, second, third) = r(p(0, 0), p(0, 3)).sub_delimited_tri("abc", D1, D2);
    assert_eq!(first, Some(r(p(0, 0), p(0, 3))));
    assert_eq!(second, None);
    assert_eq!(third, None);
}

#[test]
fn sub_delimited_tri_multiline() {
    let (first, second, third) = r(p(0, 0), p(2, 3)).sub_delimited_tri("one\ntwo\n@@@", LF, D2);
    assert_eq!(first, Some(r(p(0, 0), p(0, 3))));
    assert_eq!(second, Some(r(p(1, 0), p(2, 0))));
    assert_eq!(third, Some(r(p(2, 1), p(2, 3))));
}
