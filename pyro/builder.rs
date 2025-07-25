use crate::config::{BuildConfig, PackageSource, PackageSpec};
use crate::rustc_builder::RustcBuilder;
use crate::store::{BuildResult, PyroStore, StorePath};
use reqwest;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::{Instant, SystemTime};
use tokio::sync::Semaphore;

use flate2::read::GzDecoder;
use tar::Archive;

/// Nix-like package builder with sandboxing and reproducibility
#[derive(Debug, Clone)]
pub struct PyroBuilder {
	config: BuildConfig,
	store: Arc<tokio::sync::Mutex<PyroStore>>,
	build_semaphore: Arc<Semaphore>,
}

#[derive(Debug, Clone)]
pub struct BuildContext {
	pub package: PackageSpec,
	pub build_dir: PathBuf,
	pub store_path: PathBuf,
	pub environment: HashMap<String, String>,
	pub dependencies: Vec<StorePath>,
}

#[derive(Debug)]
pub enum BuildError {
	DependencyResolutionFailed(String),
	BuildFailed(String),
	SandboxViolation(String),
	IoError(String),
	NetworkError(String),
	GitError(String),
}

impl std::fmt::Display for BuildError {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		match self {
			BuildError::DependencyResolutionFailed(msg) => {
				write!(f, "Dependency resolution failed: {}", msg)
			}
			BuildError::BuildFailed(msg) => write!(f, "Build failed: {}", msg),
			BuildError::SandboxViolation(msg) => {
				write!(f, "Sandbox violation: {}", msg)
			}
			BuildError::IoError(msg) => write!(f, "IO error: {}", msg),
			BuildError::NetworkError(msg) => {
				write!(f, "Network error: {}", msg)
			}
			BuildError::GitError(msg) => write!(f, "Git error: {}", msg),
		}
	}
}

impl std::error::Error for BuildError {}

impl From<std::io::Error> for BuildError {
	fn from(error: std::io::Error) -> Self {
		BuildError::IoError(error.to_string())
	}
}

impl PyroBuilder {
	pub fn new(
		config: BuildConfig,
		store: Arc<tokio::sync::Mutex<PyroStore>>,
	) -> Self {
		let build_semaphore = Arc::new(Semaphore::new(config.max_jobs));

		PyroBuilder {
			config,
			store,
			build_semaphore,
		}
	}

	/// Build a package with all its dependencies
	pub async fn build_package(
		&self,
		spec: &PackageSpec,
	) -> Result<BuildResult, BuildError> {
		Box::pin(self.build_package_impl(spec)).await
	}

	/// Build a Rust crate using custom rustc builder instead of cargo
	pub async fn build_rust_crate_with_rustc(
		&self,
		spec: &PackageSpec,
	) -> Result<BuildResult, BuildError> {
		println!(
			"DEBUG: Starting build_rust_crate_with_rustc for: {}",
			spec.name
		);

		// Check if package already exists in store
		{
			println!("DEBUG: Checking if package exists in store");
			let mut store = self.store.lock().await;
			if store.package_exists(spec) {
				if let Some(store_path) = store.get_package(spec) {
					println!("DEBUG: Package already exists, returning early");
					return Ok(BuildResult {
						store_path: store_path.clone(),
						build_log: "Package already exists in store"
							.to_string(),
						success: true,
					});
				}
			}
			println!("DEBUG: Package does not exist, proceeding with build");
		}

		// Acquire build semaphore
		println!("DEBUG: Acquiring build semaphore");
		let _permit = self.build_semaphore.acquire().await.unwrap();
		println!("DEBUG: Build semaphore acquired");

		// Create target directory for rustc builder
		println!("DEBUG: Creating target directory");
		let target_dir =
			std::env::temp_dir().join(format!("pyro-rustc-{}", spec.name));
		fs::create_dir_all(&target_dir)?;
		println!("DEBUG: Target directory created: {:?}", target_dir);

		// Create rustc builder instance
		println!("DEBUG: Creating rustc builder instance");
		let mut rustc_builder =
			RustcBuilder::new(self.clone(), target_dir.clone());
		println!("DEBUG: Rustc builder created");

		// Build with rustc using Pyro's dependency graph
		println!("DEBUG: Starting rustc build");
		let mut build_log = String::new();
		let success =
			rustc_builder.build_with_rustc(spec, &mut build_log).await?;
		println!("DEBUG: Rustc build completed with success: {}", success);

		if success {
			// Create store path
			let store = self.store.lock().await;
			let store_path_dir = store.get_store_path(spec);
			drop(store);

			// Install to store
			fs::create_dir_all(&store_path_dir)?;
			let output_dir = target_dir.join("output").join(&spec.name);
			if output_dir.exists() {
				self.copy_directory(&output_dir, &store_path_dir)?;
			}

			let store_path = StorePath {
				hash: {
					let store = self.store.lock().await;
					store.compute_package_hash(spec)
				},
				name: spec.name.clone(),
				path: store_path_dir.clone(),
				dependencies: vec![], // Dependencies handled by rustc builder
				size: self
					.calculate_directory_size(&store_path_dir)
					.unwrap_or(0),
				created_at: SystemTime::now(),
				last_accessed: SystemTime::now(),
			};

			build_log.push_str(
				"Successfully built Rust crate with rustc and Pyro dependency graph\n",
			);

			Ok(BuildResult {
				store_path,
				build_log,
				success: true,
			})
		} else {
			Ok(BuildResult {
				store_path: StorePath {
					hash: String::new(),
					name: spec.name.clone(),
					path: PathBuf::new(),
					dependencies: vec![],
					size: 0,
					created_at: SystemTime::now(),
					last_accessed: SystemTime::now(),
				},
				build_log,
				success: false,
			})
		}
	}

