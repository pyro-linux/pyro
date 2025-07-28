use std::io;

use clap::Parser as _;
use smoltar::smoltar_main;

pub fn main() -> io::Result<()> {
	smoltar_main(&smoltar::Args::parse())
}
