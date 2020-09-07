/*!
 * swdl: Raspberry Pi firmware update engine.
 * logic to program nImage parts to flash
 *
 * Copyright 2020 Allen Wild
 * SPDX-License-Identifier: GPL-3.0-or-later
 */

use std::cmp::min;
use std::fs::OpenOptions;
use std::io::{Read, Write};
use std::path::Path;

use anyhow::{anyhow, Context, Result};
use indicatif::{ProgressBar, ProgressStyle};
use yall::log_macros::*;
use zstd::stream::write::Decoder as ZstdWriteDecoder;

use nimage::format::*;
use nimage::xxhio;

//use crate::flashbanks::*;
use crate::input::Input;

const BLOCK_SIZE: usize = 128 * 1024;

fn make_progress_bar(size: u64) -> ProgressBar {
    let pb = ProgressBar::new(size);
    pb.set_style(ProgressStyle::default_bar().template("{spinner} {bar:80} {bytes}/{total_bytes}"));
    pb
}

fn program_raw<P: AsRef<Path>>(
    input: &mut Input,
    dest: P,
    part: &PartHeader,
    progress: &ProgressBar,
) -> Result<()> {
    if part.comp == CompMode::None {
        info!("Programming part {}", part.ptype);
    } else {
        info!("Programming part {} compressed with {}", part.ptype, part.comp);
    }

    // open output with the equivalent of open(dest, O_WRONLY), without O_TRUNC or O_CREAT
    let outfile = OpenOptions::new().write(true).open(&dest).with_context(|| {
        format!("failed to open output '{}' for writing", dest.as_ref().to_string_lossy())
    })?;

    // we have to create and wrap the output in two steps because Box::<dyn Write>::new(...) fails
    // with weird error messages.
    // The xxhio Writer is outside of the decompressor so that the hash is computed against the
    // compressed data rather than the uncompressed data.
    let out: Box<dyn Write> = match part.comp {
        CompMode::None => Box::new(outfile),
        CompMode::Zstd => Box::new(
            ZstdWriteDecoder::new(outfile).context("failed to initialized zstd compressor")?,
        ),
        CompMode::LibArchive => return Err(anyhow!("part comp mode {} is unsupported", part.comp)),
    };
    let mut out = xxhio::Writer::new(out);

    // do the data copy
    let mut buf = vec![0u8; BLOCK_SIZE];
    let mut total = 0;
    while total < part.size {
        let to_read = min(BLOCK_SIZE, (part.size - total) as usize);
        let count = match input.read(&mut buf[..to_read]) {
            Ok(0) => return Err(anyhow!("EOF after reading only {}/{} bytes", total, part.size)),
            Ok(c) => c,
            Err(e) => return Err(e).context("failed to read input"),
        };

        out.write_all(&buf[..count])?;
        total += count as u64;
        progress.set_position(total);
        std::thread::sleep(std::time::Duration::from_millis(1)); // FIXME: artifical slowness
    }

    let hash = out.hash();
    if hash != part.xxh {
        return Err(anyhow!("xxHash mismatch! Expected 0x{:08X} got 0x{:08X}", part.xxh, hash));
    }

    Ok(())
}

pub fn program_part(input: &mut Input, part: &PartHeader) -> Result<()> {
    // set up the progress bar here so that we can control what happens if the inner function fails
    // in the middle of writing.
    let progress = make_progress_bar(part.size);

    let ret = match part.ptype {
        PartType::BootImg | PartType::Rootfs | PartType::RootfsRw => {
            // FIXME: write images somewhere useful
            program_raw(input, "/dev/null", part, &progress)
        }
        PartType::BootTar | PartType::Invalid => {
            // FIXME: actually implement tar part types
            Err(anyhow!("unsupported part type {}", part.ptype))
        }
    };

    // finish the progress bar after the inner function fails, leave its position as-is if it
    // failed in the middle of writing.
    progress.finish_at_current_pos();
    ret
}
