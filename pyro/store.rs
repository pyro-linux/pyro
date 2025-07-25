use crate::config::{PackageSpec, StoreConfig};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::time::SystemTime;

/// Nix-like immutable package store
#[derive(Debug)]
pub struct PyroStore {
	config: StoreConfig,
	/// Maps package hash to store path
	store_db: HashMap<String, StorePath>,
}

#[derive(Debug, Clone)]
pub struct StorePath {
	pub hash: String,
	pub name: String,
	pub path: PathBuf,
	pub dependencies: Vec<String>,
	pub size: u64,
	pub created_at: SystemTime,
	pub last_accessed: SystemTime,
}

#[derive(Debug)]
pub struct BuildResult {
	pub store_path: StorePath,
	pub build_log: String,
	pub success: bool,
}

impl PyroStore {
	pub fn new(
		config: StoreConfig,
	) -> Result<Self, Box<dyn std::error::Error>> {
		fs::create_dir_all(&config.store_path)?;

		let mut store = PyroStore {
			config,
			store_db: HashMap::new(),
		};

		store.load_store_db()?;
		Ok(store)
	}

	/// Generate content-addressable hash for a package
	pub fn compute_package_hash(&self, spec: &PackageSpec) -> String {
		let mut hasher = Sha256::new();

		// Hash package specification for reproducibility
		hasher.update(spec.name.as_bytes());
		if let Some(version) = &spec.version {
			hasher.update(version.as_bytes());
		}

		// Hash source
		match &spec.source {
			crate::config::PackageSource::Crates { name, version } => {
				hasher.update(b"crates");
				hasher.update(name.as_bytes());
				hasher.update(version.as_bytes());
			}
			crate::config::PackageSource::Git { url, rev } => {
				hasher.update(b"git");
				hasher.update(url.as_bytes());
				if let Some(rev) = rev {
					hasher.update(rev.as_bytes());
				}
			}
			crate::config::PackageSource::Path { path } => {
				hasher.update(b"path");
				hasher.update(path.to_string_lossy().as_bytes());
			}
			crate::config::PackageSource::Url { url, hash } => {
				hasher.update(b"url");
				hasher.update(url.as_bytes());
				hasher.update(hash.as_bytes());
			}
		}

		// Hash dependencies
		for dep in &spec.build_inputs {
			hasher.update(dep.as_bytes());
		}
		for dep in &spec.runtime_inputs {
			hasher.update(dep.as_bytes());
		}

		// Hash environment
		let mut env_keys: Vec<_> = spec.environment.keys().collect();
		env_keys.sort();
		for key in env_keys {
			hasher.update(key.as_bytes());
			hasher.update(spec.environment[key].as_bytes());
		}

		format!("{:x}", hasher.finalize())[..32].to_string()
	}

	/// Get store path for a package
	pub fn get_store_path(&self, spec: &PackageSpec) -> PathBuf {
		let hash = self.compute_package_hash(spec);
		let name = format!("{}-{}", hash, spec.name);
		self.config.store_path.join(name)
	}

	/// Check if package exists in store
	pub fn package_exists(&self, spec: &PackageSpec) -> bool {
		let hash = self.compute_package_hash(spec);
		self.store_db.contains_key(&hash)
	}

	/// Add package to store
	pub fn add_package(
		&mut self,
		spec: &PackageSpec,
		build_result: BuildResult,
	) -> Result<(), Box<dyn std::error::Error>> {
		let hash = self.compute_package_hash(spec);

		if build_result.success {
			self.store_db.insert(hash, build_result.store_path);
			self.save_store_db()?;
		}

		Ok(())
	}

	/// Install a package to the store
	pub async fn install_package(
		&self,
		spec: &PackageSpec,
		build_output: &std::path::Path,
	) -> Result<String, Box<dyn std::error::Error>> {
		let package_hash = self.compute_package_hash(spec);
		let package_path = self.config.store_path.join(&package_hash);

		if package_path.exists() {
			return Ok(package_hash);
		}

		// Create package directory structure
		std::fs::create_dir_all(&package_path)?;

		let bin_path = package_path.join("bin");
		let lib_path = package_path.join("lib");
		let share_path = package_path.join("share");

		std::fs::create_dir_all(&bin_path)?;
		std::fs::create_dir_all(&lib_path)?;
		std::fs::create_dir_all(&share_path)?;

		// Copy build artifacts to appropriate directories
		self.install_build_artifacts(build_output, &package_path)
			.await?;

		// Store package metadata
		let metadata_path = package_path.join("metadata.json");
		let metadata = serde_json::to_string_pretty(spec)?;
		std::fs::write(metadata_path, metadata)?;

		// Create symlinks for binaries
		self.create_binary_symlinks(&package_hash, &bin_path)
			.await?;

		Ok(package_hash)
	}