	async fn build_package_impl(
		&self,
		spec: &PackageSpec,
	) -> Result<BuildResult, BuildError> {
		// Check if package already exists in store
		{
			let mut store = self.store.lock().await;
			if store.package_exists(spec) {
				if let Some(store_path) = store.get_package(spec) {
					return Ok(BuildResult {
						store_path: store_path.clone(),
						build_log: "Package already exists in store"
							.to_string(),
						success: true,
					});
				}
			}
		}

		// Acquire build semaphore
		let _permit = self.build_semaphore.acquire().await.unwrap();

		// Build dependencies first
		let mut dependencies = Vec::new();
		for dep_name in &spec.build_inputs {
			// For simplicity, assume dependencies are also PackageSpecs
			// In a real implementation, you'd resolve these from a registry
			let dep_spec = self.resolve_dependency(dep_name).await?;
			let dep_result =
				Box::pin(self.build_package_impl(&dep_spec)).await?;
			if dep_result.success {
				dependencies.push(dep_result.store_path);
			} else {
				return Err(BuildError::DependencyResolutionFailed(
					dep_name.clone(),
				));
			}
		}

		// Create build context
		let build_context =
			self.create_build_context(spec, dependencies).await?;

		// Perform the actual build
		self.execute_build(&build_context).await
	}

	/// Create isolated build context
	async fn create_build_context(
		&self,
		spec: &PackageSpec,
		dependencies: Vec<StorePath>,
	) -> Result<BuildContext, BuildError> {
		let store = self.store.lock().await;
		let store_path = store.get_store_path(spec);
		drop(store);

		// Create temporary build directory
		let build_dir =
			std::env::temp_dir().join(format!("pyro-build-{}", spec.name));
		fs::create_dir_all(&build_dir)?;

		// Set up build environment
		let mut environment = HashMap::new();

		// Add dependency paths to environment
		let mut dep_paths = Vec::new();
		for dep in &dependencies {
			dep_paths.push(dep.path.to_string_lossy().to_string());
		}
		environment.insert("PYRO_DEPS".to_string(), dep_paths.join(":"));

		// Add package-specific environment variables
		for (key, value) in &spec.environment {
			environment.insert(key.clone(), value.clone());
		}

		// Set build flags for reproducibility
		if self.config.sandbox {
			environment
				.insert("SOURCE_DATE_EPOCH".to_string(), "1".to_string());
			environment.insert("TZ".to_string(), "UTC".to_string());
		}

		Ok(BuildContext {
			package: spec.clone(),
			build_dir,
			store_path,
			environment,
			dependencies,
		})
	}

