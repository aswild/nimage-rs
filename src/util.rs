/*!
 * nimage utilities
 * Copyright 2020 Allen Wild
 * SPDX-License-Identifier: GPL-3.0-or-later
 */

use std::fs::File;
use std::io::{self, BufRead, BufReader, Read, Stdin};
use std::path::Path;

pub use clap::ArgMatches;

pub type CmdResult = Result<(), String>;
pub type CmdHandler = fn(&ArgMatches) -> CmdResult;

/**
 * An Input stream which implements Read and BufRead an can either be stdin
 * or a file opened for reading.
 */
pub enum Input {
    Stdin(BufReader<Stdin>),
    File(BufReader<File>),
}

impl Input {
    /**
     * Open a file for buffered reading, or stdin if name is "-"
     */
    pub fn open_file_or_stdin(name: &str) -> Result<Self, String> {
        if name == "-" {
            Ok(Self::Stdin(BufReader::new(io::stdin())))
        } else {
            let path = Path::new(name);
            match File::open(&path) {
                Ok(f) => Ok(Self::File(BufReader::new(f))),
                Err(err) => Err(format!("failed to open '{}' for reading: {}", name, err)),
            }
        }
    }

    /**
     * Check whether this Input object represents a file (return true),
     * or stdin (return false).
     */
    pub fn is_file(&self) -> bool {
        match self {
            Self::Stdin(_) => false,
            Self::File(_) => true,
        }
    }
}

impl Read for Input {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self {
            Self::Stdin(s) => s.read(buf),
            Self::File(f) => f.read(buf),
        }
    }
}

impl BufRead for Input {
    fn fill_buf(&mut self) -> io::Result<&[u8]> {
        match self {
            Self::Stdin(s) => s.fill_buf(),
            Self::File(f) => f.fill_buf(),
        }
    }

    fn consume(&mut self, amt: usize) {
        match self {
            Self::Stdin(s) => s.consume(amt),
            Self::File(f) => f.consume(amt),
        };
    }
}
