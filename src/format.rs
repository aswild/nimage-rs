/*!
 * Definitions, parsing, and serialization for the nImage header and binary format.
 * Copyright 2020 Allen Wild
 * SPDX-License-Identifier: GPL-3.0-or-later
 */

use std::convert::{TryFrom, TryInto};
use std::fmt;
use std::io::{self, Cursor, Seek, SeekFrom, Write};

use super::errors::*;
use super::util::*;
use super::xxhio;

/// 8-byte magic for the nImage header, "NEWBSIMG" in ASCII, or a little-endian u64
pub const NIMG_HDR_MAGIC: u64 = 0x474D4953_4257454E_u64;

/// 8-byte magic for each nImage part, "NIMGPART" in ASCII, or a little-endian u64
pub const NIMG_PHDR_MAGIC: u64 = 0x54524150_474d494e_u64;

/// Current (latest) version of the nImage format supported by this code
pub const NIMG_CURRENT_VERSION: u8 = 3;

/// Size of the nImage header
pub const NIMG_HDR_SIZE: usize = 1024;

/// Size of each nImage part header
pub const NIMG_PHDR_SIZE: usize = 32;

/// Max length (in bytes without a null-terminator) of the nImage name field
pub const NIMG_NAME_LEN: usize = 128;

/// Max number of parts in an image
pub const NIMG_MAX_PARTS: usize = 27;

#[repr(u8)]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum PartType {
    /// Invalid/undefined type
    Invalid = 0,
    /// Filesystem image for /boot, should be FAT for Raspberry Pi
    BootImg,
    /// Filesystem contents to be extrated to /boot
    BootTar,
    /// Filesystem image for the rootfs that should be mounted read-only
    Rootfs,
    /// Filesystem image for the rootfs that should be mounted read-write
    RootfsRw,
}
// Safety! Keep this up to date
const PART_TYPE_LAST: PartType = PartType::RootfsRw;

/// list of part type names, used for Display and TryFrom<&str>
pub static PART_TYPE_NAMES: [(PartType, &str); PART_TYPE_LAST as usize + 1] = [
    (PartType::Invalid, "invalid"),
    (PartType::BootImg, "boot_img"),
    (PartType::BootTar, "boot_tar"),
    (PartType::Rootfs, "rootfs"),
    (PartType::RootfsRw, "rootfs_rw"),
];

impl PartType {
    /**
     * Like try_from but also map the explicit Invalid variant to an error.
     */
    fn from_u8_valid(val: u8) -> PartValidResult<Self> {
        PartType::try_from(val).and_then(|t| match t {
            PartType::Invalid => Err(PartValidError::BadType(PartType::Invalid as u8)),
            x => Ok(x),
        })
    }
}

impl Default for PartType {
    fn default() -> Self {
        Self::Invalid
    }
}

impl TryFrom<u8> for PartType {
    type Error = PartValidError;
    /**
     * Convert a u8 into a PartType, returning Err on an unrecognized type.
     */
    fn try_from(val: u8) -> Result<Self, Self::Error> {
        // Apparently the normal way to do this is a big ugly match block.
        // But since PartType is repr(u8), we can take an unsafe shortcut.
        // This is safe as long as PART_TYPE_LAST is set correctly.
        // We don't have to check (val >= 0) because it's unsigned.
        if val <= (PART_TYPE_LAST as u8) {
            Ok(unsafe { std::mem::transmute(val) })
        } else {
            Err(PartValidError::BadType(val))
        }
    }
}

impl TryFrom<&str> for PartType {
    type Error = ();
    /**
     * Convert a str into a PartType. The unit-type error means "invalid type name"
     */
    fn try_from(name: &str) -> Result<Self, Self::Error> {
        for (t, n) in PART_TYPE_NAMES.iter() {
            if name == *n {
                return Ok(*t);
            }
        }
        Err(())
    }
}

