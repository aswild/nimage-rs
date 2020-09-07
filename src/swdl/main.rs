/*!
 * swdl: Raspberry Pi firmware update engine.
 * main executable
 *
 * Copyright 2020 Allen Wild
 * SPDX-License-Identifier: GPL-3.0-or-later
 */

mod flashbanks;
mod input;
mod program;

use std::io::Read;
use std::process::exit;

use anyhow::{anyhow, Context, Result};
use clap::{crate_version, App, AppSettings, Arg};
use yall::{log_macros::*, Logger};

use nimage::format::*;

use input::Input;
use program::program_part;

#[allow(clippy::comparison_chain)] // suppress lint on the "if part.offset < current_offset"
fn do_swdl(url: &str) -> Result<()> {
    let mut input = Input::new(url)?;
    let mut header = [0u8; NIMG_HDR_SIZE];
    input.read_exact(&mut header).context("failed to read image header")?;
    let header = ImageHeader::from_bytes(&header).context("failed to parse image header")?;
    info!("Image name is {}", if header.name.is_empty() { "empty" } else { &header.name });

    if header.parts.is_empty() {
        warn!("image is empty, nothing to do");
        return Ok(());
    }

    let mut current_offset = 0u64;
    for (i, part) in header.parts.iter().enumerate() {
        if part.offset < current_offset {
            return Err(anyhow!("Part {} offset {} is out of order", i, part.offset));
        } else if part.offset > current_offset {
            let pad_bytes = part.offset - current_offset;
            let mut padding = vec![0u8; pad_bytes as usize];
            input
                .read_exact(&mut padding)
                .with_context(|| format!("failed to read padding before part {}", i))?;
            current_offset += pad_bytes;
            debug!("read {} bytes of padding", pad_bytes);
        }

        program_part(&mut input, part)?;
        current_offset += part.size;
    }

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
