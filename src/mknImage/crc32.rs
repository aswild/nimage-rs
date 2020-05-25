/*!
 * mknImage: a tool to work with files in the nImage format.
 * handler for the crc32 subcommand.
 *
 * Copyright 2020 Allen Wild
 * SPDX-License-Identifier: GPL-3.0-or-later
 */

use std::io;

use nimage::crc32::Reader as CrcReader;
use nimage::util::*;

use crate::*;

pub fn cmd_crc32(args: &ArgMatches) -> CmdResult {
    let input = Input::open_file_or_stdin(args.value_of("FILE").unwrap_or("-"))?;
    let mut reader = CrcReader::new(input);
    if let Err(err) = io::copy(&mut reader, &mut io::sink()) {
        Err(format!("failed reading: {}", err).into())
    } else {
        println!("0x{:08x}", reader.sum());
        Ok(())
    }
}
