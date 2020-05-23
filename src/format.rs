/*!
 * Definitions, parsing, and serialization for the nImage header and binary format.
 * Copyright 2020 Allen Wild
 * SPDX-License-Identifier: GPL-3.0-or-later
 */

use std::convert::TryFrom;
use std::io::{Seek, SeekFrom};

use super::crc32::*;
use super::errors::*;
use super::util::*;

/// 8-byte magic for the nImage header, "NEWBSIMG" in ASCII, represented as a u64.
pub const NIMG_HDR_MAGIC: u64 = 0x4E45574253494D47u64;

/// 8-byte magic for each nImage part, "NIMGPART" in ASCII, represented as a u64.
pub const NIMG_PHDR_MAGIC: u64 = 0x4E494D4750415254u64;

/// Current (latest) version of the nImage format supported by this code
pub const NIMG_CURRENT_VERSION: u8 = 2;

/// Size of the nImage header
pub const NIMG_HDR_SIZE: usize = 1024;

/// Size of each nImage part header
pub const NIMG_PHDR_SIZE: usize = 32;

/// Max length (in bytes without a null-terminator) of the nImage name field
pub const NIMG_NAME_LEN: usize = 128;

/// Max number of parts in an image
pub const NIMG_MAX_PARTS: usize = 27;

#[repr(u8)]
pub enum PartType {
    Invalid = 0,
    BootImg,
    BootTar,
    BootTarGz,
    BootTarXz,
    Rootfs,
    RootfsRw,
    BootImgGz,
    BootImgXz,
    BootImgZstd,
}
// Safety! Keep this up to date
const PART_TYPE_LAST: PartType = PartType::BootImgZstd;

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

/**
 * The main nImage header struct, in native Rust types. In C this is a packed
 * struct that can be directly read from the file, but that's not so in Rust.
 * This parsed representation of the header omits the magic string and unused
 * fields, containing only data that matters.
 */
pub struct ImageHeader {
    // 8 byte magic "NEWBSIMG"
    /// nImage format version
    version: u8,

    // 1 byte number of parts in the image, up to NIMG_MAX_PARTS (in Rust as parts.len())
    // 6 unused bytes
    /// name of the image, max NIMG_NAME_LEN (128) bytes
    name: String,

    /// vector of part headers, up to NIMG_MAX_PARTS (27)
    parts: Vec<PartHeader>,
    // 12 unused bytes
    // 4 byte CRC32 checksum of the rest of the image header data
}

/**
 * Struct representing the subheader for each nImage part, using native Rust types
 * rather than a packed representation of the on-disk format.
 */
pub struct PartHeader {
    // 8 byte magic "NIMGPART"
    /// size of the part data
    size: u64,

    /// offset of the start of image data, relative to the end of the main header
    offset: u64,

    /// part type (1 byte)
    ptype: PartType,

    // 3 unused bytes
    /// 4 byte CRC32 checksum of the image data
    crc: u32,
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
        let mut reader = SliceReader::new(buf);

        // read and validate magic
        let magic = reader.read_u64_le().unwrap();
        if magic != NIMG_HDR_MAGIC {
            return Err(ImageValidError::BadMagic(magic));
        }

        // validate the CRC
        // seek to the last 4 bytes where the CRC is
        reader.seek(SeekFrom::End(-4)).unwrap();
        let expected_crc = reader.read_u32_le().unwrap();
        let actual_crc = crc32_data(&buf[..(NIMG_HDR_SIZE - 4)]);
        if expected_crc != actual_crc {
            return Err(ImageValidError::BadCrc {
                expected: expected_crc,
                actual: actual_crc,
            });
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
        //  * 4 byte CRC32 (already handled)
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
        Ok(())
    }
}

impl PartHeader {
    /**
     * Create a new empty nImage part header with the given type.
     */
    pub fn new(ptype: PartType) -> Self {
        PartHeader {
            size: 0,
            offset: 0,
            ptype,
            crc: 0,
        }
    }

    /**
     * Parse and validate an nImage part header read from disk.
     * Data must be exactly NIMG_PHDR_SIZE (32) bytes long.
     */
    pub fn from_bytes(buf: &[u8]) -> PartValidResult<Self> {
        if buf.len() != NIMG_PHDR_SIZE {
            return Err(PartValidError::BadSize(buf.len()));
        }

        let mut header = PartHeader::new(PartType::Invalid);
        let mut reader = SliceReader::new(buf);

        let magic = reader.read_u64_le().unwrap();
        if magic != NIMG_PHDR_MAGIC {
            return Err(PartValidError::BadMagic(magic));
        }

        header.size = reader.read_u64_le().unwrap();
        header.offset = reader.read_u64_le().unwrap();
        header.ptype = PartType::from_u8_valid(reader.read_byte().unwrap())?;

        reader.skip(3);
        header.crc = reader.read_u32_le().unwrap();

        Ok(header)
    }

    pub fn type_name(&self) -> &'static str {
        match self.ptype {
            PartType::Invalid => "invalid",
            PartType::BootImg => "boot_img",
            PartType::BootTar => "boot_tar",
            PartType::BootTarGz => "boot_tar_gz",
            PartType::BootTarXz => "boot_tar_xz",
            PartType::Rootfs => "rootfs",
            PartType::RootfsRw => "rootfs_rw",
            PartType::BootImgGz => "boot_img_gz",
            PartType::BootImgXz => "boot_img_xz",
            PartType::BootImgZstd => "boot_img_zstd",
        }
    }
}
