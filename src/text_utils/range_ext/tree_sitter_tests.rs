use tree_sitter::{Point as TsPosition, Range as TsRange};

use super::RangeExt;

const LF: char = '\n';
const D1: char = '/';
const D2: char = '@';

const fn r(
    start_byte: usize,
    start_position: TsPosition,
    end_byte: usize,
    end_position: TsPosition,
) -> TsRange {
    TsRange {
        start_byte,
        start_point: start_position,
        end_byte,
        end_point: end_position,
    }
}

const fn p(line: usize, column: usize) -> TsPosition {
    TsPosition { row: line, column }
}

// Basic happy path tests

#[test]
fn basic_split_at() {
    let text = "hello";
    let (left, right) = r(0, p(0, 0), 5, p(0, 5)).split_at(text, p(0, 2));
    assert_eq!(left, r(0, p(0, 0), 2, p(0, 2)));
    assert_eq!(right, r(2, p(0, 2), 5, p(0, 5)));
}

#[test]
fn basic_split_off_left() {
    let text = "hello";
    let left = r(0, p(0, 0), 5, p(0, 5)).split_off_left(text, p(0, 3));
    assert_eq!(left, r(0, p(0, 0), 3, p(0, 3)));
}

#[test]
fn basic_split_off_right() {
    let text = "hello";
    let right = r(0, p(0, 0), 5, p(0, 5)).split_off_right(text, p(0, 2));
    assert_eq!(right, r(2, p(0, 2), 5, p(0, 5)));
}

#[test]
fn basic_sub() {
    let text = "hello";
    let sub_range = r(0, p(0, 0), 5, p(0, 5)).sub(text, p(0, 1), p(0, 4));
    assert_eq!(sub_range, r(1, p(0, 1), 4, p(0, 4)));
}

#[test]
fn basic_sub_delimited() {
    let text = "one/two";
    let (left, right) = r(0, p(0, 0), 7, p(0, 7)).sub_delimited(text, D1);
    assert_eq!(left, Some(r(0, p(0, 0), 3, p(0, 3))));
    assert_eq!(right, Some(r(4, p(0, 4), 7, p(0, 7))));
}

#[test]
fn basic_sub_delimited_tri() {
    let text = "one/two@three";
    let (first, second, third) = r(0, p(0, 0), 13, p(0, 13)).sub_delimited_tri(text, D1, D2);
    assert_eq!(first, Some(r(0, p(0, 0), 3, p(0, 3))));
    assert_eq!(second, Some(r(4, p(0, 4), 7, p(0, 7))));
    assert_eq!(third, Some(r(8, p(0, 8), 13, p(0, 13))));
}

// Edge case tests

#[test]
fn split_at_boundaries() {
    let text = "hello";

    let (left, right) = r(0, p(0, 0), 5, p(0, 5)).split_at(text, p(0, 0));
    assert_eq!(left, r(0, p(0, 0), 0, p(0, 0)));
    assert_eq!(right, r(0, p(0, 0), 5, p(0, 5)));

    let (left, right) = r(0, p(0, 0), 5, p(0, 5)).split_at(text, p(0, 5));
    assert_eq!(left, r(0, p(0, 0), 5, p(0, 5)));
    assert_eq!(right, r(5, p(0, 5), 5, p(0, 5)));
}

#[test]
fn split_at_multiline() {
    let text = "one\ntwo";
    let (left, right) = r(0, p(0, 0), 7, p(1, 3)).split_at(text, p(1, 1));
    assert_eq!(left, r(0, p(0, 0), 5, p(1, 1)));
    assert_eq!(right, r(5, p(1, 1), 7, p(1, 3)));
}

#[test]
fn sub_empty_range() {
    let text = "hello";
    let sub_range = r(10, p(1, 5), 15, p(1, 10)).sub(text, p(0, 2), p(0, 2));
    assert_eq!(sub_range, r(12, p(1, 7), 12, p(1, 7)));
}

#[test]
fn sub_multiline() {
    let text = "one\ntwo\nthree";
    let sub_range = r(0, p(0, 0), 13, p(2, 5)).sub(text, p(0, 2), p(1, 1));
    assert_eq!(sub_range, r(2, p(0, 2), 5, p(1, 1)));
}