	/// Install build artifacts to the package directory
	async fn install_build_artifacts(
		&self,
		build_output: &std::path::Path,
		package_path: &std::path::Path,
	) -> Result<(), Box<dyn std::error::Error>> {
		let bin_path = package_path.join("bin");
		let lib_path = package_path.join("lib");

		// Look for common build output patterns
		if let Ok(entries) = std::fs::read_dir(build_output) {
			for entry in entries {
				let entry = entry?;
				let path = entry.path();
				let file_name = path.file_name().unwrap().to_string_lossy();

				// Install executables
				if path.is_file() && self.is_executable(&path) {
					let dest = bin_path.join(&*file_name);
					std::fs::copy(&path, &dest)?;

					// Make executable on Unix systems
					#[cfg(unix)]
					{
						use std::os::unix::fs::PermissionsExt;
						let mut perms = std::fs::metadata(&dest)?.permissions();
						perms.set_mode(0o755);
						std::fs::set_permissions(&dest, perms)?;
					}
				}

				// Install libraries
				if path.is_file() && self.is_library(&path) {
					let dest = lib_path.join(&*file_name);
					std::fs::copy(&path, &dest)?;
				}
			}
		}

		// Look in target/release for Rust projects
		let target_release = build_output.join("target").join("release");
		if target_release.exists() {
			if let Ok(entries) = std::fs::read_dir(&target_release) {
				for entry in entries {
					let entry = entry?;
					let path = entry.path();

					if path.is_file() && self.is_executable(&path) {
						let file_name =
							path.file_name().unwrap().to_string_lossy();
						let dest = bin_path.join(&*file_name);
						std::fs::copy(&path, &dest)?;

						#[cfg(unix)]
						{
							use std::os::unix::fs::PermissionsExt;
							let mut perms =
								std::fs::metadata(&dest)?.permissions();
							perms.set_mode(0o755);
							std::fs::set_permissions(&dest, perms)?;
						}
					}
				}
			}
		}

		Ok(())
	}

	/// Check if a file is executable
	fn is_executable(&self, path: &std::path::Path) -> bool {
		if let Some(extension) = path.extension() {
			let ext = extension.to_string_lossy().to_lowercase();
			if ext == "exe" {
				return true;
			}
		}

		// On Unix, check if file has execute permissions
		#[cfg(unix)]
		{
			use std::os::unix::fs::PermissionsExt;
			if let Ok(metadata) = std::fs::metadata(path) {
				return metadata.permissions().mode() & 0o111 != 0;
			}
		}

		// On Windows, check common executable extensions
		#[cfg(windows)]
		{
			if let Some(extension) = path.extension() {
				let ext = extension.to_string_lossy().to_lowercase();
				return ext == "exe" || ext == "bat" || ext == "cmd";
			}
		}

		false
	}

	/// Check if a file is a library
	fn is_library(&self, path: &std::path::Path) -> bool {
		if let Some(extension) = path.extension() {
			let ext = extension.to_string_lossy().to_lowercase();
			return ext == "so"
				|| ext == "dylib"
				|| ext == "dll"
				|| ext == "a"
				|| ext == "lib";
		}
		false
	}

	/// Create symlinks for binaries in the global bin directory
	async fn create_binary_symlinks(
		&self,
		_package_hash: &str,
		bin_path: &std::path::Path,
	) -> Result<(), Box<dyn std::error::Error>> {
		let global_bin = self.config.store_path.join("bin");
		std::fs::create_dir_all(&global_bin)?;

		if let Ok(entries) = std::fs::read_dir(bin_path) {
			for entry in entries {
				let entry = entry?;
				let path = entry.path();

				if path.is_file() {
					let file_name = path.file_name().unwrap();
					let symlink_path = global_bin.join(file_name);

					// Remove existing symlink if it exists
					if symlink_path.exists() {
						std::fs::remove_file(&symlink_path)?;
					}

					// Create symlink
					#[cfg(unix)]
					{
						std::os::unix::fs::symlink(&path, &symlink_path)?;
					}

					#[cfg(windows)]
					{
						// On Windows, copy the file instead of creating a symlink
						std::fs::copy(&path, &symlink_path)?;
					}
				}
			}
		}

		Ok(())
	}