impl fmt::Display for PartType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (t, n) in PART_TYPE_NAMES.iter() {
            if self == t {
                return f.write_str(n);
            }
        }
        // if we get here, then PART_TYPE_NAMES is messed up
        panic!("Missing display name for PartType {:?}", self);
    }
}

#[repr(u8)]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum CompMode {
    /// Part is uncompressed
    None = 0,
    /// Part is compressed with zstd
    Zstd,
    /// Part is compressed with an unspecified format that's readable by libarchive(3) or
    /// bsdcat(1), but otherwise opaque to nimage-rs. See archive_read_filter(3).
    LibArchive,
}
// Safety! Keep this up to date
const COMP_MODE_LAST: CompMode = CompMode::LibArchive;

/// list of comp modes used for Display and TryFrom<&str>
#[rustfmt::skip]
pub static COMP_MODE_NAMES: [(CompMode, &str); COMP_MODE_LAST as usize + 1] = [
    (CompMode::None, "none"),
    (CompMode::Zstd, "zstd"),
    (CompMode::LibArchive, "libarchive"),
];

impl Default for CompMode {
    fn default() -> Self {
        CompMode::None
    }
}

impl TryFrom<u8> for CompMode {
    type Error = PartValidError;
    /**
     * Convert a u8 into a CompMode, return Err on an unrecognized mode.
     */
    fn try_from(val: u8) -> Result<Self, Self::Error> {
        if val <= (COMP_MODE_LAST as u8) {
            // safe because CompMode is repr(u8) and we did a bounds check
            Ok(unsafe { std::mem::transmute(val) })
        } else {
            Err(PartValidError::BadComp(val))
        }
    }
}

impl TryFrom<&str> for CompMode {
    type Error = ();
    fn try_from(name: &str) -> Result<Self, Self::Error> {
        for (t, n) in COMP_MODE_NAMES.iter() {
            if name == *n {
                return Ok(*t);
            }
        }
        Err(())
    }
}

impl fmt::Display for CompMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (t, n) in COMP_MODE_NAMES.iter() {
            if self == t {
                return f.write_str(n);
            }
        }
        // if we get here, then COMP_MODE_NAMES is messed up
        panic!("Missing display name for CompMode {:?}", self);
    }
}

/**
 * The main nImage header struct, in native Rust types. In C this is a packed
 * struct that can be directly read from the file, but that's not so in Rust.
 * This parsed representation of the header omits the magic string and unused
 * fields, containing only data that matters.
 */
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImageHeader {
    // 8 byte magic "NEWBSIMG"
    /// nImage format version
    pub version: u8,

    // 1 byte number of parts in the image, up to NIMG_MAX_PARTS (in Rust as parts.len())
    // 6 unused bytes
    /// name of the image, max NIMG_NAME_LEN (128) bytes
    pub name: String,

    /// vector of part headers, up to NIMG_MAX_PARTS (27)
    pub parts: Vec<PartHeader>,
    // 12 unused bytes
    // 4 byte xxHash32 checksum of the rest of the image header data
}

impl Default for ImageHeader {
    fn default() -> Self {
        ImageHeader { version: NIMG_CURRENT_VERSION, name: String::new(), parts: Vec::new() }
    }
}

/**
 * Struct representing the subheader for each nImage part, using native Rust types
 * rather than a packed representation of the on-disk format.
 */
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PartHeader {
    // 8 byte magic "NIMGPART"
    /// size of the part data
    pub size: u64,

    /// offset of the start of image data, relative to the end of the main header
    pub offset: u64,

    /// part type (1 byte)
    pub ptype: PartType,

    /// compression mode (1 byte)
    pub comp: CompMode,

    // 2 unused bytes
    /// 4 byte xxHash32 checksum of the image data
    pub xxh: u32,
}

impl ImageHeader {
    /**
     * creates a new empty image header with no parts yet
     */
    pub fn new(name: &str) -> Self {
        ImageHeader {
            version: NIMG_CURRENT_VERSION,
            name: String::from(name), // could probably be fancy and use Cow
            parts: Vec::new(),
        }
    }

