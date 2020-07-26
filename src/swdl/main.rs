/*!
 * swdl: Raspberry Pi firmware update engine.
 * main executable
 *
 * Copyright 2020 Allen Wild
 * SPDX-License-Identifier: GPL-3.0-or-later
 */

mod flashbanks;
mod input;

use std::process::exit;

use anyhow::Result;
use clap::{crate_version, App, AppSettings, Arg};
use yall::{log_macros::*, Logger};

use input::Input;

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

    Logger::with_verbosity(3 + args.occurrences_of("debug")).init();
    debug!("debug logging enabled");

    if let Err(err) = do_swdl(args.value_of("url").unwrap()) {
        error!("{:#}", err);
        exit(1);
    }
}
