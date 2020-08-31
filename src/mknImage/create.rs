/*!
 * mknImage: a tool to work with files in the nImage format.
 * handler for the check subcommand.
 *
 * Copyright 2020 Allen Wild
 * SPDX-License-Identifier: GPL-3.0-or-later
 */

use std::convert::TryFrom;
use std::fs::{self, File};
use std::io::prelude::*;
use std::io::{self, BufReader, SeekFrom};
use std::path::PathBuf;

use anyhow::{anyhow, Context, Result};
use clap::ArgMatches;
use yall::log_macros::*;
use zstd::stream::read::Encoder as ZstdReadEncoder;

use nimage::format::*;
use nimage::util::WriteHelper;
use nimage::xxhio;

use crate::CmdResult;

#[derive(Debug)]
struct Output {
    path: PathBuf,
    file: File,
    finished: bool,
    pub count: u64,
}

impl Output {
    pub fn new(filename: &str) -> io::Result<Self> {
        let path = PathBuf::from(filename);
        let file = File::create(&path)?;
        Ok(Output { path, file, finished: false, count: 0 })
    }

    pub fn finish(&mut self) {
        self.finished = true;
    }
}

impl Write for Output {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let ret = self.file.write(buf);
        if let Ok(count) = ret {
            self.count += count as u64;
        }
        ret
    }

    fn flush(&mut self) -> io::Result<()> {
        self.file.flush()
    }
}

impl Seek for Output {
    fn seek(&mut self, from: SeekFrom) -> io::Result<u64> {
        self.file.seek(from)
    }
}

impl Drop for Output {
    fn drop(&mut self) {
        if !self.finished {
            // Remove the output path if it we haven't been flaged as finished, this is to clean up
            // incomplete files on error. Rust calls this drop before the file is actually closed,
            // but on Linux that's fine - we can unlink() files that are still open.
            debug!("Deleting {}", self.path.to_string_lossy());
            fs::remove_file(&self.path).unwrap_or(());
        }
    }
}

#[derive(Debug)]
struct PartInput<'a> {
    filename: &'a str,
    ptype: PartType,
    comp: CompMode,
    auto_comp: Option<i32>,
}

fn parse_input(arg: &str) -> Result<PartInput> {
    // parse the format FILE:TYPE[:COMPRESSION] and validate that
    //   1) FILE isn't an empty string
    //   2) TYPE is a valid type
    //   3) COMPRESSION, if specified is valid, if unspecified is CompMode::None
    //   4) there's no trailing colon-separated items
    // A side effect of this format is that FILE can't contain any ':' characters because
    // they'll be mistaken for field separators.
    let mut words = arg.split(':');

    let filename = match words.next() {
        Some("") => return Err(anyhow!("empty filename")),
        Some(s) => s,
        None => panic!(), // str.split() always returns at least one thing
    };

    let ptype = match words.next() {
        Some(s) => PartType::try_from(s).map_err(|_| anyhow!("unrecognized part type '{}'", s))?,
        None => return Err(anyhow!("missing part type")),
    };

    let (comp, auto_comp) = match words.next() {
        Some(s) => {
            let mut compwords = s.splitn(2, '+');
            let typestr = compwords.next().unwrap();
            let comp = CompMode::try_from(typestr)
                .map_err(|_| anyhow!("unrecognized compression mode '{}'", typestr))?;

            if comp == CompMode::Zstd {
                let auto_comp = match compwords.next() {
                    Some("") => Some(15i32), // default zstd level
                    Some(x) => {
                        let level = x
                            .parse::<i32>()
                            .map_err(|_| anyhow!("bad zstd compression level '{}'", x))?;
                        Some(level)
                    }
                    None => None,
                };
                (comp, auto_comp)
            } else {
                if compwords.next().is_some() {
                    warn!("ignoring auto-compression specifier on non-zstd part");
                }
                (comp, None)
            }
        }
        None => (CompMode::None, None),
    };

    if words.next().is_some() {
        return Err(anyhow!("trailing colon-delimited fields"));
    }

    Ok(PartInput { filename, ptype, comp, auto_comp })
}

fn add_part(output: &mut Output, header: &mut ImageHeader, pinput: &PartInput) -> CmdResult {
    const ALIGN: u64 = 16;
    let infile = File::open(pinput.filename)
        .with_context(|| format!("Unable to open '{}' for reading", pinput.filename))?;

    let mut reader = match pinput.auto_comp {
        Some(level) => {
            debug!("compressing part '{}' with zstd level {}", pinput.filename, level);
            let mut zenc = ZstdReadEncoder::new(BufReader::new(infile), level)?;
            // try to enable multithreading, but ignore errors if it doesn't work
            let _ = zenc.multithread(num_cpus::get() as u32);
            xxhio::Reader::new(zenc)
        }
        None => xxhio::Reader::new(BufReader::new(infile)),
    };

    debug!("Opened part input file '{}'", pinput.filename);
    let offset = output.count;
    debug!("start writing output at offset {}", offset);
    io::copy(&mut reader, output)?;

    let size = reader.total_len();
    let xxh = reader.hash();
    let pheader = PartHeader { size, offset, ptype: pinput.ptype, comp: pinput.comp, xxh };
    debug!("Created PartHeader {:?}", pheader);

    let mut pheader_str = Vec::<u8>::new();
    pheader.print_to(&mut pheader_str, 2).unwrap();
    // note: the number of spaces here should match PartHeader::print_to() for alignment
    info!(
        "Part {}\n  file:        {}\n{}",
        header.parts.len(),
        pinput.filename,
        std::str::from_utf8(&pheader_str).unwrap()
    );

    let padding = (ALIGN - (size % ALIGN)) % ALIGN;
    if padding > 0 {
        debug!("Writing {} bytes of padding", padding);
        output.write_zeros(padding as usize)?;
    }

    header.parts.push(pheader);
    Ok(())
}

pub fn cmd_create(args: &ArgMatches) -> CmdResult {
    let image_name = args.value_of("name").unwrap_or("");
    let output_path = args.value_of("output").unwrap();

    let mut input_parts = Vec::<PartInput>::new();
    for arg in args.values_of("parts").unwrap() {
        let part = parse_input(arg).with_context(|| format!("invalid part '{}'", arg))?;
        debug!("parsed input part {:?}", part);
        input_parts.push(part);
    }

    info!("Creating image {}", output_path);
    info!("Image name is '{}'", image_name);

    // input is parsed, open the output file
    let mut output = Output::new(&output_path)
        .with_context(|| format!("unable to open '{}' for writing", output_path))?;

    // write header placeholder, then reset the write count to calculate correct offsets
    output.write_zeros(NIMG_HDR_SIZE)?;
    output.count = 0;

    let mut header = ImageHeader::new(image_name);
    for part in input_parts.iter() {
        add_part(&mut output, &mut header, part)?;
    }

    // seek back to the beginning and write the real header
    output.seek(SeekFrom::Start(0)).context("Failed to seek output file")?;
    header.write_to(&mut output).context("Failed to write image header")?;

    // success, don't delete the output file when we return
    output.finish();
    Ok(())
}
