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

// Tests go here ...
