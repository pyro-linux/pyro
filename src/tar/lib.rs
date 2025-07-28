use clap::Parser;
use std::{
	fs::{File, create_dir_all},
	io::{Cursor, Read, Seek},
};

#[derive(Parser)]
#[command(version, about, long_about = None)]
pub struct Args {
	/// Extract files from the archive
	#[clap(short, long)]
	extract: bool,

	/// List files in the archive
	#[clap(short, long)]
	list: bool,

	#[clap(short, long)]
	verbose: bool,

	/// Path to the archive file
	file: String,

	/// Output directory for extraction
	#[clap(short, long)]
	output: Option<String>,

	/// Number of path components to strip from extracted files
	#[clap(long, default_value = "0")]
	strip_components: usize,
}

pub fn smoltar_main(args: &Args) -> std::io::Result<()> {
	// usage:
	// smoltar -x archive.tar.zst outdir
	// smoltar -l archive.tar.zst
	if !args.extract && !args.list {
		println!("Usage: smoltar -x archive.tar.zst outdir");
		println!("Usage: smoltar -l archive.tar.zst");
		return Err(std::io::Error::new(
			std::io::ErrorKind::InvalidInput,
			"Invalid arguments",
		));
	};

	let input: Box<dyn Read> = {
		let (buffer, mut reader): (Option<Vec<u8>>, Box<dyn Read>);

		if args.file == "-" {
			let mut buf = Vec::new();
			std::io::stdin().read_to_end(&mut buf)?;
			reader = Box::new(Cursor::new(buf.clone()));
			buffer = Some(buf);
		} else {
			reader = Box::new(File::open(&args.file)?);
			buffer = None;
		}

		// check if zstd or zlib
		let mut magic = [0; 4];
		{
			let mut peek_reader = reader.by_ref().take(4);
			peek_reader.read_exact(&mut magic)?;
		}

		// Rewind the reader if possible, or recreate it if from buffer
		let full_reader: Box<dyn Read> = if args.file == "-" {
			Box::new(Cursor::new(buffer.as_ref().unwrap().clone()))
		} else {
			let mut file = File::open(&args.file)?;
			file.seek(std::io::SeekFrom::Start(0))?;
			Box::new(file)
		};

		let is_zstd = magic == [0x28, 0xb5, 0x2f, 0xfd];
		let is_gzip = magic == [0x1f, 0x8b, 0x08, 0x00];

		if is_zstd {
			let decoder = ruzstd::decoding::StreamingDecoder::new(full_reader)
				.map_err(|e| {
					std::io::Error::new(std::io::ErrorKind::InvalidData, e)
				})?;
			Box::new(decoder)
		} else if is_gzip {
			let decoder = flate2::read::GzDecoder::new(full_reader);
			Box::new(decoder)
		} else {
			full_reader
		}
	};
	let mut archive = tar::Archive::new(input);

	if args.extract {
		let output_directory = if let Some(out) = &args.output {
			std::fs::canonicalize(out)
				.map(|p| p.to_string_lossy().to_string())
				.unwrap_or_else(|_| out.to_string())
		} else {
			std::env::current_dir()?.to_string_lossy().to_string()
		};

		create_dir_all(&output_directory)?;

		for entry in archive.entries()? {
			let entry = entry?;
			if let Ok(path) = entry.path() {
				if let Some(stripped) =
					strip_path_components(&path, args.strip_components)
				{
					let outpath = std::path::PathBuf::from(&output_directory)
						.join(stripped);
					if entry.header().entry_type().is_dir() {
						create_dir_all(&outpath)?;
					} else if let Some(parent) = outpath.parent() {
						create_dir_all(parent)?;
					}
				}
			}
		}
		archive.unpack(&output_directory)?;
	} else if args.list {
		archive
			.entries()?
			.filter_map(Result::ok)
			.filter_map(|file| file.path().ok().map(|p| p.to_path_buf()))
			.filter(|path| {
				path.file_name()
					.is_none_or(|name| name != "pax_global_header")
			})
			.for_each(|path| println!("{}", path.display()));
	}

	Ok(())
}

fn strip_path_components(
	path: &std::path::Path,
	n: usize,
) -> Option<std::path::PathBuf> {
	let comps: Vec<_> = path.components().skip(n).collect();
	if comps.is_empty() {
		None
	} else {
		Some(comps.iter().collect())
	}
}
