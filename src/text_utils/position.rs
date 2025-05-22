use async_lsp::lsp_types::Position as LspPosition;

/**
    A position including a line and column.

    May be cheaply copied, as well as converted
    to / from language server positions.
*/
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Position {
    pub line: usize,
    pub col: usize,
}

impl Position {
    #[must_use]
    pub const fn from_lsp(position: LspPosition) -> Self {
        Self {
            line: position.line as usize,
            col: position.character as usize,
        }
    }

    #[must_use]
    pub const fn into_lsp(self) -> LspPosition {
        #[allow(clippy::cast_possible_truncation)]
        LspPosition {
            line: self.line as u32,
            character: self.col as u32,
        }
    }
}

impl From<&Position> for Position {
    fn from(position: &Position) -> Self {
        *position
    }
}

impl From<&LspPosition> for Position {
    fn from(position: &LspPosition) -> Self {
        Self::from_lsp(*position)
    }
}

impl From<LspPosition> for Position {
    fn from(position: LspPosition) -> Self {
        Self::from_lsp(position)
    }
}

impl From<&Position> for LspPosition {
    fn from(position: &Position) -> Self {
        position.into_lsp()
    }
}

impl From<Position> for LspPosition {
    fn from(position: Position) -> Self {
        position.into_lsp()
    }
}

#[cfg(feature = "tree-sitter")]
use tree_sitter::Point as TsPoint;

#[cfg(feature = "tree-sitter")]
impl Position {
    #[must_use]
    pub const fn from_ts(point: TsPoint) -> Self {
        Self {
            line: point.row,
            col: point.column,
        }
    }

    #[must_use]
    pub const fn into_ts(self) -> TsPoint {
        TsPoint {
            row: self.line,
            column: self.col,
        }
    }
}

#[cfg(feature = "tree-sitter")]
impl From<TsPoint> for Position {
    fn from(point: TsPoint) -> Self {
        Self::from_ts(point)
    }
}

#[cfg(feature = "tree-sitter")]
impl From<&TsPoint> for Position {
    fn from(point: &TsPoint) -> Self {
        Self::from_ts(*point)
    }
}

#[cfg(feature = "tree-sitter")]
impl From<Position> for TsPoint {
    fn from(position: Position) -> Self {
        position.into_ts()
    }
}

#[cfg(feature = "tree-sitter")]
impl From<&Position> for TsPoint {
    fn from(position: &Position) -> Self {
        position.into_ts()
    }
}
