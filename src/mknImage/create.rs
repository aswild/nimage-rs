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

use nimage::crc32::Reader as CrcReader;
use nimage::format::*;
use nimage::util::WriteHelper;

use crate::*;

#[derive(Debug)]
struct PartInput<'a>(PartType, &'a str);

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

fn parse_type_file(arg: &str) -> Result<PartInput, String> {
    let colon = match arg.find(':') {
        Some(x) => x,
        None => return Err(format!("invalid part argument '{}': missing ':'", arg)),
    };

    let (type_str, filename) = arg.split_at(colon);
    let ptype = PartType::try_from(type_str)
        .map_err(|_| format!("invalid part argument '{}': invalid part type", arg))?;
    if filename.len() < 2 {
        // filename will contain the colon, make sure there's more stuff too
        return Err(format!("invalid part argument '{}': missing filename", arg));
    }
    Ok(PartInput(ptype, &filename[1..]))
}

fn add_part(output: &mut Output, header: &mut ImageHeader, pinput: &PartInput) -> CmdResult {
    const ALIGN: u64 = 16;
    let inpath = Path::new(pinput.1);
    let infile = File::open(&inpath)
        .map_err(|e| format!("Unable to open '{}' for reading: {}", pinput.1, e))?;

    let mut reader = CrcReader::new(BufReader::new(infile));
    debug!("Opened part input file '{}'", pinput.1);
    let offset = output.count;
    debug!("start writing output at offset {}", offset);
    io::copy(&mut reader, output)?;

    let size = reader.total_bytes();
    let crc = reader.sum();
    let pheader = PartHeader { size, offset, ptype: pinput.0, crc };
    debug!("Created PartHeader {:?}", pheader);

    println!("Part {}\n  file:   {}", header.parts.len(), pinput.1);
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

    let mut input_parts: Vec<PartInput> = vec![];
    for arg in args.values_of("inputs").unwrap() {
        let part = parse_type_file(arg)?;
        debug!("parsed input part {:?}", part);
        input_parts.push(part);
    }

    println!("Creating image {}", output_path);
    println!("Image name is '{}'", image_name);

    // input is parsed, open the output file
    let mut output = Output::new(&output_path)
        .map_err(|e| format!("unable to open '{}' for writing: {}", output_path, e))?;

    // write header placeholder, then reset the write count to calculate correct offsets
    output.write_zeros(NIMG_HDR_SIZE)?;
    output.count = 0;

    let mut header = ImageHeader::new(image_name);
    for part in input_parts.iter() {
        add_part(&mut output, &mut header, part)?;
    }

    // seek back to the beginning and write the real header
    output.seek(SeekFrom::Start(0)).map_err(|e| format!("Failed to seek output file: {}", e))?;
    header.write_to(&mut output).map_err(|e| format!("Failed to write image header: {}", e))?;

    // success, don't delete the output file when we return
    output.finish();
    Ok(())
}