    /**
     * Parse and validate an nImage header read from disk.
     * Data must be exactly NIMG_HDR_SIZE (1024) bytes long.
     * Relevant data will be copied out of buf, thus the returned object has no
     * lifetime restrictions.
     */
    pub fn from_bytes(buf: &[u8]) -> ImageValidResult<Self> {
        // Ensure that the data is exactly the right size. This way we know that reading
        // all the fields will never error (as long as this function has no bugs)
        if buf.len() != NIMG_HDR_SIZE {
            return Err(ImageValidError::BadSize(buf.len()));
        }

        let mut header = ImageHeader::new("");
        let mut reader = Cursor::new(buf);

        // read and validate magic
        let magic = reader.read_u64_le().unwrap();
        if magic != NIMG_HDR_MAGIC {
            return Err(ImageValidError::BadMagic(magic));
        }

        // validate the hash
        // seek to the last 4 bytes where the hash is
        reader.seek(SeekFrom::End(-4)).unwrap();
        let expected_xxh = reader.read_u32_le().unwrap();
        let actual_xxh = xxhio::xxhash32(&buf[..(NIMG_HDR_SIZE - 4)]);
        if expected_xxh != actual_xxh {
            return Err(ImageValidError::BadHash { expected: expected_xxh, actual: actual_xxh });
        }

        // seek back to right after the magic
        reader.seek(SeekFrom::Start(8)).unwrap();

        header.version = reader.read_byte().unwrap();
        if header.version != NIMG_CURRENT_VERSION {
            return Err(ImageValidError::UnsupportedVersion(header.version));
        }

        let num_parts = reader.read_byte().unwrap() as usize;
        if num_parts > NIMG_MAX_PARTS {
            return Err(ImageValidError::TooManyParts(num_parts));
        }

        // 6 unused bytes
        reader.skip(6);

        // process the name, which is a 128 byte CString that may or may not be null-terminated.
        // CString::new doesn't want to see null bytes, so find and slice it ourself.
        let name = reader.read_borrow(NIMG_NAME_LEN);
        let nullpos = match name.iter().position(|c| *c == b'\0') {
            Some(x) => x,       // position of the first nullbyte
            None => name.len(), // no nullbyte found, use the whole string
        };
        header.name = String::from_utf8_lossy(&name[..nullpos]).into_owned();

        for pidx in 0..num_parts {
            let phdr = reader.read_borrow(NIMG_PHDR_SIZE);
            let phdr = PartHeader::from_bytes(phdr)
                .map_err(|err| ImageValidError::InvalidPart { index: pidx, err })?;
            header.parts.push(phdr);
        }

        // ignore everything after the last used part header:
        //  * empty part header slots
        //  * 12 unused bytes
        //  * 4 byte xxHash32 (already handled)
        Ok(header)
    }

    /**
     * validate an nImage header before serialization,
     * i.e. that it has a valid name, version, and not too many parts
     */
    pub fn validate(&self) -> ImageValidResult<()> {
        if self.version != NIMG_CURRENT_VERSION {
            return Err(ImageValidError::UnsupportedVersion(self.version));
        }
        if self.name.len() > NIMG_NAME_LEN {
            return Err(ImageValidError::NameTooLong(self.name.len()));
        }
        if self.parts.len() > NIMG_MAX_PARTS {
            return Err(ImageValidError::TooManyParts(self.parts.len()));
        }
        for (i, part) in self.parts.iter().enumerate() {
            if part.ptype == PartType::Invalid {
                return Err(ImageValidError::InvalidPart {
                    index: i,
                    err: PartValidError::BadType(PartType::Invalid as u8),
                });
            }
        }
        Ok(())
    }

