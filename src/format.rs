/*!
 * Definitions, parsing, and serialization for the nImage header and binary format.
 * Copyright 2020 Allen Wild
 * SPDX-License-Identifier: GPL-3.0-or-later
 */

use super::errors::*;

/// 8-byte magic for the nImage header, "NEWBSIMG" in ASCII, represented as a u64.
//static NIMG_HDR_MAGIC: [u8; 8] = [b'N', b'E', b'W', b'B', b'S', b'I', b'M', b'G'];
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
    // 4 byte CRC32 checksum of the image data
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
     */
    pub fn from_bytes(data: &[u8]) -> ImageValidResult<Self> {
        if data.len() != NIMG_HDR_SIZE {
            return Err(ImageValidError::BadSize(data.len()));
        }
        todo!()
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
            ptype: ptype,
        }
    }

    pub fn type_name(&self, ptype: PartType) -> &'static str {
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
