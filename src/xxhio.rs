/*!
 * IO Wrappers for twox_hash::XxHash32
 *
 * Copyright 2020 Allen Wild
 * SPDX-License-Identifier: GPL-3.0-or-later
 */

use std::hash::Hasher;
use std::io::{self, Read, Write};

use twox_hash::XxHash32;

/**
 * One-off xxHash32 of a byte slice.
 */
pub fn xxhash32(buf: &[u8]) -> u32 {
    let mut hasher = XxHash32::with_seed(0);
    hasher.write(buf);
    hasher.finish_32()
}

/**
 * Encapsulate any reader, and calculate a xxHash32 on all bytes read.
 * The generic type R must implement std::Read.
 */
pub struct Reader<R> {
    inner: R,
    xxh: XxHash32,
}

impl<R: Read> Reader<R> {
    /**
     * Create a new xxHash32 reader, taking ownership of the inner reader.
     */
    pub fn new(inner: R) -> Self {
        Reader { inner, xxh: XxHash32::with_seed(0) }
    }

    /**
     * Get the xxHash32 of all data read so far.
     */
    pub fn hash(&self) -> u32 {
        self.xxh.finish_32() as u32
    }

    /**
     * Get the total number of bytes read so far.
     */
    pub fn total_len(&self) -> u64 {
        self.xxh.total_len()
    }

    /**
     * Consume this object and return the inner reader.
     * The hash data will be lost, so call hash() before this if needed.
     */
    pub fn into_inner(self) -> R {
        self.inner
    }
}

impl<R: Read> Read for Reader<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        // first read into buf from the inner reader, then update the hash.
        // This doesn't violate "if an error is returned then it must be guaranteed
        // that no bytes were read" because the xxhash write can never fail.
        let ret = self.inner.read(buf);
        if let Ok(count) = ret {
            self.xxh.write(&buf[..count]);
        }
        ret
    }
}

/**
 * Encapsulate any writer, and calculate a xxHash32 on all bytes read.
 * The generic type W must implement std::Write.
 */
pub struct Writer<W> {
    inner: W,
    xxh: XxHash32,
}

impl<W: Write> Writer<W> {
    /**
     * Create a new xxHash32 writer, taking ownership of the inner writer.
     */
    pub fn new(inner: W) -> Self {
        Writer { inner, xxh: XxHash32::with_seed(0) }
    }

    /**
     * Get the xxHash32 of all data written so far.
     */
    pub fn hash(&self) -> u32 {
        self.xxh.finish_32()
    }

    /**
     * Get the total number of bytes written so far.
     */
    pub fn total_len(&self) -> u64 {
        self.xxh.total_len()
    }

    /**
     * Consume this object and return the inner writer.
     * The hash data will be lost, so call hash() before this if needed.
     */
    pub fn into_inner(self) -> W {
        self.inner
    }
}

impl<W: Write> Write for Writer<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let ret = self.inner.write(buf);
        if let Ok(count) = ret {
            self.xxh.write(&buf[..count]);
        }
        ret
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

#[cfg(test)]
mod test {
    use std::io::{self, Read, Write};

    use super::{Reader, Writer};

    const LOREM_IPSUM: &'static [u8] = b"\
Lorem ipsum dolor sit amet, consectetur adipiscing elit. Pellentesque id dolor
ut lorem rutrum pulvinar sed id augue. Pellentesque neque magna, dapibus eget
congue pretium, suscipit nec eros. Vestibulum ipsum metus, efficitur vitae erat
et, sodales interdum quam. Ut nisl eros, semper at fermentum euismod, faucibus
nec dolor. Suspendisse tincidunt lorem luctus dui dapibus finibus. Donec sed
molestie urna, quis suscipit orci. Donec gravida arcu in nisi facilisis
imperdiet. Donec nisi sem, iaculis eu tellus maximus, scelerisque ultricies
tortor. Vestibulum gravida aliquet odio in posuere. Pellentesque sed
ullamcorper augue. Aenean nec sem sem. Fusce condimentum vestibulum nisi quis
dictum.\n";

    const LOREM_IPSUM_HASH: u32 = 0x287e3424;

    #[test]
    fn test_reader() {
        let mut reader = Reader::new(LOREM_IPSUM);
        let mut data = Vec::<u8>::new();
        let count = reader.read_to_end(&mut data).unwrap();
        assert_eq!(count, LOREM_IPSUM.len());
        assert_eq!(count, data.len());
        assert_eq!(reader.hash(), LOREM_IPSUM_HASH);
    }

    #[test]
    fn test_writer() {
        // test lorem ipsum
        let mut writer = Writer::new(io::sink());
        let count = writer.write(LOREM_IPSUM).unwrap();
        assert_eq!(count, LOREM_IPSUM.len());
        assert_eq!(writer.hash(), LOREM_IPSUM_HASH);

        // test empty (matches twox-hash test case)
        let mut writer = Writer::new(io::sink());
        writer.write(&[]).unwrap();
        assert_eq!(writer.total_len(), 0);
        assert_eq!(writer.hash(), 0x02cc5d05);

        // another test from twox-hash
        let mut writer = Writer::new(io::sink());
        writer.write(b"Hello, world!\0").unwrap();
        assert_eq!(writer.total_len(), 14);
        assert_eq!(writer.hash(), 0x9e5e7e93);
    }
}
