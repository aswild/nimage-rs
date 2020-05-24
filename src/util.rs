/*!
 * nimage utilities
 * Copyright 2020 Allen Wild
 * SPDX-License-Identifier: GPL-3.0-or-later
 */

use std::convert::{AsRef, TryInto};
use std::fs::File;
use std::io::{self, BufRead, BufReader, Cursor, Read, Seek, SeekFrom, Stdin, Write};
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

#[macro_export]
macro_rules! assert_ok {
    ($expression:expr) => {
        assert_matches!($expression, Ok(_))
    };
}

#[macro_export]
macro_rules! assert_err {
    ($expression:expr) => {
        assert_matches!($expression, Err(_))
    };
}

/**
 * Extension of io::Cursor for reading numeric fields.
 */
pub trait ReadHelper {
    /**
     * Read one byte of data, or return None if there's no byte left to read.
     */
    fn read_byte(&mut self) -> Option<u8>;

    /**
     * Read 4 bytes, interpret them as a little-endian u32, and return the result.
     * Return None if there were less than 4 bytes remaining.
     */
    fn read_u32_le(&mut self) -> Option<u32>;

    /**
     * Read 8 bytes, interpret them as a little-endian u64, and return the result.
     * Return None if there were less than 8 bytes remaining.
     */
    fn read_u64_le(&mut self) -> Option<u64>;

    /**
     * Read up to count bytes and return it as a borrowed slice.
     * The returned slice's length may be less than count, or zero.
     */
    fn read_borrow(&mut self, count: usize) -> &[u8];

    /**
     * Advance the read position by count bytes. Returns how many bytes which were
     * skipped, in case there were less than count bytes available to read.
     */
    fn skip(&mut self, count: usize) -> usize;
}

impl<T> ReadHelper for Cursor<T>
where
    T: AsRef<[u8]>,
{
    fn read_byte(&mut self) -> Option<u8> {
        let mut b = [0u8];
        self.read_exact(&mut b).ok()?;
        Some(b[0])
    }

    fn read_u32_le(&mut self) -> Option<u32> {
        let mut arr = [0u8; 4];
        self.read_exact(&mut arr).ok()?;
        Some(u32::from_le_bytes(arr))
    }

    fn read_u64_le(&mut self) -> Option<u64> {
        let mut arr = [0u8; 8];
        self.read_exact(&mut arr).ok()?;
        Some(u64::from_le_bytes(arr))
    }

    fn read_borrow(&mut self, count: usize) -> &[u8] {
        let pos = self.position() as usize;
        self.set_position(self.position() + count as u64);
        match self.get_ref().as_ref().get(pos..(pos + count)) {
            Some(ref x) => x,
            None => &[],
        }
    }

    fn skip(&mut self, count: usize) -> usize {
        let count: i64 = count.try_into().unwrap();
        let oldpos = self.position();
        self.seek(SeekFrom::Current(count)).unwrap();
        (self.position() - oldpos) as usize
    }
}

pub trait WriteHelper {
    /**
     * Write one byte of data.
     */
    fn write_byte(&mut self, val: u8) -> io::Result<()>;

    /**
     * Write 4 bytes of data as a little-endian u32.
     */
    fn write_u32_le(&mut self, val: u32) -> io::Result<()>;

    /**
     * Write 8 bytes of data as a little-endian u64.
     */
    fn write_u64_le(&mut self, val: u64) -> io::Result<()>;

    /**
     * Write some number of zero bytes.
     */
    fn write_zeros(&mut self, count: usize) -> io::Result<()>;
}

impl<T: Write> WriteHelper for T {
    fn write_byte(&mut self, val: u8) -> io::Result<()> {
        self.write_all(&[val])
    }

    fn write_u32_le(&mut self, val: u32) -> io::Result<()> {
        self.write_all(&val.to_le_bytes())
    }

    fn write_u64_le(&mut self, val: u64) -> io::Result<()> {
        self.write_all(&val.to_le_bytes())
    }

    fn write_zeros(&mut self, count: usize) -> io::Result<()> {
        let vec = vec![0; count];
        self.write_all(&vec)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[rustfmt::skip]
    const fn header_arr() -> [u8; 32] {
        [
            0x4e, 0x49, 0x4d, 0x47, 0x50, 0x41, 0x52, 0x54,
            0xe0, 0xee, 0x91, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x09, 0x00, 0x00, 0x00, 0x21, 0x28, 0x7c, 0xcd,
        ]
    }

    #[test]
    fn test_read_helper() {
        let arr = header_arr();
        let mut reader = Cursor::new(&arr);

        {
            // read_borrow does a mutable borrow of reader even though it returns an immutable
            // reference to the inner slice. Thus, we can't touch reader again until we're done
            // using magic.
            let magic = reader.read_borrow(8);
            assert_eq!(magic.len(), 8);
            assert_eq!(String::from_utf8_lossy(magic), "NIMGPART");
        }
        assert_eq!(reader.position(), 8);

        // read some integers, check the position along the way
        assert_eq!(reader.read_u32_le(), Some(0x0091eee0));
        assert_eq!(reader.read_u64_le(), Some(0));
        reader.skip(4);
        assert_eq!(reader.position(), 24);
        assert_eq!(reader.read_byte(), Some(0x09));
        reader.skip(3);

        // try to read a u64 when there's only 4 bytes remaining. It should return
        // None and not move the position
        assert_eq!(reader.position(), 28);
        assert_eq!(reader.read_u64_le(), None);
        assert_eq!(reader.position(), 28);

        // verify we can still read
        assert_eq!(reader.read_u32_le(), Some(0xcd7c2821));
        assert_eq!(reader.position(), 32);

        // seek tests
        reader.seek(SeekFrom::Start(8)).unwrap();
        assert_eq!(reader.read_u64_le(), Some(0x00000000_0091eee0));
        reader.seek(SeekFrom::Current(-8)).unwrap();
        assert_eq!(reader.read_u64_le(), Some(0x00000000_0091eee0));
        reader.seek(SeekFrom::End(-4)).unwrap();
        assert_eq!(reader.read_u32_le(), Some(0xcd7c2821));
        assert_eq!(reader.read_byte(), None);
    }

    #[test]
    fn test_write_helper_slice() {
        let mut arr = [0u8; 32];
        let mut writer = Cursor::new(arr.as_mut());

        assert_ok!(writer.write(b"NIMGPART"));
        assert_ok!(writer.write_u64_le(0x0091eee0));
        assert_ok!(writer.write_zeros(8));
        assert_ok!(writer.write_byte(0x09));
        assert_ok!(writer.write_zeros(3));
        assert_ok!(writer.write_u32_le(0xcd7c2821));

        // verify we can't write anything else
        assert!(writer.write_byte(0).is_err());

        // We can't directly access arr because writer holds a mutref to it. But we can
        // get a reference indirectly from writer, which is legal.
        assert_eq!(writer.get_ref(), &header_arr());

        // go back and make sure we can't do a partial write either
        writer.seek(SeekFrom::End(-3)).unwrap();
        assert_eq!(writer.position(), 29);
        assert_err!(writer.write_u32_le(0));
        // not the behavior I want, but can live with. The write methods will write as much
        // as they can before hitting the end, rather than checking for sufficient space first.
        // No easy way around that without specialized implementations.
        assert_eq!(writer.position(), 32);
    }
}
