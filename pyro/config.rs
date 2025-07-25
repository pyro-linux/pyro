use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

/// Nix-like configuration for declarative package management
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PyroConfig {
	/// System packages to be installed
	pub system_packages: Vec<PackageSpec>,
	/// User packages
	pub user_packages: Vec<PackageSpec>,
	/// Build configuration
	pub build_config: BuildConfig,
	/// Store configuration
	pub store_config: StoreConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackageSpec {
	pub name: String,
	pub version: Option<String>,
	pub source: PackageSource,
	pub build_inputs: Vec<String>,
	pub runtime_inputs: Vec<String>,
	pub environment: HashMap<String, String>,
	pub build_script: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PackageSource {
	Crates { name: String, version: String },
	Git { url: String, rev: Option<String> },
	Path { path: PathBuf },
	Url { url: String, hash: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildConfig {
	pub max_jobs: usize,
	pub sandbox: bool,
	pub cache_dir: PathBuf,
	pub system_packages: bool,
	pub cross_compile: Option<String>,
	pub toolchain_path: Option<PathBuf>,
	pub sysroot: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoreConfig {
	pub store_path: PathBuf,
	pub gc_roots: Vec<PathBuf>,
	pub auto_gc: bool,
	pub max_store_size: Option<u64>,
}

impl Default for PyroConfig {
	fn default() -> Self {
		Self {
			system_packages: vec![],
			user_packages: vec![],
			build_config: BuildConfig {
				max_jobs: std::thread::available_parallelism()
					.map_or(1, |n| n.get()),
				sandbox: true,
				cache_dir: PathBuf::from(".pyro/cache"),
				system_packages: false,
				cross_compile: None,
				toolchain_path: None,
				sysroot: None,
			},
			store_config: StoreConfig {
				store_path: PathBuf::from("/nix/store"),
				gc_roots: vec![],
				auto_gc: false,
				max_store_size: None,
			},
		}
	}
}

impl PyroConfig {
	pub fn from_file(
		path: &PathBuf,
	) -> Result<Self, Box<dyn std::error::Error>> {
		let content = std::fs::read_to_string(path)?;
		let config: PyroConfig = toml::from_str(&content)?;
		Ok(config)
	}

	pub fn to_file(
		&self,
		path: &PathBuf,
	) -> Result<(), Box<dyn std::error::Error>> {
		let content = toml::to_string_pretty(self)?;
		std::fs::write(path, content)?;
		Ok(())
	}
}
