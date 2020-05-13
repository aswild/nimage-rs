/*!
 * nimage utilities
 * Copyright 2020 Allen Wild
 * SPDX-License-Identifier: GPL-3.0-or-later
 */

use clap::ArgMatches;

/**
 * represents a command handler, because I can't figure out how to make a table
 * of "function pointers" or FnOnce traits for some reason.
 */
pub trait CmdHandler {
    fn run(&self, args: &ArgMatches) -> Result<(), String>;
}
