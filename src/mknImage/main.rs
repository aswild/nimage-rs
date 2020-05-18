/*!
 * mknImage: a tool to work with files in the nImage format.
 * main executable
 *
 * Copyright 2020 Allen Wild
 * SPDX-License-Identifier: GPL-3.0-or-later
 */

// don't complain that mknImage isn't a snake-case crate name, we want that
// to be the binary name.
// Unfortunately this has the side effect of disabling snake case warnings throughout
// the entire mknimage crate. See https://github.com/rust-lang/rust/issues/45127
#![allow(non_snake_case)]

use std::process::exit;

use clap::{crate_version, App, AppSettings, Arg, SubCommand};

use nimage::util::*;

mod crc32;
use crc32::cmd_crc32;

fn app() -> App<'static, 'static> {
    App::new("mknImage")
        .version(crate_version!())
        .about("Build and manipulate nImage files")
        .setting(AppSettings::SubcommandRequired)
        .subcommand(
            SubCommand::with_name("crc32")
                .about("Read a file and compute the CRC32")
                .arg(
                    Arg::with_name("FILE")
                        .required(false)
                        .help("Input file. Read stdin if FILE isn't present or is '-'"),
                ),
        )
}

fn get_handler(name: &str) -> CmdHandler {
    match name {
        "crc32" => cmd_crc32,
        _ => unreachable!("command handler not found"),
    }
}

fn main() {
    let args = app().get_matches();
    let (subname, subargs) = args.subcommand();
    let subargs = subargs.unwrap();

    if let Err(msg) = get_handler(subname)(subargs) {
        eprintln!("Error: {}", msg);
        exit(1);
    }
}
