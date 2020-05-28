/*!
 * mknImage: a tool to work with files in the nImage format.
 * handler for the check subcommand.
 *
 * Copyright 2020 Allen Wild
 * SPDX-License-Identifier: GPL-3.0-or-later
 */

use std::io;
use std::io::prelude::*;

use nimage::crc32::Reader as CrcReader;
use nimage::format::*;
use nimage::util::*;

use crate::*;

pub fn cmd_create(args: &ArgMatches) -> CmdResult {
    Err("Not yet implemented".into())
}