#[test]
fn sub_delimited_delimiter_at_start() {
    let text = "/abc";
    let (left, right) = r(0, p(0, 0), 4, p(0, 4)).sub_delimited(text, D1);
    assert_eq!(left, None);
    assert_eq!(right, Some(r(1, p(0, 1), 4, p(0, 4))));
}

#[test]
fn sub_delimited_delimiter_at_end() {
    let text = "abc/";
    let (left, right) = r(0, p(0, 0), 4, p(0, 4)).sub_delimited(text, D1);
    assert_eq!(left, Some(r(0, p(0, 0), 3, p(0, 3))));
    assert_eq!(right, None);
}

#[test]
fn sub_delimited_no_delimiter() {
    let text = "abc";
    let (left, right) = r(0, p(0, 0), 3, p(0, 3)).sub_delimited(text, D1);
    assert_eq!(left, Some(r(0, p(0, 0), 3, p(0, 3))));
    assert_eq!(right, None);
}

#[test]
fn sub_delimited_empty_text() {
    let text = "";
    let (left, right) = r(0, p(0, 0), 0, p(0, 0)).sub_delimited(text, D1);
    assert_eq!(left, None);
    assert_eq!(right, None);
}

#[test]
fn sub_delimited_multiline() {
    let text = "abc\ndef";
    let (left, right) = r(0, p(0, 0), 7, p(1, 3)).sub_delimited(text, LF);
    assert_eq!(left, Some(r(0, p(0, 0), 3, p(0, 3))));
    assert_eq!(right, Some(r(4, p(1, 0), 7, p(1, 3))));
}

#[test]
fn sub_delimited_tri_partial() {
    let text = "one/two";
    let (first, second, third) = r(0, p(0, 0), 7, p(0, 7)).sub_delimited_tri(text, D1, D2);
    assert_eq!(first, Some(r(0, p(0, 0), 3, p(0, 3))));
    assert_eq!(second, Some(r(4, p(0, 4), 7, p(0, 7))));
    assert_eq!(third, None);
}

#[test]
fn sub_delimited_tri_no_delimiters() {
    let text = "abc";
    let (first, second, third) = r(0, p(0, 0), 3, p(0, 3)).sub_delimited_tri(text, D1, D2);
    assert_eq!(first, Some(r(0, p(0, 0), 3, p(0, 3))));
    assert_eq!(second, None);
    assert_eq!(third, None);
}

#[test]
fn sub_delimited_tri_multiline() {
    let text = "one\ntwo\n@@@";
    let (first, second, third) = r(0, p(0, 0), 11, p(2, 3)).sub_delimited_tri(text, LF, D2);
    assert_eq!(first, Some(r(0, p(0, 0), 3, p(0, 3))));
    assert_eq!(second, Some(r(4, p(1, 0), 8, p(2, 0))));
    assert_eq!(third, Some(r(9, p(2, 1), 11, p(2, 3))));
}

// Tree-sitter specific multiline tests

#[test]
fn split_at_newline_boundary() {
    let text = "line1\nline2";
    let (left, right) = r(0, p(0, 0), 11, p(1, 5)).split_at(text, p(1, 0));
    assert_eq!(left, r(0, p(0, 0), 6, p(1, 0)));
    assert_eq!(right, r(6, p(1, 0), 11, p(1, 5)));
}

#[test]
fn sub_across_multiple_lines() {
    let text = "line1\nline2\nline3";
    let sub_range = r(0, p(0, 0), 17, p(2, 5)).sub(text, p(0, 3), p(2, 2));
    assert_eq!(sub_range, r(3, p(0, 3), 14, p(2, 2)));
}

#[test]
fn sub_delimited_complex_multiline() {
    let text = "start\nfirst/second\nend";
    let (left, right) = r(0, p(0, 0), 22, p(2, 3)).sub_delimited(text, D1);
    assert_eq!(left, Some(r(0, p(0, 0), 11, p(1, 5))));
    assert_eq!(right, Some(r(12, p(1, 6), 22, p(2, 3))));
}
