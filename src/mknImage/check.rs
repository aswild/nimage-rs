/*!
 * mknImage: a tool to work with files in the nImage format.
 * handler for the check subcommand.
 *
 * Copyright 2020 Allen Wild
 * SPDX-License-Identifier: GPL-3.0-or-later
 */

use std::io::prelude::*;
use std::io::{self, Cursor, SeekFrom};

use nimage::format::*;
use nimage::util::*;
use nimage::xxhio;

use crate::*;

macro_rules! qprintln {
    ($quiet:expr, $($arg:tt)*) => {
        if !$quiet { println!($($arg)*) }
    }
}

/// Read the last 4 bytes of buf as a u32le. Panics if buf isn't at least 4 bytes long
fn last_u32(buf: &[u8]) -> u32 {
    let mut reader = Cursor::new(buf);
    reader.seek(SeekFrom::End(-4)).unwrap();
    reader.read_u32_le().unwrap()
}

/// Read exactly count bytes from input and return the xxHash32.
fn read_exact_xxh<R: Read>(input: &mut R, count: u64) -> io::Result<u32> {
    let mut writer = xxhio::Writer::new(io::sink());
    let read = io::copy(&mut input.take(count), &mut writer)?;
    if read == count {
        Ok(writer.hash())
    } else {
        Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            format!("read only {}/{} bytes", read, count),
        ))
    }
}

#[allow(clippy::comparison_chain)] // suppress lint on the "if part.offset < current_offset"
fn check_image(mut input: Input, q: bool) -> CmdResult {
    qprintln!(q, "{}:", input);
    let mut header_bytes = [0u8; NIMG_HDR_SIZE];
    input.read_exact(&mut header_bytes)?;
    let header = ImageHeader::from_bytes(&header_bytes)?;

    if !q {
        // header doesn't store its xxh, get it from the original buffer
        let xxh = last_u32(&header_bytes);
        header.print_to(&mut io::stdout(), Some(xxh))?;
    }

    // validate all the parts' data
    let mut current_offset = 0u64;
    for (i, part) in header.parts.iter().enumerate() {
        // handle padding before this part
        if part.offset < current_offset {
            return Err(anyhow!("Part {} offset {} is out of order", i, part.offset));
        } else if part.offset > current_offset {
            let pad_bytes = part.offset - current_offset;
            let mut padding = vec![0u8; pad_bytes as usize];
            input
                .read_exact(&mut padding)
                .with_context(|| format!("failed to read padding before part {}", i))?;
            current_offset += pad_bytes;
        }

        // wrap the input to only read part.size bytes, then wrap that in a hash reader
        let actual_xxh = read_exact_xxh(&mut input, part.size)
            .with_context(|| format!("failed to read data for part {}", i))?;
        if actual_xxh != part.xxh {
            return Err(anyhow!(
                "Part {} hash is invalid: expected 0x{:08x} actual 0x{:08x}",
                i,
                part.xxh,
                actual_xxh
            ));
        }

        current_offset += part.size;
    }

    qprintln!(q, "Image check SUCCESS");
    Ok(())
}

pub fn cmd_check(args: &ArgMatches) -> CmdResult {
    let quiet_level = args.occurrences_of("quiet");
    let input = Input::open_file_or_stdin(args.value_of("FILE").unwrap_or("-"))?;

    let ret = check_image(input, quiet_level > 0);
    if quiet_level >= 2 {
        // silent mode, hide the error messasge, but still return error
        ret.map_err(|_| anyhow!(""))
    } else {
        ret
    }
}
