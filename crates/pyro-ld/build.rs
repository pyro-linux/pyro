use std::env;
use std::path::PathBuf;

fn main() {
	// Add linker arguments for no_std binary
	println!("cargo:rustc-link-arg=-nostartfiles");
	println!("cargo:rustc-link-arg=-nodefaultlibs");
	println!("cargo:rustc-link-arg=-static");

	let crate_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
	let out_path = PathBuf::from(&crate_dir).join("include");

	std::fs::create_dir_all(&out_path).ok();

	cbindgen::Builder::new()
		.with_crate(crate_dir)
		.with_language(cbindgen::Language::C)
		.generate()
		.expect("Unable to generate bindings")
		.write_to_file(out_path.join("pyro-ld.h"));
}
