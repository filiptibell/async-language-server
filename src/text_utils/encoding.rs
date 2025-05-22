use async_lsp::lsp_types::PositionEncodingKind as LspPositionEncoding;

/**
    A position encoding supported by this library.

    Easy to copy and match against, unlike `PositionEncodingKind`, and
    contains several similar utilities, that are additionally `const`.
*/
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Encoding {
    /**
        Character offsets count UTF-8 code units.
    */
    UTF8,
    /**
        Character offsets count UTF-16 code units.

        This is the default for the Language Server Protocol - if a client
        does not specify which position encoding they prefer and / or support,
        this encoding must always be used.
    */
    UTF16,
    /**
        Character offsets count UTF-32 code units.

        This encoding is equivalent to Unicode code points, so it may also
        be used for an encoding-agnostic representation of character offsets.
    */
    UTF32,
}

impl Encoding {
    #[must_use]
    pub const fn default() -> Self {
        Self::UTF16
    }

    #[must_use]
    pub const fn into_lsp(self) -> LspPositionEncoding {
        match self {
            Self::UTF8 => LspPositionEncoding::UTF8,
            Self::UTF16 => LspPositionEncoding::UTF16,
            Self::UTF32 => LspPositionEncoding::UTF32,
        }
    }

    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::UTF8 => "utf-8",
            Self::UTF16 => "utf-16",
            Self::UTF32 => "utf-32",
        }
    }

    #[allow(clippy::missing_panics_doc)]
    #[must_use]
    pub fn from_lsp(encoding: &LspPositionEncoding) -> Self {
        if encoding == &LspPositionEncoding::UTF8 {
            Self::UTF8
        } else if encoding == &LspPositionEncoding::UTF16 {
            Self::UTF16
        } else if encoding == &LspPositionEncoding::UTF32 {
            Self::UTF32
        } else {
            panic!("unsupported position encoding kind: {encoding:?}")
        }
    }
}

impl From<&Encoding> for Encoding {
    fn from(encoding: &Encoding) -> Self {
        *encoding
    }
}

impl From<&LspPositionEncoding> for Encoding {
    fn from(encoding: &LspPositionEncoding) -> Self {
        Self::from_lsp(encoding)
    }
}

impl From<LspPositionEncoding> for Encoding {
    fn from(value: LspPositionEncoding) -> Self {
        Self::from_lsp(&value)
    }
}

impl From<&Encoding> for LspPositionEncoding {
    fn from(value: &Encoding) -> Self {
        value.into_lsp()
    }
}

impl From<Encoding> for LspPositionEncoding {
    fn from(value: Encoding) -> Self {
        value.into_lsp()
    }
}

impl Default for Encoding {
    fn default() -> Self {
        Self::default()
    }
}
