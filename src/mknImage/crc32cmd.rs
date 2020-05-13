/*!
 * mknImage: a tool to work with files in the nImage format.
 * handler for the crc32 subcommand.
 *
 * Copyright 2020 Allen Wild
 * SPDX-License-Identifier: GPL-3.0-or-later
 */

use std::fs::File;
use std::io::{self, Read, BufReader};
use std::path::Path;

use clap::ArgMatches;

use nimage::crc32::Reader as CrcReader;
use nimage::util::CmdHandler;

pub struct Crc32Cmd;

fn open_input(name: &str) -> Result<Box<dyn Read>, String> {
    let path = Path::new(name);
    match File::open(&path) {
        Ok(f) => Ok(Box::new(f)),
        Err(err) => Err(format!("failed to open {}: {}", name, err)),
    }
}

impl CmdHandler for Crc32Cmd {
    fn run(&self, args: &ArgMatches) -> Result<(), String> {
        let input: Box<dyn Read> = match args.value_of("FILE") {
            None | Some("-") => Box::new(io::stdin()),
            Some(name) => open_input(name)?,
        };

        let mut reader = CrcReader::new(BufReader::new(input));
        if let Err(err) = io::copy(&mut reader, &mut io::sink()) {
            Err(format!("failed reading: {}", err))
        } else {
            println!("{:#08x}", reader.sum());
            Ok(())
        }
    }
}
