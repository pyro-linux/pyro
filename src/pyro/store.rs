use crate::config::{Package, StoreConfig};
use blake3::{Hash, Hasher};
use jiff::Span;
use libsql::Database;
use std::fmt::Debug;
use std::fs;
use std::path::PathBuf;
use std::time::SystemTime;

#[derive(Debug)]
pub struct BuildResult {
	pub store_path: PathBuf,
	pub build_log: String,
	pub success: bool,
}

/// Nix-like immutable package store
#[derive(Debug)]
pub struct PyroStore {
	config: StoreConfig,
	database: Database,
}

impl PyroStore {
	pub async fn new(
		config: StoreConfig,
	) -> Result<Self, Box<dyn std::error::Error>> {
		fs::create_dir_all(&config.store_path)?;

		let store = Self {
			database: libsql::Builder::new_local(
				config.db_path.to_string_lossy().as_ref(),
			)
			.build()
			.await?,
			config,
		};

		// Create tables
		let connection = store.database.connect()?;
		connection
			.execute(
				"CREATE TABLE IF NOT EXISTS valid_paths (
					id INTEGER PRIMARY KEY AUTOINCREMENT,
					path TEXT NOT NULL UNIQUE,
					hash TEXT NOT NULL UNIQUE,
					serialized_size INTEGER NOT NULL,
					references INTEGER DEFAULT 0,
					signature TEXT
				)",
				(),
			)
			.await?;

		Ok(store)
	}

	/// Generate content-addressable hash for a package
	pub fn compute_package_hash(&self, spec: &Package) -> Hash {
		let mut hasher = Hasher::new();

		// Hash package specification for reproducibility
		hasher.update(spec.name.as_bytes());
		hasher.update(spec.version.as_bytes());

		// Hash source
		match &spec.source {
			crate::config::PackageSource::Crate { name, version } => {
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

		hasher.finalize()
	}

	/// Get store path for a package
	pub fn get_store_path(&self, spec: &Package) -> PathBuf {
		let hash = self.compute_package_hash(spec);
		let name = format!("{}-{}", hash, spec.name);
		self.config.store_path.join(name)
	}

	/// Check if package exists in store
	pub async fn package_exists(
		&self,
		spec: &Package,
	) -> Result<bool, Box<dyn std::error::Error>> {
		let hash = self.compute_package_hash(spec);
		let package_path = self.get_store_path(spec);
		if package_path.exists() {
			// Check if the hash matches the stored hash
			if let Ok(connection) = self.database.connect() {
				let mut rows = connection
					.query(
						"SELECT hash FROM valid_paths WHERE path = ?",
						[package_path.to_string_lossy().as_ref()],
					)
					.await?;

				if let Some(row) = rows.next().await? {
					let stored_hash: String = row.get(2)?;
					return Ok(stored_hash == hash.to_hex().to_string());
				}
			}
		}

		Ok(false)
	}

	/// Add package to store
	pub async fn add_package(
		&mut self,
		spec: &Package,
		build_result: BuildResult,
	) -> Result<(), Box<dyn std::error::Error>> {
		let hash = self.compute_package_hash(spec);

		if build_result.success {
			let store_path = self.get_store_path(spec);
			if !store_path.exists() {
				fs::create_dir_all(&store_path)?;
			}

			// Save package metadata to database
			let connection = self.database.connect()?;
			connection
				.execute(
					"INSERT OR REPLACE INTO valid_paths (path, hash, serialized_size)
					 VALUES (?, ?, ?)",
					(
						store_path.to_string_lossy().as_ref(),
						hash.to_hex().as_ref(),
						build_result.store_path
							.metadata()
							.map(|m| m.len() as i64)
							.unwrap_or(0),
					),
				)
				.await?;
		} else {
			tracing::error!(
				"Failed to add package {}: Build failed",
				spec.name
			);
		}

		Ok(())
	}

	/// Install a package to the store
	pub async fn install_package(
		&self,
		spec: &Package,
		build_output: &std::path::Path,
	) -> Result<Hash, Box<dyn std::error::Error>> {
		let package_hash = self.compute_package_hash(spec);
		let package_path = self.config.store_path.join(format!(
			"{}-{}-{}",
			package_hash.to_hex(),
			spec.name,
			spec.version,
		));

		if package_path.exists() {
			return Ok(package_hash);
		}

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

		// Create symlinks for binaries
		self.create_profile_symlinks(&bin_path).await?;

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
	async fn create_profile_symlinks(
		&self,
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
						std::os::windows::fs::symlink_file(
							&path,
							&symlink_path,
						)?;
					}
				}
			}
		}

		Ok(())
	}

	/// Get package from store
	pub async fn get_package(
		&mut self,
		spec: &Package,
	) -> Result<Option<String>, Box<dyn std::error::Error>> {
		let hash = self.compute_package_hash(spec);
		let store_path = self.get_store_path(spec);

		if store_path.exists() {
			// Check if the package is registered in the database
			let connection = self.database.connect()?;
			let mut rows = connection
				.query(
					"SELECT path FROM valid_paths WHERE hash = ?",
					[hash.to_hex().as_ref()],
				)
				.await?;

			if let Some(row) = rows.next().await? {
				let path: String = row.get(0)?;
				return Ok(Some(path));
			}
		}

		Ok(None)
	}

	/// Garbage collect unused packages
	pub async fn garbage_collect(
		&mut self,
		dry_run: bool,
	) -> Result<Vec<String>, Box<dyn std::error::Error>> {
		let mut removed = Vec::new();

		// Find packages that are older than the configured threshold
		let mut to_remove = Vec::new();

		let connection = self.database.connect()?;
		let mut rows = connection
			.query("SELECT hash, path FROM valid_paths", ())
			.await?;
		while let Some(row) = rows.next().await? {
			let store_path: String = row.get(0)?;

			let fs_path = store_path.to_string();
			let last_accessed = std::fs::metadata(&fs_path)
				.and_then(|m| m.modified())
				.unwrap_or(SystemTime::UNIX_EPOCH);

			let remove_after = self
				.config
				.gc
				.remove_older_than
				.as_ref()
				.and_then(|s| s.parse::<Span>().ok())
				.map(|ts| ts.get_milliseconds())
				.map(|ms| {
					SystemTime::now()
						- std::time::Duration::from_millis(ms as u64)
				})
				.unwrap_or(SystemTime::UNIX_EPOCH);

			if last_accessed < remove_after {
				to_remove.push((store_path, fs_path));
			}
		}

		if dry_run {
			return Ok(to_remove.iter().map(|(s, _)| s.clone()).collect());
		}

		for (store_path_str, fs_path) in to_remove {
			// Remove package from store
			if let Err(e) = fs::remove_dir_all(&fs_path) {
				tracing::error!(
					"Failed to remove package {}: {}",
					store_path_str,
					e
				);
				continue;
			}

			// Remove from store database
			connection
				.execute(
					"DELETE FROM valid_paths WHERE path = ?",
					[store_path_str.as_str()],
				)
				.await?;
			removed.push(store_path_str);
		}

		Ok(removed)
	}

	/// Get store statistics
	pub fn get_stats(&self) -> StoreStats {
		StoreStats {
			total_packages: 0,
			total_size: 0,
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
