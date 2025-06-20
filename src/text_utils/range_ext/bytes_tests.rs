use super::RangeExt;

type ByteRange = std::ops::Range<usize>;
type BytePosition = usize;

const fn range(start: BytePosition, end: BytePosition) -> ByteRange {
    start..end
}

// Basic happy path tests

#[test]
fn basic_split_at() {
    let (left, right) = range(0, 10).split_at("", 5);
    assert_eq!(left, range(0, 5));
    assert_eq!(right, range(5, 10));
}

#[test]
fn basic_split_off_left() {
    let left = range(0, 10).split_off_left("", 3);
    assert_eq!(left, range(0, 3));
}

#[test]
fn basic_split_off_right() {
    let right = range(0, 10).split_off_right("", 7);
    assert_eq!(right, range(7, 10));
}

#[test]
fn basic_sub() {
    let sub_range = range(0, 10).sub("", 2, 8);
    assert_eq!(sub_range, range(2, 8));
}

#[test]
fn basic_sub_delimited() {
    let (left, right) = range(0, 7).sub_delimited("one/two", '/');
    assert_eq!(left, Some(range(0, 3)));
    assert_eq!(right, Some(range(4, 7)));
}

#[test]
fn basic_sub_delimited_tri() {
    let (first, second, third) = range(0, 13).sub_delimited_tri("one/two@three", '/', '@');
    assert_eq!(first, Some(range(0, 3)));
    assert_eq!(second, Some(range(4, 7)));
    assert_eq!(third, Some(range(8, 13)));
}

// Edge case tests

#[test]
fn split_at_boundaries() {
    let (left, right) = range(5, 15).split_at("", 0);
    assert_eq!(left, range(5, 5));
    assert_eq!(right, range(5, 15));

    let (left, right) = range(5, 15).split_at("", 10);
    assert_eq!(left, range(5, 15));
    assert_eq!(right, range(15, 15));
}

#[test]
fn sub_empty_range() {
    let sub_range = range(5, 15).sub("", 3, 3);
    assert_eq!(sub_range, range(8, 8));
}

#[test]
fn sub_delimited_delimiter_at_start() {
    let (left, right) = range(0, 4).sub_delimited("/abc", '/');
    assert_eq!(left, None);
    assert_eq!(right, Some(range(1, 4)));
}

#[test]
fn sub_delimited_delimiter_at_end() {
    let (left, right) = range(0, 4).sub_delimited("abc/", '/');
    assert_eq!(left, Some(range(0, 3)));
    assert_eq!(right, None);
}

#[test]
fn sub_delimited_no_delimiter() {
    let (left, right) = range(0, 3).sub_delimited("abc", '/');
    assert_eq!(left, Some(range(0, 3)));
    assert_eq!(right, None);
}

#[test]
fn sub_delimited_empty_text() {
    let (left, right) = range(0, 0).sub_delimited("", '/');
    assert_eq!(left, None);
    assert_eq!(right, None);
}

#[test]
fn sub_delimited_tri_partial() {
    let (first, second, third) = range(0, 7).sub_delimited_tri("one/two", '/', '@');
    assert_eq!(first, Some(range(0, 3)));
    assert_eq!(second, Some(range(4, 7)));
    assert_eq!(third, None);
}

#[test]
fn sub_delimited_tri_no_delimiters() {
    let (first, second, third) = range(0, 3).sub_delimited_tri("abc", '/', '@');
    assert_eq!(first, Some(range(0, 3)));
    assert_eq!(second, None);
    assert_eq!(third, None);
}
