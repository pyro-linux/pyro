use mlua::{Lua, LuaSerdeExt};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

/// Nix-like configuration for declarative package management
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PyroConfig {
	/// Build configuration
	pub build_config: BuildConfig,
	/// Store configuration
	pub store_config: StoreConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Package {
	pub name: String,
	pub version: String,
	pub source: PackageSource,
	pub build_inputs: Vec<String>,
	pub runtime_inputs: Vec<String>,
	pub environment: HashMap<String, String>,
	pub builder: String,
	pub args: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PackageSource {
	Crate { name: String, version: String },
	Git { url: String, rev: Option<String> },
	Path { path: PathBuf },
	Url { url: String, hash: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildConfig {
	pub max_jobs: usize,
	pub sandbox: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoreConfig {
	pub store_path: PathBuf,
	pub db_path: PathBuf,
	pub gc: GcSettings,
	pub max_store_size: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GcSettings {
	pub enabled: bool,
	pub remove_older_than: Option<String>,
}

impl Default for PyroConfig {
	fn default() -> Self {
		Self {
			build_config: BuildConfig {
				max_jobs: std::thread::available_parallelism()
					.map_or(1, |n| n.get()),
				sandbox: true,
			},
			store_config: StoreConfig {
				store_path: PathBuf::from("/pyro/store"),
				db_path: PathBuf::from("/pyro/database.sqlite"),
				gc: GcSettings {
					enabled: true,
					remove_older_than: Some("30d".to_string()),
				},
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
		let lua = Lua::new();
		let value = lua.load(content).eval()?;
		let config: PyroConfig = lua.from_value(value)?;
		Ok(config)
	}
}
