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
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;

use anyhow::{anyhow, Context, Result};
use indicatif::{ProgressBar, ProgressStyle};
use yall::log_macros::*;
use zstd::stream::write::Decoder as ZstdWriteDecoder;

use nimage::format::*;
use nimage::util::human_size;
use nimage::xxhio;

use crate::flashbanks::raw_dest_path;
use crate::input::Input;

const BLOCK_SIZE: usize = 256 * 1024;

/// Write wrapper that counts the number of bytes written in an external u64.
/// Slightly hacky, but works around the fact that this will get hidden somewhere
/// in a Box<dyn Write>.
struct CountWriter<'a, W> {
    inner: W,
    count: &'a mut u64,
}

impl<'a, W> CountWriter<'a, W> {
    pub fn new(inner: W, count: &'a mut u64) -> Self {
        Self { inner, count }
    }
}

impl<'a, W: Write> Write for CountWriter<'a, W> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let count = self.inner.write(buf)?;
        *self.count += count as u64;
        Ok(count)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}

fn make_progress_bar(size: u64) -> ProgressBar {
    let pb = ProgressBar::new(size);
    pb.set_style(ProgressStyle::default_bar().template("{spinner} {bar:80} {bytes}/{total_bytes}"));
    pb
}

/// program a raw partition nImage part.
/// Returns the number of bytes written to disk (after decompression, if applicable)
fn program_raw<P: AsRef<Path>>(
    input: &mut Input,
    dest: P,
    part: &PartHeader,
    progress: &ProgressBar,
) -> Result<u64> {
    if part.comp == CompMode::None {
        info!("Programming part {}", part.ptype);
    } else {
        info!("Programming part {} compressed with {}", part.ptype, part.comp);
    }

    let dest_string = dest.as_ref().to_string_lossy();
    info!("Writing to {}", dest_string);

    // open output with the equivalent of open(dest, O_WRONLY | O_SYNC), without O_TRUNC or O_CREAT
    // Using O_SYNC is slightly slower, but it makes the progress bar smoother and eliminates the
    // big fsync delay when the outfile's file descriptor is closed. We write in pretty big chunks
    // so the extra overhead is measurable but small, on the order of a second or two.
    let outfile = OpenOptions::new()
        .write(true)
        .custom_flags(libc::O_SYNC)
        .open(&dest)
        .with_context(|| format!("failed to open output '{}' for writing", dest_string))?;

    // count how many bytes we wrote to disk (after decompression)
    let mut out_count = 0;
    let outfile = CountWriter::new(outfile, &mut out_count);

    // we have to create and wrap the output in two steps because Box::<dyn Write>::new(...) fails
    // with weird error messages.
    // The xxhio Writer is outside of the decompressor so that the hash is computed against the
    // compressed data rather than the uncompressed data.
    // TODO: refactor this with a explicit type that more gracefully handles keeping track of the
    // xxHash and the decompressed bytes written.
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
    }

    let hash = out.hash();
    if hash != part.xxh {
        return Err(anyhow!("xxHash mismatch! Expected 0x{:08X} got 0x{:08X}", part.xxh, hash));
    }

    // out owns outfile, which contains an exclusive reference to out_count.
    // Drop it so we can use out_count again.
    std::mem::drop(out);
    Ok(out_count)
}

pub fn program_part(input: &mut Input, part: &PartHeader) -> Result<()> {
    // set up the progress bar here so that we can control what happens if the inner function fails
    // in the middle of writing.
    let progress = make_progress_bar(part.size);

    let ret = match part.ptype {
        PartType::BootImg | PartType::Rootfs | PartType::RootfsRw => {
            // FIXME: unmount and remount /boot, or at least check that /boot isn't mounted
            let dest_path = raw_dest_path(part.ptype)?;
            program_raw(input, dest_path, part, &progress)
        }
        PartType::BootTar | PartType::Invalid => {
            // FIXME: actually implement tar part types
            Err(anyhow!("unsupported part type {}", part.ptype))
        }
    };

    // finish the progress bar after the inner function fails, leave its position as-is if it
    // failed in the middle of writing.
    progress.finish_at_current_pos();

    match ret {
        Ok(written) => {
            info!("Read: {}, Wrote: {}", human_size(part.size), human_size(written));
            Ok(())
        }
        Err(err) => Err(err),
    }
}