	/// Execute the build in a sandboxed environment
	async fn execute_build(
		&self,
		context: &BuildContext,
	) -> Result<BuildResult, BuildError> {
		let _start_time = Instant::now();
		let mut build_log = String::new();

		// Download/prepare source
		self.prepare_source(context, &mut build_log).await?;

		// Execute build script
		let success = if let Some(build_script) = &context.package.build_script
		{
			self.run_build_script(context, build_script, &mut build_log)
				.await?
		} else {
			self.run_default_build(context, &mut build_log).await?
		};

		if success {
			// Install to store path
			fs::create_dir_all(&context.store_path)?;
			self.install_package(context, &mut build_log).await?;

			let store_path = StorePath {
				hash: {
					let store = self.store.lock().await;
					store.compute_package_hash(&context.package)
				},
				name: context.package.name.clone(),
				path: context.store_path.clone(),
				dependencies: context
					.dependencies
					.iter()
					.map(|d| d.hash.clone())
					.collect(),
				size: self.calculate_directory_size(&context.store_path)?,
				created_at: SystemTime::now(),
				last_accessed: SystemTime::now(),
			};

			Ok(BuildResult {
				store_path,
				build_log,
				success: true,
			})
		} else {
			Ok(BuildResult {
				store_path: StorePath {
					hash: String::new(),
					name: context.package.name.clone(),
					path: context.store_path.clone(),
					dependencies: vec![],
					size: 0,
					created_at: SystemTime::now(),
					last_accessed: SystemTime::now(),
				},
				build_log,
				success: false,
			})
		}
	}

	/// Prepare package source code
	async fn prepare_source(
		&self,
		context: &BuildContext,
		build_log: &mut String,
	) -> Result<(), BuildError> {
		build_log.push_str(&format!(
			"Preparing source for {}\n",
			context.package.name
		));

		match &context.package.source {
			PackageSource::Crates { name, version } => {
				let url = format!(
					"https://static.crates.io/crates/{}/{}-{}.crate",
					name, name, version
				);
				self.download_and_extract(&url, &context.build_dir, build_log)
					.await?
			}
			PackageSource::Git { url, rev } => {
				self.clone_git_repo(
					url,
					rev.as_deref(),
					&context.build_dir,
					build_log,
				)
				.await?
			}
			PackageSource::Path { path } => {
				self.copy_local_path(path, &context.build_dir, build_log)
					.await?
			}
			PackageSource::Url { url, hash: _ } => {
				self.download_and_extract(url, &context.build_dir, build_log)
					.await?
			}
		}

		Ok(())
	}

	/// Download and extract source archive
	async fn download_and_extract(
		&self,
		url: &str,
		dest: &Path,
		build_log: &mut String,
	) -> Result<(), BuildError> {
		build_log.push_str(&format!("Downloading from {}\n", url));

		let response = reqwest::get(url)
			.await
			.map_err(|e| BuildError::NetworkError(e.to_string()))?;

		if !response.status().is_success() {
			return Err(BuildError::NetworkError(format!(
				"Failed to download: {}",
				response.status()
			)));
		}

		let bytes = response
			.bytes()
			.await
			.map_err(|e| BuildError::NetworkError(e.to_string()))?;

		build_log.push_str("Extracting source archive\n");

		// Extract the archive directly from memory
		let tar = GzDecoder::new(bytes.as_ref());
		let mut archive = Archive::new(tar);
		archive
			.unpack(dest)
			.map_err(|e| BuildError::IoError(e.to_string()))?;

		Ok(())
	}

	/// Clone git repository
	async fn clone_git_repo(
		&self,
		url: &str,
		rev: Option<&str>,
		dest: &Path,
		build_log: &mut String,
	) -> Result<(), BuildError> {
		build_log.push_str(&format!("Cloning git repository {}\n", url));

		let mut cmd = Command::new("git");
		cmd.arg("clone").arg(url).arg("source").current_dir(dest);

		let output = cmd.output()?;
		if !output.status.success() {
			return Err(BuildError::BuildFailed(
				"Failed to clone repository".to_string(),
			));
		}

		if let Some(rev) = rev {
			build_log.push_str(&format!("Checking out revision {}\n", rev));
			let output = Command::new("git")
				.arg("checkout")
				.arg(rev)
				.current_dir(dest.join("source"))
				.output()?;

			if !output.status.success() {
				return Err(BuildError::BuildFailed(
					"Failed to checkout revision".to_string(),
				));
			}
		}

		Ok(())
	}

	/// Copy local path
	async fn copy_local_path(
		&self,
		src: &Path,
		dest: &Path,
		build_log: &mut String,
	) -> Result<(), BuildError> {
		build_log
			.push_str(&format!("Copying from local path {}\n", src.display()));

		let output = Command::new("cp")
			.arg("-r")
			.arg(src)
			.arg(dest.join("source"))
			.output()?;

		if !output.status.success() {
			return Err(BuildError::BuildFailed(
				"Failed to copy local path".to_string(),
			));
		}

		Ok(())
	}