	/// Get package from store
	pub fn get_package(
		&mut self,
		spec: &PackageSpec,
	) -> Option<&mut StorePath> {
		let hash = self.compute_package_hash(spec);
		if let Some(store_path) = self.store_db.get_mut(&hash) {
			store_path.last_accessed = SystemTime::now();
			Some(store_path)
		} else {
			None
		}
	}

	/// Garbage collect unused packages
	pub fn garbage_collect(
		&mut self,
	) -> Result<Vec<String>, Box<dyn std::error::Error>> {
		let mut removed = Vec::new();
		let now = SystemTime::now();

		// Find packages not accessed in 30 days
		let mut to_remove = Vec::new();
		for (hash, store_path) in &self.store_db {
			if let Ok(duration) = now.duration_since(store_path.last_accessed) {
				if duration.as_secs() > 30 * 24 * 60 * 60 {
					// 30 days
					to_remove.push(hash.clone());
				}
			}
		}

		// Remove old packages
		for hash in to_remove {
			if let Some(store_path) = self.store_db.remove(&hash) {
				if store_path.path.exists() {
					fs::remove_dir_all(&store_path.path)?;
				}
				removed.push(store_path.name);
			}
		}

		self.save_store_db()?;
		Ok(removed)
	}

	/// Load store database from disk
	fn load_store_db(&mut self) -> Result<(), Box<dyn std::error::Error>> {
		let db_path = self.config.store_path.join(".pyro-store.json");
		if db_path.exists() {
			let content = fs::read_to_string(db_path)?;
			self.store_db = serde_json::from_str(&content)?;
		}
		Ok(())
	}

	/// Save store database to disk
	fn save_store_db(&self) -> Result<(), Box<dyn std::error::Error>> {
		let db_path = self.config.store_path.join(".pyro-store.json");
		let content = serde_json::to_string_pretty(&self.store_db)?;
		fs::write(db_path, content)?;
		Ok(())
	}

	/// Get store statistics
	pub fn get_stats(&self) -> StoreStats {
		let total_packages = self.store_db.len();
		let total_size: u64 = self.store_db.values().map(|p| p.size).sum();

		StoreStats {
			total_packages,
			total_size,
			store_path: self.config.store_path.clone(),
		}
	}
}

#[derive(Debug)]
pub struct StoreStats {
	pub total_packages: usize,
	pub total_size: u64,
	pub store_path: PathBuf,
}

// Implement serialization for StorePath
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
struct StorePathSerde {
	hash: String,
	name: String,
	path: PathBuf,
	dependencies: Vec<String>,
	size: u64,
	created_at: u64,
	last_accessed: u64,
}

impl Serialize for StorePath {
	fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
	where
		S: serde::Serializer,
	{
		let serde_path = StorePathSerde {
			hash: self.hash.clone(),
			name: self.name.clone(),
			path: self.path.clone(),
			dependencies: self.dependencies.clone(),
			size: self.size,
			created_at: self
				.created_at
				.duration_since(SystemTime::UNIX_EPOCH)
				.unwrap_or_default()
				.as_secs(),
			last_accessed: self
				.last_accessed
				.duration_since(SystemTime::UNIX_EPOCH)
				.unwrap_or_default()
				.as_secs(),
		};
		serde_path.serialize(serializer)
	}
}

impl<'de> Deserialize<'de> for StorePath {
	fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
	where
		D: serde::Deserializer<'de>,
	{
		let serde_path = StorePathSerde::deserialize(deserializer)?;
		Ok(StorePath {
			hash: serde_path.hash,
			name: serde_path.name,
			path: serde_path.path,
			dependencies: serde_path.dependencies,
			size: serde_path.size,
			created_at: SystemTime::UNIX_EPOCH
				+ std::time::Duration::from_secs(serde_path.created_at),
			last_accessed: SystemTime::UNIX_EPOCH
				+ std::time::Duration::from_secs(serde_path.last_accessed),
		})
	}
}
