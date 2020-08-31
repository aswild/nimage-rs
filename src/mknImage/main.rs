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
mod create;
mod hash;

use clap::{crate_version, App, AppSettings, Arg, ArgMatches, SubCommand};

use nimage::format::{COMP_MODE_NAMES, NIMG_MAX_PARTS, NIMG_NAME_LEN, PART_TYPE_NAMES};

// exports to command modules
pub type CmdResult = anyhow::Result<()>;
pub type CmdHandler = fn(&ArgMatches) -> CmdResult;

// debug log flag as a global variable. safe beacuse we're single-threaded and this gets set only
// once at program startup.
static mut DEBUG_ENABLED: bool = false;

/// write a formatted message to stderr, prefixed with "[DEBUG FILE:LINE] ".
/// This simple version uses a static global boolean
#[macro_export]
macro_rules! debug {
    ($($arg:tt)*) => {
        if unsafe { DEBUG_ENABLED } {
            let mut filename = file!();
            if filename.starts_with("src/") {
                filename = &filename[4..];
            }
            if filename.ends_with(".rs") {
                filename = &filename[..(filename.len()-3)];
            }
            eprint!("[DEBUG {}:{}] ", filename, line!());
            eprintln!($($arg)*);
        }
    }
}

fn get_handler(name: &str) -> CmdHandler {
    match name {
        "create" => create::cmd_create,
        "check" => check::cmd_check,
        "hash" => hash::cmd_hash,
        _ => unreachable!("command handler not found"),
    }
}

fn main() {
    // comma separated string listing all the valid part types. Skip the first "invalid" entry
    let part_types = PART_TYPE_NAMES.iter().skip(1).map(|x| x.1).collect::<Vec<&str>>().join(", ");
    let comp_modes = COMP_MODE_NAMES.iter().map(|x| x.1).collect::<Vec<&str>>().join(", ");

    // To use format! anywhere in the help text, we have to create the app and call .get_matches()
    // all in one statement or else we'll get errors about passing references to temporary objects.
    // If the app needs to be saved as a separate variable, then all the dynamically generated help
    // text strings have to be created separately with a lifetime as long as the app.
    let args = App::new("mknImage")
        .version(crate_version!())
        .about("Build and manipulate nImage files")
        .max_term_width(100)
        .global_setting(AppSettings::ColoredHelp)
        .setting(AppSettings::SubcommandRequired)
        .arg(
            Arg::with_name("debug")
                .short("D")
                .long("debug")
                .help("Enable extra debug output")
        )
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
                    Arg::with_name("parts")
                        .value_name("FILE:TYPE[:COMPRESSION]")
                        .required(true)
                        .multiple(true)
                        .min_values(1)
                        .max_values(NIMG_MAX_PARTS as u64)
                        .help("List of parts to add to the image")
                )
                .after_help(format!("Valid part types are: {}\n\
                                     Valid compression modes are: {}\n\
                                     If omitted, the default compression mode is 'none'.\n\
                                     If the zstd compression mode is specified as 'zstd+' or 'zstd+N', \
                                     mknImage will assume the input file is uncompressed and compress it \
                                     with zstd level N (default 15), otherwise it's assumed the part is \
                                     already compressed.",
                                    part_types, comp_modes).as_str())
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
            SubCommand::with_name("hash")
                .about("Read a file and compute its xxHash32")
                .arg(
                    Arg::with_name("FILE")
                        .required(false)
                        .help("Input file. Read stdin if FILE isn't present or is '-'")
                ),
        )
        .get_matches();

    unsafe {
        DEBUG_ENABLED = args.is_present("debug");
    }
    debug!("debug logging enabled");

    let (subname, subargs) = args.subcommand();
    let subargs = subargs.unwrap();

    if let Err(err) = get_handler(subname)(subargs) {
        if !err.to_string().is_empty() {
            eprintln!("Error: {:#}", err);
        }
        std::process::exit(1);
    }
}