	/// Run custom build script
	async fn run_build_script(
		&self,
		context: &BuildContext,
		script: &str,
		build_log: &mut String,
	) -> Result<bool, BuildError> {
		build_log.push_str("Running custom build script\n");

		// Split script into commands
		let commands: Vec<&str> = script
			.split('\n')
			.filter(|line| !line.trim().is_empty())
			.collect();

		for command in commands {
			let command = command.trim();
			if command.starts_with('#') {
				continue; // Skip comments
			}

			build_log.push_str(&format!("Running: {}\n", command));

			let mut cmd = if cfg!(target_os = "windows") {
				let mut cmd = Command::new("cmd");
				cmd.args(["/C", command]);
				cmd
			} else {
				let mut cmd = Command::new("sh");
				cmd.args(["-c", command]);
				cmd
			};

			cmd.current_dir(&context.build_dir)
				.envs(&context.environment);

			if self.config.sandbox {
				// Add sandboxing flags (simplified)
				cmd.env("PATH", "/usr/bin:/bin");
			}

			let output = cmd.output()?;
			build_log.push_str(&String::from_utf8_lossy(&output.stdout));
			if !output.stderr.is_empty() {
				build_log.push_str(&String::from_utf8_lossy(&output.stderr));
			}

			if !output.status.success() {
				build_log.push_str(&format!(
					"Command failed with exit code: {:?}\n",
					output.status.code()
				));
				return Ok(false);
			}
		}

		build_log.push_str("Custom build completed successfully\n");
		Ok(true)
	}

	/// Run default build (cargo build for Rust packages)
	async fn run_default_build(
		&self,
		context: &BuildContext,
		build_log: &mut String,
	) -> Result<bool, BuildError> {
		let source_dir = context.build_dir.join("source");

		// Check if this is a Rust project
		let cargo_toml = source_dir.join("Cargo.toml");
		if cargo_toml.exists() {
			build_log.push_str("Detected Rust project, running cargo build\n");
			return self
				.execute_cargo_build(
					&source_dir,
					&context.environment,
					build_log,
				)
				.await;
		}

		// Check if this is a Node.js project
		let package_json = source_dir.join("package.json");
		if package_json.exists() {
			build_log
				.push_str("Detected Node.js project, running npm install\n");
			return self
				.execute_npm_build(&source_dir, &context.environment, build_log)
				.await;
		}

		// Check if this is a Python project
		let setup_py = source_dir.join("setup.py");
		let pyproject_toml = source_dir.join("pyproject.toml");
		if setup_py.exists() || pyproject_toml.exists() {
			build_log
				.push_str("Detected Python project, running pip install\n");
			return self
				.execute_python_build(
					&source_dir,
					&context.environment,
					build_log,
				)
				.await;
		}

		// Check for Makefile
		let makefile = source_dir.join("Makefile");
		if makefile.exists() {
			build_log.push_str("Detected Makefile, running make\n");
			return self
				.execute_make_build(
					&source_dir,
					&context.environment,
					build_log,
				)
				.await;
		}

		build_log
			.push_str("No recognized build system found, assuming pre-built\n");
		Ok(true)
	}

	/// Execute cargo build for Rust projects
	async fn execute_cargo_build(
		&self,
		build_dir: &Path,
		environment: &HashMap<String, String>,
		build_log: &mut String,
	) -> Result<bool, BuildError> {
		let mut cmd = Command::new("cargo");
		cmd.args(["build", "--release"])
			.current_dir(build_dir)
			.envs(environment);

		let output = cmd.output()?;
		build_log.push_str(&String::from_utf8_lossy(&output.stdout));
		if !output.stderr.is_empty() {
			build_log.push_str(&String::from_utf8_lossy(&output.stderr));
		}

		if output.status.success() {
			build_log.push_str("Cargo build completed successfully\n");
			Ok(true)
		} else {
			build_log.push_str(&format!(
				"Cargo build failed with exit code: {:?}\n",
				output.status.code()
			));
			Ok(false)
		}
	}

	/// Execute npm build for Node.js projects
	async fn execute_npm_build(
		&self,
		build_dir: &Path,
		environment: &HashMap<String, String>,
		build_log: &mut String,
	) -> Result<bool, BuildError> {
		let mut cmd = Command::new("npm");
		cmd.args(["install"])
			.current_dir(build_dir)
			.envs(environment);

		let output = cmd.output()?;
		build_log.push_str(&String::from_utf8_lossy(&output.stdout));
		if !output.stderr.is_empty() {
			build_log.push_str(&String::from_utf8_lossy(&output.stderr));
		}

		if output.status.success() {
			build_log.push_str("npm install completed successfully\n");
			Ok(true)
		} else {
			build_log.push_str(&format!(
				"npm install failed with exit code: {:?}\n",
				output.status.code()
			));
			Ok(false)
		}
	}

