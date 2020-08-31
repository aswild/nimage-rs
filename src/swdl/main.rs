/*!
 * swdl: Raspberry Pi firmware update engine.
 * main executable
 *
 * Copyright 2020 Allen Wild
 * SPDX-License-Identifier: GPL-3.0-or-later
 */

mod input;

use std::process::exit;

use anyhow::{anyhow, Context, Result};
use clap::ArgMatches;
use clap::{crate_version, App, AppSettings, Arg};

use input::Input;

// copied from mknImage/main.rs
// TODO: use the log crate or something cleaner
static mut DEBUG_ENABLED: bool = false;
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

fn do_swdl(url: &str) -> Result<()> {
    let mut rx = Input::new(url)?;
    std::io::copy(&mut rx, &mut std::io::stdout())?;
    Ok(())
}

fn main() {
    #[rustfmt::skip]
    let args = App::new("newbs-swdl")
        .version(crate_version!())
        .about("RPi Software Download")
        .max_term_width(100)
        .global_setting(AppSettings::ColoredHelp)
        .arg(
            Arg::with_name("debug")
                .short("D")
                .long("debug")
                .help("Enable extra debug output")
        )
        .arg(
            Arg::with_name("url")
                .required(true)
                .value_name("IMAGE FILE/URL")
                .help("Image to download. Can be a local file path, URL, or '-' for stdin"),
        )
        .get_matches();

    unsafe {
        DEBUG_ENABLED = args.is_present("debug");
    }
    debug!("debug logging enabled");

    if let Err(err) = do_swdl(args.value_of("url").unwrap()) {
        if !err.to_string().is_empty() {
            eprintln!("Error: {:#}", err);
        }
        exit(1);
    }
}
