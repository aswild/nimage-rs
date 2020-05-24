/*!
 * Error enums and descriptions for nImage processing errors.
 * Copyright 2020 Allen Wild
 * SPDX-License-Identifier: GPL-3.0-or-later
 */

use std::error::Error;
use std::fmt;

use super::format::*;

/// Errors that may be seen when parsing/validating an nImage header
#[derive(Debug, Eq, PartialEq)]
pub enum ImageValidError {
    BadSize(usize),
    BadMagic(u64),
    UnsupportedVersion(u8),
    NameTooLong(usize),
    TooManyParts(usize),
    InvalidPart { index: usize, err: PartValidError },
    BadCrc { expected: u32, actual: u32 },
}

pub type ImageValidResult<T> = Result<T, ImageValidError>;

impl Error for ImageValidError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidPart { index: _, ref err } => Some(err),
            _ => None,
        }
    }
}

impl fmt::Display for ImageValidError {
    #[rustfmt::skip] // rustfmt mangles this, use manual consistent formatting
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BadSize(size) => {
                write!(f, "bad nImage header size. Expected {}, found {}",
                       NIMG_HDR_SIZE, size)
            }
            Self::BadMagic(magic) => {
                write!(f, "bad nImage magic. Expected 0x{:016x}, found 0x{:016x}",
                       NIMG_HDR_MAGIC, magic)
            }
            Self::UnsupportedVersion(ver) => {
                write!(f, "unsupported version {}", ver)
            }
            Self::NameTooLong(n) => {
                write!(f, "name length {} exceeds maximum of {}", n, NIMG_NAME_LEN)
            }
            Self::TooManyParts(n) => {
                write!(f, "part count {} exceeds maximum of {}", n, NIMG_MAX_PARTS)
            }
            Self::InvalidPart { index, err } => {
                write!(f, "invalid part header at index {}: {}", index, err)
            }
            Self::BadCrc { expected, actual } => {
                write!(f, "invalid image header CRC. Expected 0x{:08x}, found 0x{:08x}",
                       expected, actual)
            }
        }
    }
}

/// Errors that may be seen when parsing/validating an nImage part header
#[derive(Debug, Eq, PartialEq)]
pub enum PartValidError {
    BadSize(usize),
    BadMagic(u64),
    BadType(u8),
    BadCrc { expected: u32, actual: u32 },
}

pub type PartValidResult<T> = Result<T, PartValidError>;

impl Error for PartValidError {}

impl fmt::Display for PartValidError {
    #[rustfmt::skip] // rustfmt mangles this, use manual consistent formatting
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BadSize(size) => {
                write!(f, "bad nImage part header size. Expected {}, found {}",
                       NIMG_PHDR_SIZE, size)
            }
            Self::BadMagic(magic) => {
                write!(f, "bad nImage part magic. Expected 0x{:016x}, actual 0x{:016x}",
                       NIMG_PHDR_MAGIC, magic)
            }
            Self::BadType(t) => {
                write!(f, "bad nImage part type {}", t)
            }
            Self::BadCrc { expected, actual } => {
                write!(f, "invalid part data CRC. Expected 0x{:08x}, found 0x{:08x}",
                       expected, actual)
            }
        }
    }
}