	/// Execute pip install for Python projects
	async fn execute_python_build(
		&self,
		build_dir: &Path,
		environment: &HashMap<String, String>,
		build_log: &mut String,
	) -> Result<bool, BuildError> {
		let mut cmd = Command::new("pip");
		cmd.args(["install", "."])
			.current_dir(build_dir)
			.envs(environment);

		let output = cmd.output()?;
		build_log.push_str(&String::from_utf8_lossy(&output.stdout));
		if !output.stderr.is_empty() {
			build_log.push_str(&String::from_utf8_lossy(&output.stderr));
		}

		if output.status.success() {
			build_log.push_str("pip install completed successfully\n");
			Ok(true)
		} else {
			build_log.push_str(&format!(
				"pip install failed with exit code: {:?}\n",
				output.status.code()
			));
			Ok(false)
		}
	}

	/// Execute make for projects with Makefile
	async fn execute_make_build(
		&self,
		build_dir: &Path,
		environment: &HashMap<String, String>,
		build_log: &mut String,
	) -> Result<bool, BuildError> {
		let mut cmd = Command::new("make");
		cmd.current_dir(build_dir).envs(environment);

		let output = cmd.output()?;
		build_log.push_str(&String::from_utf8_lossy(&output.stdout));
		if !output.stderr.is_empty() {
			build_log.push_str(&String::from_utf8_lossy(&output.stderr));
		}

		if output.status.success() {
			build_log.push_str("make completed successfully\n");
			Ok(true)
		} else {
			build_log.push_str(&format!(
				"make failed with exit code: {:?}\n",
				output.status.code()
			));
			Ok(false)
		}
	}

	/// Install package to store path
	async fn install_package(
		&self,
		context: &BuildContext,
		build_log: &mut String,
	) -> Result<(), BuildError> {
		build_log.push_str(&format!(
			"Installing to {}\n",
			context.store_path.display()
		));

		let source_dir = context.build_dir.join("source");
		let target_dir = source_dir.join("target/release");

		if target_dir.exists() {
			let output = Command::new("cp")
				.arg("-r")
				.arg(&target_dir)
				.arg(&context.store_path)
				.output()?;

			if !output.status.success() {
				return Err(BuildError::BuildFailed(
					"Failed to install package".to_string(),
				));
			}
		}

		Ok(())
	}

	/// Resolve a dependency to a concrete package specification
	async fn resolve_dependency(
		&self,
		dep_name: &str,
	) -> Result<PackageSpec, BuildError> {
		// Try to resolve from crates.io first
		if let Ok(spec) = self.resolve_from_crates_io(dep_name).await {
			return Ok(spec);
		}

		// Try to resolve from npm registry
		if let Ok(spec) = self.resolve_from_npm(dep_name).await {
			return Ok(spec);
		}

		// Try to resolve from PyPI
		if let Ok(spec) = self.resolve_from_pypi(dep_name).await {
			return Ok(spec);
		}

		Err(BuildError::DependencyResolutionFailed(format!(
			"Dependency {} not found in any registry",
			dep_name
		)))
	}

	/// Resolve dependency from crates.io
	async fn resolve_from_crates_io(
		&self,
		dep_name: &str,
	) -> Result<PackageSpec, BuildError> {
		let url = format!("https://crates.io/api/v1/crates/{}", dep_name);
		let response = reqwest::get(&url)
			.await
			.map_err(|e| BuildError::NetworkError(e.to_string()))?;

		if !response.status().is_success() {
			return Err(BuildError::DependencyResolutionFailed(format!(
				"Crate {} not found on crates.io",
				dep_name
			)));
		}

		let crate_info: serde_json::Value = response
			.json()
			.await
			.map_err(|e| BuildError::NetworkError(e.to_string()))?;

		let latest_version =
			crate_info["crate"]["max_version"].as_str().ok_or_else(|| {
				BuildError::DependencyResolutionFailed(
					"No version found".to_string(),
				)
			})?;

		Ok(PackageSpec {
			name: dep_name.to_string(),
			version: Some(latest_version.to_string()),
			source: PackageSource::Crates {
				name: dep_name.to_string(),
				version: latest_version.to_string(),
			},
			build_inputs: vec![],
			runtime_inputs: vec![],
			environment: HashMap::new(),
			build_script: None,
		})
	}

