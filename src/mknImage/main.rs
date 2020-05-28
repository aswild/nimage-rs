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

mod check;
mod crc32;
mod create;

use std::convert::From;
use std::fmt;
use std::process::exit;

use clap::{crate_version, App, AppSettings, Arg, SubCommand};

use nimage::format::{NIMG_MAX_PARTS, NIMG_NAME_LEN, PART_TYPE_NAMES};

// exports to command modules
pub use clap::ArgMatches;

/**
 * String wrapper type since we can't write implementations for external types
 * like "impl From<io::Error> for String"
 * This lets handler functions use ? on calls that return io::Result without
 * having to manually map the error to a String. Other uses need to call .into()
 * to turn one of the From types into a CmdError.
 */
pub struct CmdError(String);

// this From implementation that works for everything which implements Display is cool,
// but it means that CmdError itself can't be Display or else we fail to build with
// "conflicting implementations of trait `std::convert::From<CmdError>` for type `CmdError`"
impl<T: fmt::Display> From<T> for CmdError {
    fn from(e: T) -> Self {
        CmdError(e.to_string())
    }
}

pub type CmdResult = Result<(), CmdError>;
pub type CmdHandler = fn(&ArgMatches) -> CmdResult;

fn get_handler(name: &str) -> CmdHandler {
    match name {
        "create" => create::cmd_create,
        "check" => check::cmd_check,
        "crc32" => crc32::cmd_crc32,
        _ => unreachable!("command handler not found"),
    }
}

fn main() {
    // comma separated string listing all the valid part types
    let part_types = PART_TYPE_NAMES
        .iter()
        .map(|pair| pair.1)
        .collect::<Vec<&str>>()
        .join(", ");

    // To use format! anywhere in the help text, we have to create the app and call .matches() all
    // in one statement or else we'll get errors about passing references to temporary objects.  If
    // the app needs to be saved as a separate variable, then all the dynamically generated help
    // text strings have to be created separately with a lifetime as long as the app.
    let args = App::new("mknImage")
        .version(crate_version!())
        .about("Build and manipulate nImage files")
        .max_term_width(100)
        .setting(AppSettings::SubcommandRequired)
        .subcommand(
            SubCommand::with_name("create")
                .about("Create an nImage")
                .arg(
                    Arg::with_name("name")
                        .short("n")
                        .long("name")
                        .takes_value(true)
                        .help(format!("Name to embed in the image (max {} chars)", NIMG_NAME_LEN).as_str())
                )
                .arg(
                    Arg::with_name("output")
                        .value_name("IMAGE_FILE")
                        .required(true)
                        .help("Output filename. Must be a regular seekable file, not a pipe")
                )
                .arg(
                    Arg::with_name("inputs")
                        .value_name("TYPE:FILE")
                        .required(true)
                        .multiple(true)
                        .min_values(1)
                        .max_values(NIMG_MAX_PARTS as u64)
                        .help("List of input files to add to the image.")
                )
                .after_help(format!("Valid part types are: {}", part_types).as_str())
        )
        .subcommand(
            SubCommand::with_name("check")
                .about("Check an nImage file for errors and print header information")
                .arg(
                    Arg::with_name("FILE")
                        .required(false)
                        .help("Input file. Read from stdin if FILE isn't present or is '-'")
                )
                .arg(
                    Arg::with_name("quiet")
                        .short("q")
                        .long("quiet")
                        .multiple(true)
                        .help("Only check for errors, don't dump info. \
                               Pass -q twice to suppress printing errors and only use the exit code.")
                ),
        )
        .subcommand(
            SubCommand::with_name("crc32")
                .about("Read a file and compute its CRC32")
                .arg(
                    Arg::with_name("FILE")
                        .required(false)
                        .help("Input file. Read stdin if FILE isn't present or is '-'")
                ),
        )
        .get_matches();

    let (subname, subargs) = args.subcommand();
    let subargs = subargs.unwrap();

    if let Err(err) = get_handler(subname)(subargs) {
        if !err.0.is_empty() {
            eprintln!("Error: {}", err.0);
        }
        exit(1);
    }
}
