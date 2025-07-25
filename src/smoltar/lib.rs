use std::fs::{create_dir_all, File};

pub fn smoltar_main(
	args: &mut impl Iterator<Item = String>,
) -> std::io::Result<()> {
	// usage:
	// smoltar -x archive.tar.zst outdir
	// smoltar -l archive.tar.zst
	let mut mode = None;
	let mut archive_path = None;
	let mut outdir = None;

	// Skip program name and parse arguments using a loop
	for (i, arg) in args.enumerate() {
		match i {
			0 => mode = Some(arg),
			1 => archive_path = Some(arg),
			2 => outdir = Some(arg),
			_ => break,
		}
	}

	let mode = match mode {
		Some(m) => m,
		None => {
			println!("Usage: smoltar -x archive.tar.zst outdir");
			println!("Usage: smoltar -l archive.tar.zst");
			return Err(std::io::Error::new(
				std::io::ErrorKind::InvalidInput,
				"Invalid arguments",
			));
		}
	};
	let archive_path = match archive_path {
		Some(p) => p,
		None => {
			println!("Usage: smoltar -x archive.tar.zst outdir");
			println!("Usage: smoltar -l archive.tar.zst");
			return Err(std::io::Error::new(
				std::io::ErrorKind::InvalidInput,
				"Invalid arguments",
			));
		}
	};

	let file = File::open(&archive_path)?;
	let unzipped = flate2::read::GzDecoder::new(file);
	let mut archive = tar::Archive::new(unzipped);

	match mode.as_str() {
		"-x" => {
			let outdir = match outdir {
				Some(d) => d,
				None => {
					println!("Usage: smoltar -x archive.tar.zst outdir");
					return Err(std::io::Error::new(
						std::io::ErrorKind::InvalidInput,
						"Missing output directory",
					));
				}
			};
			create_dir_all(&outdir)?;
			archive.unpack(&outdir)?;
		}
		"-l" => {
			archive
				.entries()?
				.filter_map(Result::ok)
				.filter_map(|file| file.path().ok().map(|p| p.to_path_buf()))
				.for_each(|path| println!("{}", path.display()));
		}
		_ => {
			println!("Usage: smoltar -x archive.tar.zst outdir");
			println!("Usage: smoltar -l archive.tar.zst");
		}
	}

	Ok(())
}
