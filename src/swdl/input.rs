/*!
 * swdl: Raspberry Pi firmware update engine.
 * input reader
 *
 * Copyright 2020 Allen Wild
 * SPDX-License-Identifier: GPL-3.0-or-later
 */

use std::fmt;
use std::fs::File;
use std::io::{self, BufReader, Read};
use std::os::unix::process::ExitStatusExt;
use std::process::{Child, Command, Stdio};

use anyhow::Result;
use yall::log_macros::*;

#[derive(Debug)]
pub struct FileInfo {
    // path as a string for easier printing. Technically should be a PathBuf
    path: String,
    // bufreader of the open file object
    file: BufReader<File>,
}

#[derive(Debug)]
pub struct CurlInfo {
    url: String,
    child: Child,
}

#[derive(Debug)]
pub enum Input {
    Stdin(BufReader<io::Stdin>),
    File(FileInfo),
    Curl(CurlInfo),
}

impl Input {
    pub fn new(path: &str) -> Result<Self> {
        // easy case, reading stdin
        if path == "-" {
            debug!("opening stdin");
            return Ok(Input::Stdin(BufReader::new(io::stdin())));
        }

        // try to open path as a local file
        match File::open(path) {
            // sucess
            Ok(file) => {
                debug!("opened {} as a local file", path);
                Ok(Input::File(FileInfo { path: path.to_string(), file: BufReader::new(file) }))
            }

            // couldn't open as a file, do it as a piped curl command
            Err(_) => {
                let child = Command::new("curl")
                    .arg("-sSLf")
                    .arg("--netrc")
                    .arg("--")
                    .arg(path)
                    .stdout(Stdio::piped())
                    .spawn()?;
                debug!("downloading {} with curl", path);
                Ok(Input::Curl(CurlInfo { url: path.to_string(), child }))
            }
        }
    }
}

impl fmt::Display for Input {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(match self {
            Input::Stdin(_) => "[standard input]",
            Input::File(info) => &info.path,
            Input::Curl(info) => &info.url,
        })
    }
}

impl Read for Input {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self {
            // stdin and a local file are easy
            Input::Stdin(r) => r.read(buf),
            Input::File(info) => info.file.read(buf),

            // curl pipe is trickier
            Input::Curl(info) => {
                // try the read, immediately returning any read error
                let count = info.child.stdout.as_mut().unwrap().read(buf)?;
                if count != 0 {
                    // we read some bytes
                    return Ok(count);
                }

                // we read nothing, which means curl is done, check its return status.
                // wait() could return an error but that shouldn't happen
                let status = info.child.wait().expect("failed to wait for curl process");
                if status.success() {
                    Ok(0)
                } else if let Some(code) = status.code() {
                    // normal non-successful exit
                    Err(io::Error::new(
                        io::ErrorKind::Other,
                        format!("curl process exited with status {}", code),
                    ))
                } else if let Some(sig) = status.signal() {
                    // killed by a signal
                    Err(io::Error::new(
                        io::ErrorKind::Other,
                        format!("curl process killed with signal {}", sig),
                    ))
                } else {
                    // should never get here
                    panic!("curl process exited in an unknown fashion!")
                }
            }
        }
    }
}
