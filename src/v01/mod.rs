//! Tigr v0.1 — preserved as-is for reference.
//!
//! This module is intentionally NOT wired into the build for v0.2.
//! The source files remain in the tree (lexer.rs, syntax.rs, ast.rs,
//! parser/mod.lalrpop, interpreter/mod.rs) as a semantic reference for
//! features that survive into v0.2.
//!
//! To re-enable for differential testing: add `lalrpop` + `lalrpop-util`
//! deps back to Cargo.toml, restore build.rs, declare `pub mod v01;` in
//! main.rs, and add the inner `pub mod ast; pub mod lexer; ...` lines
//! here.
