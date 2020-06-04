/*!
 * mknImage: a tool to work with files in the nImage format.
 * handler for the hash subcommand.
 *
 * Copyright 2020 Allen Wild
 * SPDX-License-Identifier: GPL-3.0-or-later
 */

use std::io;

use nimage::util::Input;
use nimage::xxhio::Reader;

use crate::*;

pub fn cmd_hash(args: &ArgMatches) -> CmdResult {
    let input = Input::open_file_or_stdin(args.value_of("FILE").unwrap_or("-"))?;
    let mut reader = Reader::new(input);
    if let Err(err) = io::copy(&mut reader, &mut io::sink()) {
        Err(anyhow!("failed reading: {}", err))
    } else {
        println!("0x{:08x}", reader.hash());
        Ok(())
    }
}
