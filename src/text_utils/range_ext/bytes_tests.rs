use super::RangeExt;

type ByteRange = std::ops::Range<usize>;
type BytePosition = usize;

const T: &str = ""; // Byte range & position do not need text information
const D1: char = '/';
const D2: char = '@';

const fn r(start: BytePosition, end: BytePosition) -> ByteRange {
    start..end
}

// Basic happy path tests

#[test]
fn basic_split_at() {
    let (left, right) = r(0, 10).split_at(T, 5);
    assert_eq!(left, r(0, 5));
    assert_eq!(right, r(5, 10));
}

#[test]
fn basic_split_off_left() {
    let left = r(0, 10).split_off_left(T, 3);
    assert_eq!(left, r(0, 3));
}

#[test]
fn basic_split_off_right() {
    let right = r(0, 10).split_off_right(T, 7);
    assert_eq!(right, r(7, 10));
}

#[test]
fn basic_shrink() {
    let shrunk = r(0, 10).shrink(T, 2, 3);
    assert_eq!(shrunk, r(2, 7));
}

#[test]
fn basic_sub() {
    let sub_range = r(0, 10).sub(T, 2, 8);
    assert_eq!(sub_range, r(2, 8));
}

#[test]
fn basic_sub_delimited() {
    let (left, right) = r(0, 7).sub_delimited("one/two", D1);
    assert_eq!(left, Some(r(0, 3)));
    assert_eq!(right, Some(r(4, 7)));
}

#[test]
fn basic_sub_delimited_tri() {
    let (first, second, third) = r(0, 13).sub_delimited_tri("one/two@three", D1, D2);
    assert_eq!(first, Some(r(0, 3)));
    assert_eq!(second, Some(r(4, 7)));
    assert_eq!(third, Some(r(8, 13)));
}

// Edge case tests

#[test]
fn split_at_boundaries() {
    let (left, right) = r(5, 15).split_at(T, 0);
    assert_eq!(left, r(5, 5));
    assert_eq!(right, r(5, 15));

    let (left, right) = r(5, 15).split_at(T, 10);
    assert_eq!(left, r(5, 15));
    assert_eq!(right, r(15, 15));
}

#[test]
fn sub_empty_range() {
    let sub_range = r(5, 15).sub(T, 3, 3);
    assert_eq!(sub_range, r(8, 8));
}

#[test]
fn sub_delimited_delimiter_at_start() {
    let (left, right) = r(0, 4).sub_delimited("/abc", D1);
    assert_eq!(left, None);
    assert_eq!(right, Some(r(1, 4)));
}

#[test]
fn sub_delimited_delimiter_at_end() {
    let (left, right) = r(0, 4).sub_delimited("abc/", D1);
    assert_eq!(left, Some(r(0, 3)));
    assert_eq!(right, None);
}

#[test]
fn sub_delimited_no_delimiter() {
    let (left, right) = r(0, 3).sub_delimited("abc", D1);
    assert_eq!(left, Some(r(0, 3)));
    assert_eq!(right, None);
}

#[test]
fn sub_delimited_empty_text() {
    let (left, right) = r(0, 0).sub_delimited(T, D1);
    assert_eq!(left, None);
    assert_eq!(right, None);
}

#[test]
fn sub_delimited_tri_partial() {
    let (first, second, third) = r(0, 7).sub_delimited_tri("one/two", D1, D2);
    assert_eq!(first, Some(r(0, 3)));
    assert_eq!(second, Some(r(4, 7)));
    assert_eq!(third, None);
}

#[test]
fn sub_delimited_tri_no_delimiters() {
    let (first, second, third) = r(0, 3).sub_delimited_tri("abc", D1, D2);
    assert_eq!(first, Some(r(0, 3)));
    assert_eq!(second, None);
    assert_eq!(third, None);
}
