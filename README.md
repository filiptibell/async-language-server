# `async-language-server`

A higher-level abstraction on top of [async-lsp] with good defaults
and a streamlined integration with [tokio] for stdio / tcp transports.

Designed specifically for making language servers with less boilerplate and handling of text documents, while
still being as efficient as possible - using the excellent [ropey] library for fast incremental document updates.

## Additional Features

Just like how incremental text content updates are handled automatically when using this crate,
an optional `tree-sitter` cargo feature is also provided, and uses the same, fast incremental
updates to update arbitrary [tree-sitter] syntax trees when text document contents are updated.

With the `tree-sitter` cargo feature enabled, each document may be associated with its own parser,
allowing a language-per-document architecture for language servers that work with multiple languages.

## Stability Guarantees

This crate is a personal project of mine, to make small language servers that I want to have, easier to write.
It is not generally intended for public consumption, and **will not be published to `crates.io`**.

It is however generally stable, so feel free to use it at your own risk, either by:

- Specifying it as a git dependency
- Forking this repository

[async-lsp](https://crates.io/crates/async-lsp)
[tokio](https://tokio.rs)
[ropey](https://crates.io/crates/ropey)
[tree-sitter](https://tree-sitter.github.io/tree-sitter/)
