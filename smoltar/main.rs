use std::io;

use smoltar::smoltar_main;

pub fn main() -> io::Result<()> {
	let args: Vec<String> = std::env::args().collect();
	smoltar_main(&mut args.into_iter())
}
