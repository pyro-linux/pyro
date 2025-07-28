use crate::config::{BuildConfig, Package, PackageSource};
use crate::store::{BuildResult, PyroStore};
use petgraph::graph::{DiGraph, NodeIndex};
use std::collections::HashMap;
use std::fs;
use std::io::BufRead as _;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::Instant;
use thiserror::Error;
use tokio::sync::Semaphore;

mod fetch;

/// Nix-like package builder with sandboxing and reproducibility
pub struct PyroBuilder {
	config: BuildConfig,
	store: Arc<tokio::sync::Mutex<PyroStore>>,
	build_semaphore: Arc<Semaphore>,
}

#[derive(Debug, Clone)]
pub struct BuildContext {
	pub package: Package,
	pub build_dir: PathBuf,
	pub store_path: PathBuf,
	pub environment: HashMap<String, String>,
	pub dependencies: Vec<PathBuf>,
}

#[derive(Debug, Error)]
pub enum BuildError {
	#[error("Failed to resolve dependency: {0}")]
	DependencyResolutionFailed(String),
	#[error("Build failed: {0}")]
	BuildFailed(String),
	#[error("Sandbox violation: {0}")]
	SandboxViolation(String),
	#[error("Failed to create directory: {0}")]
	IoError(#[from] std::io::Error),
	#[error("Failed to fetch: {0}")]
	RequestError(#[from] FetchError),
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
		graph: &DiGraph<Package, ()>,
		root: NodeIndex,
	) -> Result<BuildResult, BuildError> {
		self.build_package_inner(graph, root).await
	}

	fn build_package_inner<'a>(
		&'a self,
		graph: &'a DiGraph<Package, ()>,
		root: NodeIndex,
	) -> std::pin::Pin<
		Box<
			dyn std::future::Future<Output = Result<BuildResult, BuildError>>
				+ Send
				+ 'a,
		>,
	> {
		Box::pin(async move {
			// Check if package already exists in store
			{
				let store = self.store.lock().await;

				let spec = &graph[root];
				let store_path = store.get_store_path(spec);
				if store_path.exists() {
					return Ok(BuildResult {
						store_path,
						build_log: String::new(),
						success: true,
					});
				}
			}

			// Acquire build semaphore
			let _permit = self.build_semaphore.acquire().await.unwrap();

			// Build dependencies first
			let mut dependencies = Vec::new();
			for dep in graph.neighbors(root) {
				let dep_result = self.build_package_inner(graph, dep).await?;
				if !dep_result.success {
					return Err(BuildError::DependencyResolutionFailed(
						format!(
							"Failed to build dependency: {}",
							graph[dep].name
						),
					));
				}
				dependencies.push(dep_result.store_path);
			}

			// Create build context
			let build_context = self
				.create_build_context(&graph[root], dependencies)
				.await?;

			// Perform the actual build
			self.execute_build(&build_context).await
		})
	}

	/// Create isolated build context
	async fn create_build_context(
		&self,
		spec: &Package,
		dependencies: Vec<PathBuf>,
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
			dep_paths.push(dep.to_string_lossy().to_string());
		}
		environment.insert("PYRO_DEPS".to_string(), dep_paths.join(":"));

		// Add package-specific environment variables
		for (key, value) in &spec.environment {
			environment.insert(key.clone(), value.clone());
		}

		Ok(BuildContext {
			package: spec.clone(),
			build_dir,
			store_path,
			environment,
			dependencies,
		})
	}

	async fn execute_build(
		&self,
		context: &BuildContext,
	) -> Result<BuildResult, BuildError> {
		let _start_time = Instant::now();

		fetch::fetch_source(
			&self.config,
			&context.package.source,
			&context.build_dir,
		)?;

		let mut command = Command::new(&context.package.builder);
		command
			.args(&context.package.args)
			.envs(&context.environment)
			.current_dir(&context.build_dir)
			.stderr(std::process::Stdio::piped());

		let mut child = command.spawn().map_err(|e| {
			BuildError::BuildFailed(format!(
				"Failed to spawn build command: {}",
				e
			))
		})?;

		let mut build_log_stderr = String::new();
		if let Some(mut stderr) = child.stderr.take() {
			let mut reader = std::io::BufReader::new(&mut stderr);
			let mut buffer = String::new();
			while reader.read_line(&mut buffer).unwrap_or(0) > 0 {
				build_log_stderr.push_str(&buffer);
				buffer.clear();
			}
		}

		let exit_status = child.wait().map_err(|e| {
			BuildError::BuildFailed(format!("Build command failed: {}", e))
		})?;

		let store_path =
			self.store.lock().await.get_store_path(&context.package);

		if exit_status.success() {
			// Install to store path
			fs::create_dir_all(&context.store_path)?;
			self.install_package(context, &mut build_log).await?;

			Ok(BuildResult {
				store_path,
				build_log,
				success: true,
			})
		} else {
			Ok(BuildResult {
				store_path,
				build_log,
				success: false,
			})
		}
	}

	/// Copy local path
	#[tracing::instrument(skip(self, src, dest))]
	async fn copy_local_path(
		&self,
		src: &Path,
		dest: &Path,
	) -> Result<(), BuildError> {
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

	/// Install package to store path
	#[tracing::instrument(skip(self, context))]
	async fn install_package(
		&self,
		context: &BuildContext,
	) -> Result<(), BuildError> {
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
}

/// Copy directory recursively
fn copy_directory(source: &Path, destination: &Path) -> Result<(), BuildError> {
	if !source.exists() {
		return Ok(());
	}

	fs::create_dir_all(destination)?;

	for entry in fs::read_dir(source)? {
		let entry = entry?;
		let source_path = entry.path();
		let dest_path = destination.join(entry.file_name());

		if source_path.is_dir() {
			copy_directory(&source_path, &dest_path)?;
		} else {
			fs::copy(&source_path, &dest_path)?;
		}
	}

	Ok(())
}