    /**
     * Serialize this image header into an array of bytes.
     */
    pub fn write_to<W: Write>(&self, writer: W) -> io::Result<()> {
        // validate ourselves, ensuring that the number of parts and name length won't overflow
        self.validate().map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;

        // wrap the writer to a xxhWriter which keeps track of the xxHash32 for everything written
        let mut writer = xxhio::Writer::new(writer);

        writer.write_u64_le(NIMG_HDR_MAGIC)?;
        writer.write_byte(self.version)?;
        writer.write_byte(self.parts.len().try_into().unwrap())?;
        writer.write_zeros(6)?;

        writer.write_all(self.name.as_bytes())?;
        writer.write_zeros(NIMG_NAME_LEN - self.name.len())?;

        for part in self.parts.iter() {
            part.write_to(&mut writer)?;
        }
        writer.write_zeros(NIMG_PHDR_SIZE * (NIMG_MAX_PARTS - self.parts.len()))?;
        writer.write_zeros(12)?;

        // get the xxHash32 of all the data written so far and unwrap the xxh writer
        let xxh = writer.hash();
        let mut writer = writer.into_inner();
        writer.write_u32_le(xxh)?;

        Ok(())
    }

    /**
     * Print image header metadata to a writer. Optionally print the xxHash32 given here,
     * e.g. extracted from the original image, since the hash isn't saved in ImageHeader itself.
     */
    pub fn print_to<W: Write>(&self, w: &mut W, xxh: Option<u32>) -> io::Result<()> {
        let name = if self.name.is_empty() { "[empty]" } else { self.name.as_str() };
        writeln!(w, "Image Name:      {}", name)?;
        writeln!(w, "Image Version:   {}", self.version)?;
        writeln!(w, "Number of Parts: {}", self.parts.len())?;
        if let Some(xxh) = xxh {
            writeln!(w, "Header xxHash:   0x{:08x}", xxh)?;
        }

        for (i, part) in self.parts.iter().enumerate() {
            writeln!(w, "Part {}:", i)?;
            part.print_to(w, 2)?;
        }
        Ok(())
    }
}

impl PartHeader {
    /**
     * Parse and validate an nImage part header read from disk.
     * Data must be exactly NIMG_PHDR_SIZE (32) bytes long.
     */
    pub fn from_bytes(buf: &[u8]) -> PartValidResult<Self> {
        if buf.len() != NIMG_PHDR_SIZE {
            return Err(PartValidError::BadSize(buf.len()));
        }

        let mut header = PartHeader::default();
        let mut reader = Cursor::new(buf);

        let magic = reader.read_u64_le().unwrap();
        if magic != NIMG_PHDR_MAGIC {
            return Err(PartValidError::BadMagic(magic));
        }

        header.size = reader.read_u64_le().unwrap();
        header.offset = reader.read_u64_le().unwrap();
        header.ptype = PartType::from_u8_valid(reader.read_byte().unwrap())?;
        header.comp = CompMode::try_from(reader.read_byte().unwrap())?;

        reader.skip(2);
        header.xxh = reader.read_u32_le().unwrap();

        Ok(header)
    }

    /**
     * Serialize this part header into a writer. On Success, exactly 32 bytes should
     * have been written.
     */
    pub fn write_to<W: Write>(&self, writer: &mut W) -> io::Result<()> {
        // use WriteHelper methods from util.rs, which are automatically implemented
        writer.write_u64_le(NIMG_PHDR_MAGIC)?;
        writer.write_u64_le(self.size)?;
        writer.write_u64_le(self.offset)?;
        writer.write_byte(self.ptype as u8)?;
        writer.write_byte(self.comp as u8)?;
        writer.write_zeros(2)?;
        writer.write_u32_le(self.xxh)?;
        Ok(())
    }

