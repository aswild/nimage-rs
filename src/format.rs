/*!
 * Definitions, parsing, and serialization for the nImage header and binary format.
 * Copyright 2020 Allen Wild
 * SPDX-License-Identifier: GPL-3.0-or-later
 */

use std::convert::{TryFrom, TryInto};
use std::io::{self, Cursor, Seek, SeekFrom, Write};

use super::crc32::{crc32_data, Writer as CrcWriter};
use super::errors::*;
use super::util::*;

/// 8-byte magic for the nImage header, "NEWBSIMG" in ASCII, or a little-endian u64
pub const NIMG_HDR_MAGIC: u64 = 0x474D4953_4257454E_u64;

/// 8-byte magic for each nImage part, "NIMGPART" in ASCII, or a little-endian u64
pub const NIMG_PHDR_MAGIC: u64 = 0x54524150_474d494e_u64;

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
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
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
#[derive(Clone, Debug, Eq, PartialEq)]
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
#[derive(Clone, Debug, Eq, PartialEq)]
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
        let mut reader = Cursor::new(buf);

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
        self.validate()
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;

        // wrap the writer to a CrcWriter which keeps track of the CRC32 for everything written
        let mut writer = CrcWriter::new(writer);

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

        // get the CRC32 of all the data written so far and unwrap the CRC writer
        let crc = writer.sum();
        let mut writer = writer.into_inner();
        writer.write_u32_le(crc)?;

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
        let mut reader = Cursor::new(buf);

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
        writer.write_zeros(3)?;
        writer.write_u32_le(self.crc)?;
        Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assert_matches;

    #[rustfmt::skip]
    const GOOD_HEADER_START: [u8; 208] = [
        0x4e, 0x45, 0x57, 0x42, 0x53, 0x49, 0x4d, 0x47, 0x02, 0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x63, 0x6f, 0x72, 0x65, 0x2d, 0x69, 0x6d, 0x61, 0x67, 0x65, 0x2d, 0x6e, 0x65, 0x77, 0x62, 0x73,
        0x2d, 0x72, 0x61, 0x73, 0x70, 0x62, 0x65, 0x72, 0x72, 0x79, 0x70, 0x69, 0x34, 0x2d, 0x36, 0x34,
        0x2d, 0x32, 0x30, 0x32, 0x30, 0x30, 0x35, 0x30, 0x31, 0x32, 0x32, 0x32, 0x38, 0x32, 0x37, 0x2e,
        0x73, 0x71, 0x75, 0x61, 0x73, 0x68, 0x66, 0x73, 0x2d, 0x7a, 0x73, 0x74, 0x64, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x4e, 0x49, 0x4d, 0x47, 0x50, 0x41, 0x52, 0x54, 0xe0, 0xee, 0x91, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x09, 0x00, 0x00, 0x00, 0x21, 0x28, 0x7c, 0xcd,
        0x4e, 0x49, 0x4d, 0x47, 0x50, 0x41, 0x52, 0x54, 0x00, 0xe0, 0xb0, 0x06, 0x00, 0x00, 0x00, 0x00,
        0xe0, 0xee, 0x91, 0x00, 0x00, 0x00, 0x00, 0x00, 0x05, 0x00, 0x00, 0x00, 0x38, 0xf9, 0xc5, 0x1f,
    ];
    const GOOD_HEADER_CRC: [u8; 4] = [0xed, 0x28, 0x2e, 0xf9];

    fn good_header_bytes() -> [u8; NIMG_HDR_SIZE] {
        // construct the full header array at runtime so we don't have a page worth of zero bytes
        // in the source code
        let mut arr = [0u8; NIMG_HDR_SIZE];
        (&mut arr[..GOOD_HEADER_START.len()]).copy_from_slice(&GOOD_HEADER_START);
        (&mut arr[(NIMG_HDR_SIZE - 4)..]).copy_from_slice(&GOOD_HEADER_CRC);
        arr
    }

    fn good_header_obj() -> ImageHeader {
        ImageHeader {
            version: NIMG_CURRENT_VERSION,
            name: String::from("core-image-newbs-raspberrypi4-64-20200501222827.squashfs-zstd"),
            parts: vec![
                PartHeader {
                    size: 0x0091eee0,
                    offset: 0,
                    ptype: PartType::BootImgZstd,
                    crc: 0xcd7c2821,
                },
                PartHeader {
                    size: 0x06b0e000,
                    offset: 0x0091eee0,
                    ptype: PartType::Rootfs,
                    crc: 0x1fc5f938,
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
        assert_matches!(
            ImageHeader::from_bytes(&data),
            Err(ImageValidError::BadMagic(_))
        );

        // fix the image magic, break the second header magic
        data[0] = 0x4e;
        data[0xb0] = 0;
        // fix the main crc to match the broken phdr data
        (&mut data[(NIMG_HDR_SIZE - 4)..]).copy_from_slice(&[0x87, 0x06, 0xef, 0x9b]);
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