	/// Resolve dependency from npm registry
	async fn resolve_from_npm(
		&self,
		dep_name: &str,
	) -> Result<PackageSpec, BuildError> {
		let url = format!("https://registry.npmjs.org/{}", dep_name);
		let response = reqwest::get(&url)
			.await
			.map_err(|e| BuildError::NetworkError(e.to_string()))?;

		if !response.status().is_success() {
			return Err(BuildError::DependencyResolutionFailed(format!(
				"Package {} not found on npm",
				dep_name
			)));
		}

		let package_info: serde_json::Value = response
			.json()
			.await
			.map_err(|e| BuildError::NetworkError(e.to_string()))?;

		let latest_version = package_info["dist-tags"]["latest"]
			.as_str()
			.ok_or_else(|| {
				BuildError::DependencyResolutionFailed(
					"No version found".to_string(),
				)
			})?;

		let tarball_url =
			package_info["versions"][latest_version]["dist"]["tarball"]
				.as_str()
				.ok_or_else(|| {
					BuildError::DependencyResolutionFailed(
						"No tarball URL found".to_string(),
					)
				})?;

		Ok(PackageSpec {
			name: dep_name.to_string(),
			version: Some(latest_version.to_string()),
			source: PackageSource::Url {
				url: tarball_url.to_string(),
				hash: String::new(),
			},
			build_inputs: vec![],
			runtime_inputs: vec![],
			environment: HashMap::new(),
			build_script: None,
		})
	}

	/// Resolve dependency from PyPI
	async fn resolve_from_pypi(
		&self,
		dep_name: &str,
	) -> Result<PackageSpec, BuildError> {
		let url = format!("https://pypi.org/pypi/{}/json", dep_name);
		let response = reqwest::get(&url)
			.await
			.map_err(|e| BuildError::NetworkError(e.to_string()))?;

		if !response.status().is_success() {
			return Err(BuildError::DependencyResolutionFailed(format!(
				"Package {} not found on PyPI",
				dep_name
			)));
		}

		let package_info: serde_json::Value = response
			.json()
			.await
			.map_err(|e| BuildError::NetworkError(e.to_string()))?;

		let latest_version =
			package_info["info"]["version"].as_str().ok_or_else(|| {
				BuildError::DependencyResolutionFailed(
					"No version found".to_string(),
				)
			})?;

		// Find source distribution URL
		let urls = package_info["urls"].as_array().ok_or_else(|| {
			BuildError::DependencyResolutionFailed("No URLs found".to_string())
		})?;

		let source_url = urls
			.iter()
			.find(|url| url["packagetype"].as_str() == Some("sdist"))
			.and_then(|url| url["url"].as_str())
			.ok_or_else(|| {
				BuildError::DependencyResolutionFailed(
					"No source distribution found".to_string(),
				)
			})?;

		Ok(PackageSpec {
			name: dep_name.to_string(),
			version: Some(latest_version.to_string()),
			source: PackageSource::Url {
				url: source_url.to_string(),
				hash: String::new(),
			},
			build_inputs: vec![],
			runtime_inputs: vec![],
			environment: HashMap::new(),
			build_script: None,
		})
	}

	/// Calculate directory size
	fn calculate_directory_size(&self, path: &Path) -> Result<u64, BuildError> {
		let mut size = 0;
		if path.is_dir() {
			for entry in fs::read_dir(path)? {
				let entry = entry?;
				let metadata = entry.metadata()?;
				if metadata.is_dir() {
					size += self.calculate_directory_size(&entry.path())?;
				} else {
					size += metadata.len();
				}
			}
		}
		Ok(size)
	}

	/// Copy directory recursively
	fn copy_directory(
		&self,
		source: &Path,
		destination: &Path,
	) -> Result<(), BuildError> {
		if !source.exists() {
			return Ok(());
		}

		fs::create_dir_all(destination)?;

		for entry in fs::read_dir(source)? {
			let entry = entry?;
			let source_path = entry.path();
			let dest_path = destination.join(entry.file_name());

			if source_path.is_dir() {
				self.copy_directory(&source_path, &dest_path)?;
			} else {
				fs::copy(&source_path, &dest_path)?;
			}
		}

		Ok(())
	}
}
