/*!
 * main nimage library
 * Copyright 2020 Allen Wild
 * SPDX-License-Identifier: GPL-3.0-or-later
 */

// "global" clippy settings have to be set at crate level rather than in a config file [1]
// unreadable_literal is annoying and was demoted to "pedantic" on 2020-04-08 [2]
// [1] https://github.com/rust-lang/cargo/issues/5034
// [2] https://github.com/rust-lang/rust-clippy/pull/5419
#![allow(clippy::unreadable_literal)]

pub mod crc32;
pub mod errors;
pub mod format;
pub mod util;
pub mod xxhio;