    /**
     * Print a text representation of the part metadata to a writer.
     * Indent is the number of spaces to print before each line.
     */
    pub fn print_to<W: Write>(&self, w: &mut W, indent: usize) -> io::Result<()> {
        let indent = " ".repeat(indent);
        writeln!(w, "{}type:        {}", indent, self.ptype)?;
        writeln!(w, "{}compression: {}", indent, self.comp)?;
        writeln!(w, "{}size:        {}", indent, human_size_extended(self.size))?;
        writeln!(w, "{}offset:      {}", indent, human_size_extended(self.offset))?;
        writeln!(w, "{}xxHash:      0x{:08x}", indent, self.xxh)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assert_matches;

    const GOOD_HEADER_START: &[u8] = b"\
        \x4e\x45\x57\x42\x53\x49\x4d\x47\x03\x02\x00\x00\x00\x00\x00\x00\
        \x32\x30\x32\x30\x2d\x30\x35\x2d\x32\x37\x2d\x72\x61\x73\x70\x69\
        \x6f\x73\x2d\x62\x75\x73\x74\x65\x72\x2d\x6c\x69\x74\x65\x2d\x61\
        \x72\x6d\x68\x66\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\
        \x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\
        \x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\
        \x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\
        \x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\
        \x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\
        \x4e\x49\x4d\x47\x50\x41\x52\x54\x38\xe8\xdb\x01\x00\x00\x00\x00\
        \x00\x00\x00\x00\x00\x00\x00\x00\x01\x01\x00\x00\x70\x86\x4b\xe7\
        \x4e\x49\x4d\x47\x50\x41\x52\x54\x00\x50\x23\x14\x00\x00\x00\x00\
        \x40\xe8\xdb\x01\x00\x00\x00\x00\x03\x00\x00\x00\x41\x68\x84\xb6\
    ";

    const GOOD_HEADER_HASH: &[u8] = b"\x6c\xf6\x52\xc2";

    fn good_header_bytes() -> [u8; NIMG_HDR_SIZE] {
        // construct the full header array at runtime so we don't have a page worth of zero bytes
        // in the source code
        let mut arr = [0u8; NIMG_HDR_SIZE];
        (&mut arr[..GOOD_HEADER_START.len()]).copy_from_slice(GOOD_HEADER_START);
        (&mut arr[(NIMG_HDR_SIZE - 4)..]).copy_from_slice(GOOD_HEADER_HASH);
        arr
    }

    fn good_header_obj() -> ImageHeader {
        ImageHeader {
            version: NIMG_CURRENT_VERSION,
            name: String::from("2020-05-27-raspios-buster-lite-armhf"),
            parts: vec![
                PartHeader {
                    size: 0x1dbe838,
                    offset: 0,
                    ptype: PartType::BootImg,
                    comp: CompMode::Zstd,
                    xxh: 0xe74b8670,
                },
                PartHeader {
                    size: 0x14235000,
                    offset: 0x1dbe840,
                    ptype: PartType::Rootfs,
                    comp: CompMode::None,
                    xxh: 0xb6846841,
                },
            ],
        }
    }

    #[test]
    fn parse_image_header() {
        let mut data = good_header_bytes();
        let header = ImageHeader::from_bytes(&data).unwrap();
        assert_eq!(header, good_header_obj());

        // mangle the image magic, verify we get the correct error
        data[0] = 0;
        assert_matches!(ImageHeader::from_bytes(&data), Err(ImageValidError::BadMagic(_)));

        // fix the image magic, break the second header magic
        data[0] = 0x4e;
        data[0xb0] = 0;
        // fix the main hash to match the broken phdr data
        (&mut data[(NIMG_HDR_SIZE - 4)..]).copy_from_slice(&0x03031f18_u32.to_le_bytes());
        // expect a specific BadMagic error
        let expected_err = ImageValidError::InvalidPart {
            index: 1,
            err: PartValidError::BadMagic(0x54524150_474d4900_u64),
        };
        assert_eq!(ImageHeader::from_bytes(&data).unwrap_err(), expected_err);
    }

    #[test]
    fn write_image_header() {
        let header = good_header_obj();
        let mut arr = [0u8; NIMG_HDR_SIZE];

        let mut writer = Cursor::new(arr.as_mut());
        if let Err(err) = header.write_to(&mut writer) {
            panic!("Failed to serialize header: {}", err);
        };

        assert_eq!(arr.as_ref(), good_header_bytes().as_ref());
    }
}
