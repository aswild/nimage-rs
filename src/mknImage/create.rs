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
use std::path::{Path, PathBuf};

use nimage::format::*;
use nimage::util::WriteHelper;
use nimage::xxhio;

use crate::*;

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

    let comp = match words.next() {
        Some(s) => {
            CompMode::try_from(s).map_err(|_| anyhow!("unrecognized compression mode '{}'", s))?
        }
        None => CompMode::None,
    };

    if let Some(_) = words.next() {
        return Err(anyhow!("trailing colon-delimited fields"));
    }

    Ok(PartInput { filename, ptype, comp })
}

fn add_part(output: &mut Output, header: &mut ImageHeader, pinput: &PartInput) -> CmdResult {
    const ALIGN: u64 = 16;
    let inpath = Path::new(pinput.filename);
    let infile = File::open(&inpath)
        .with_context(|| format!("Unable to open '{}' for reading", pinput.filename))?;

    let mut reader = xxhio::Reader::new(BufReader::new(infile));
    debug!("Opened part input file '{}'", pinput.filename);
    let offset = output.count;
    debug!("start writing output at offset {}", offset);
    io::copy(&mut reader, output)?;

    let size = reader.total_len();
    let xxh = reader.hash();
    let pheader = PartHeader { size, offset, ptype: pinput.ptype, comp: pinput.comp, xxh };
    debug!("Created PartHeader {:?}", pheader);

    // note: the number of spaces here should match PartHeader::print_to() for alignment
    println!("Part {}\n  file:        {}", header.parts.len(), pinput.filename);
    pheader.print_to(&mut io::stdout(), 2).unwrap_or(());

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

    println!("Creating image {}", output_path);
    println!("Image name is '{}'", image_name);

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
