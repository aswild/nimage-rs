/*!
 * nimage utilities
 * Copyright 2020 Allen Wild
 * SPDX-License-Identifier: GPL-3.0-or-later
 */

use std::cmp::{max, min};
use std::convert::TryInto;
use std::fs::File;
use std::io::{self, BufRead, BufReader, Read, Seek, SeekFrom, Stdin};
use std::path::Path;

pub use clap::ArgMatches;

pub type CmdResult = Result<(), String>;
pub type CmdHandler = fn(&ArgMatches) -> CmdResult;

/**
 * Assert that an experssion matches a pattern. Based on the std::matches macro
 * but panics if the pattern doesn't match.
 * Requires that the value of the expression implements Debug
 */
#[macro_export]
macro_rules! assert_matches {
    ($expression:expr, $( $pattern:pat )|+ $( if $guard:expr )?) => {
        match $expression {
            $( $pattern )|+ $( if $guard )? => (),
            x => panic!("expression '{}' = '{:?}' does not match '{}'",
                        stringify!($expression), x, stringify!($( $pattern )|+ $( if $guard )?)),
        }
    }
}

/**
 * Reader for a borrowed slice of bytes.
 * All read methods besides read_borrow copy data out of the slice.
 * It's assumed that the data slice doesn't change size, which I think is valid because
 * there can't be shared references and mut references at the same time.
 */
pub struct SliceReader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> SliceReader<'a> {
    /**
     * Create a new SliceReader for the given slice of bytes. The returned object
     * is only valid for the lifetime of the data.
     */
    pub fn new(data: &'a [u8]) -> Self {
        SliceReader { data, pos: 0 }
    }

    /**
     * Get the current read position.
     */
    #[inline]
    pub fn position(&self) -> usize {
        self.pos
    }

    /**
     * Return how many bytes we have left to read.
     */
    #[inline]
    pub fn remaining(&self) -> usize {
        let len = self.data.len();
        if self.pos < len {
            len - self.pos
        } else {
            0
        }
    }

    /**
     * Read one byte of data, or return None if there's no byte left to read.
     */
    pub fn read_byte(&mut self) -> Option<u8> {
        if self.remaining() < 1 {
            None
        } else {
            let byte = self.data[self.pos];
            self.pos += 1;
            Some(byte)
        }
    }

    /**
     * Read 8 bytes, interpret them as a little-endian u64, and return the result.
     * Return None if there were less than 8 bytes remaining.
     */
    pub fn read_u64_le(&mut self) -> Option<u64> {
        if self.remaining() < 8 {
            None
        } else {
            // from_le_bytes consumes an array, so copy from the slice into a stack
            // array first. This ends up compiling down to a few register moves.
            let mut arr = [0u8; 8];
            arr.copy_from_slice(&self.data[self.pos..self.pos + 8]);
            self.pos += 8;
            Some(u64::from_le_bytes(arr))
        }
    }

    /**
     * Read 4 bytes, interpret them as a little-endian u32, and return the result.
     * Return None if there were less than 4 bytes remaining.
     */
    pub fn read_u32_le(&mut self) -> Option<u32> {
        if self.remaining() < 4 {
            None
        } else {
            let mut arr = [0u8; 4];
            arr.copy_from_slice(&self.data[self.pos..self.pos + 4]);
            self.pos += 4;
            Some(u32::from_le_bytes(arr))
        }
    }

    /**
     * Read up to count bytes and return it as a borrowed slice.
     * The returned slice's length may be less than count, or zero.
     */
    pub fn read_borrow(&mut self, count: usize) -> &[u8] {
        let count = min(count, self.remaining());
        let ret = &self.data[self.pos..self.pos + count];
        self.pos += count;
        ret
    }

    /**
     * Advance the read position by count bytes. Returns how many bytes which were
     * skipped, in case there were less than count bytes available to read.
     */
    pub fn skip(&mut self, count: usize) -> usize {
        let count: i64 = count.try_into().unwrap();
        let oldpos = self.pos;
        self.seek(SeekFrom::Current(count)).unwrap();
        self.pos - oldpos
    }
}

impl Read for SliceReader<'_> {
    /**
     * Copy data from this reader into buf. The returned result will always be Ok,
     * though the size may be less than what was requested.
     */
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let count = min(buf.len(), self.data.len() - self.pos);
        (&mut buf[..count]).copy_from_slice(&self.data[self.pos..self.pos + count]);
        self.pos += count;
        Ok(count)
    }
}

impl Seek for SliceReader<'_> {
    /**
     * Seek to a new position in the SliceReader.
     * This function never returns an error, instead the returned value will just be clamped
     * to the range [0, self.data.len()]
     */
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        // no implicit conversions for i64/u64/usize, and u64->i64 could be lossy,
        // hence all the .try_into().unwrap(). We assume that everything will fit well
        // within 0x7fffffff_ffffffff bytes and ignore conversion errors.
        self.pos = match pos {
            SeekFrom::Start(pos /*u64*/) => min(pos as usize, self.data.len()),
            SeekFrom::End(pos /*i64*/) => {
                if pos >= 0 {
                    self.data.len()
                } else {
                    let ilen: i64 = self.data.len().try_into().unwrap();
                    max(0, ilen + pos) as usize
                }
            }
            SeekFrom::Current(pos /*i64*/) => {
                let ipos: i64 = self.pos.try_into().unwrap();
                let ilen: i64 = self.data.len().try_into().unwrap();
                let newpos = ipos + pos;
                max(0, min(newpos, ilen)) as usize
            }
        };
        Ok(self.pos as u64)
    }
}

/**
 * An Input stream which implements Read and BufRead an can either be stdin
 * or a file opened for reading.
 */
pub enum Input {
    Stdin(BufReader<Stdin>),
    File(BufReader<File>),
}

impl Input {
    /**
     * Open a file for buffered reading, or stdin if name is "-"
     */
    pub fn open_file_or_stdin(name: &str) -> Result<Self, String> {
        if name == "-" {
            Ok(Self::Stdin(BufReader::new(io::stdin())))
        } else {
            let path = Path::new(name);
            match File::open(&path) {
                Ok(f) => Ok(Self::File(BufReader::new(f))),
                Err(err) => Err(format!("failed to open '{}' for reading: {}", name, err)),
            }
        }
    }

    /**
     * Check whether this Input object represents a file (return true),
     * or stdin (return false).
     */
    pub fn is_file(&self) -> bool {
        match self {
            Self::Stdin(_) => false,
            Self::File(_) => true,
        }
    }
}

impl Read for Input {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self {
            Self::Stdin(s) => s.read(buf),
            Self::File(f) => f.read(buf),
        }
    }
}

impl BufRead for Input {
    fn fill_buf(&mut self) -> io::Result<&[u8]> {
        match self {
            Self::Stdin(s) => s.fill_buf(),
            Self::File(f) => f.fill_buf(),
        }
    }

    fn consume(&mut self, amt: usize) {
        match self {
            Self::Stdin(s) => s.consume(amt),
            Self::File(f) => f.consume(amt),
        };
    }
}
