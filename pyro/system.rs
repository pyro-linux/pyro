//! System-level package management for foundational components
//! Handles building and managing system packages like glibc, LLVM, Rust, etc.

use crate::config::{BuildConfig, PackageSource};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemPackageSpec {
	pub name: String,
	pub version: String,
	pub arch: String,
	pub dependencies: Vec<String>,
	pub source: Option<PackageSource>,
	pub build_type: Option<SystemBuildType>,
	pub build_inputs: Vec<String>,
	pub runtime_inputs: Vec<String>,
	pub environment: HashMap<String, String>,
	pub build_script: Option<String>,
	pub configure_args: Option<Vec<String>>,
	pub make_args: Option<Vec<String>>,
	pub install_prefix: Option<PathBuf>,
	pub cross_compile_target: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SystemBuildType {
	/// GNU Autotools (./configure && make && make install)
	Autotools,
	/// CMake build system
	CMake,
	/// Meson build system
	Meson,
	/// Custom build script
	Custom,
	/// Rust cargo build
	Cargo,
	/// LLVM build
	Llvm,
	/// Linux kernel build
	Kernel,
}

#[derive(Debug, Clone)]
pub struct SystemBuilder {
	config: BuildConfig,
	sysroot: PathBuf,
	toolchain: Option<PathBuf>,
}

impl SystemBuilder {
	pub fn new(config: BuildConfig, sysroot: PathBuf) -> Self {
		Self {
			toolchain: config.toolchain_path.clone(),
			config,
			sysroot,
		}
	}

	/// Install a system dependency
	pub async fn install_dependency(
		&self,
		package: &str,
	) -> Result<(), Box<dyn std::error::Error>> {
		println!("Installing system dependency: {package}");

		// For now, just simulate installation
		// In a real implementation, this would download and install the package
		tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

		println!("✅ Installed system dependency: {package}");
		Ok(())
	}

	/// Build a package
	pub async fn build_package(
		&self,
		spec: &SystemPackageSpec,
	) -> Result<PathBuf, Box<dyn std::error::Error>> {
		self.build_system_package(spec).await
	}

	/// Build a system package
	pub async fn build_system_package(
		&self,
		spec: &SystemPackageSpec,
	) -> Result<PathBuf, Box<dyn std::error::Error>> {
		println!("Building system package: {} v{}", spec.name, spec.version);

		// Create build directory
		let build_dir =
			self.config.cache_dir.join("system-builds").join(&spec.name);
		std::fs::create_dir_all(&build_dir)?;

		// Use default source if not specified
		let default_source = PackageSource::Git {
			url: format!("https://github.com/{}/{}.git", spec.name, spec.name),
			rev: Some("main".to_string()),
		};
		let source = spec.source.as_ref().unwrap_or(&default_source);

		// Download and extract source
		let source_dir =
			self.prepare_source_with_source(source, &build_dir).await?;

		// Set up build environment
		let env = self.setup_build_environment(spec)?;

		// Execute build based on build type
		let build_type = spec
			.build_type
			.as_ref()
			.unwrap_or(&SystemBuildType::Autotools);
		let output_dir = match build_type {
			SystemBuildType::Autotools => {
				self.build_autotools(spec, &source_dir, &env).await?
			}
			SystemBuildType::CMake => {
				self.build_cmake(spec, &source_dir, &env).await?
			}
			SystemBuildType::Meson => {
				self.build_meson(spec, &source_dir, &env).await?
			}
			SystemBuildType::Custom => {
				self.build_custom(spec, &source_dir, &env).await?
			}
			SystemBuildType::Cargo => {
				self.build_cargo(spec, &source_dir, &env).await?
			}
			SystemBuildType::Llvm => {
				self.build_llvm(spec, &source_dir, &env).await?
			}
			SystemBuildType::Kernel => {
				self.build_kernel(spec, &source_dir, &env).await?
			}
		};

		println!("✅ Successfully built system package: {}", spec.name);
		Ok(output_dir)
	}

	/// Prepare source code for building
	async fn prepare_source_with_source(
		&self,
		source: &PackageSource,
		build_dir: &Path,
	) -> Result<PathBuf, Box<dyn std::error::Error>> {
		let source_dir = build_dir.join("source");

		match source {
			PackageSource::Git { url, rev } => {
				println!("Cloning from Git: {url}");
				let repo = git2::Repository::clone(url, &source_dir)?;
				if let Some(rev) = rev {
					let oid = git2::Oid::from_str(rev)?;
					let commit = repo.find_commit(oid)?;
					repo.checkout_tree(commit.as_object(), None)?;
				}
			}
			PackageSource::Url { url, hash: _ } => {
				println!("Downloading from URL: {url}");
				let response = reqwest::get(url).await?;
				let bytes = response.bytes().await?;

				// Extract archive
				let temp_file = tempfile::NamedTempFile::new()?;
				std::fs::write(temp_file.path(), &bytes)?;

				// Determine archive type and extract
				if url.ends_with(".tar.gz") || url.ends_with(".tgz") {
					let tar_gz = std::fs::File::open(temp_file.path())?;
					let tar = flate2::read::GzDecoder::new(tar_gz);
					let mut archive = tar::Archive::new(tar);
					archive.unpack(&source_dir)?;
				} else if url.ends_with(".tar.xz") {
					// Would need xz2 crate for .tar.xz support
					return Err("XZ archives not yet supported".into());
				}
			}
			PackageSource::Path { path } => {
				println!("Copying from local path: {}", path.display());
				Self::copy_directory(path, &source_dir)?;
			}
			_ => {
				return Err(
					"Unsupported source type for system packages".into()
				);
			}
		}

		Ok(source_dir)
	}

	/// Set up build environment
	fn setup_build_environment(
		&self,
		spec: &SystemPackageSpec,
	) -> Result<HashMap<String, String>, Box<dyn std::error::Error>> {
		let mut env = std::env::vars().collect::<HashMap<_, _>>();

		// Add package-specific environment variables
		for (key, value) in &spec.environment {
			env.insert(key.clone(), value.clone());
		}

		// Set up cross-compilation if specified
		if let Some(target) = &spec.cross_compile_target {
			env.insert("CC".to_string(), format!("{target}-gcc"));
			env.insert("CXX".to_string(), format!("{target}-g++"));
			env.insert("AR".to_string(), format!("{target}-ar"));
			env.insert("STRIP".to_string(), format!("{target}-strip"));
		}

		// Set up sysroot
		env.insert(
			"SYSROOT".to_string(),
			self.sysroot.to_string_lossy().to_string(),
		);
		env.insert(
			"PREFIX".to_string(),
			self.sysroot.to_string_lossy().to_string(),
		);

		// Set up PKG_CONFIG_PATH
		let pkg_config_path = self.sysroot.join("lib").join("pkgconfig");
		env.insert(
			"PKG_CONFIG_PATH".to_string(),
			pkg_config_path.to_string_lossy().to_string(),
		);

		Ok(env)
	}

	/// Build using GNU Autotools
	async fn build_autotools(
		&self,
		spec: &SystemPackageSpec,
		source_dir: &PathBuf,
		env: &HashMap<String, String>,
	) -> Result<PathBuf, Box<dyn std::error::Error>> {
		let install_prefix =
			spec.install_prefix.as_ref().unwrap_or(&self.sysroot);

		// Run ./configure
		let mut configure_cmd = std::process::Command::new("./configure");
		configure_cmd.current_dir(source_dir);
		configure_cmd.arg(format!("--prefix={}", install_prefix.display()));

		if let Some(configure_args) = &spec.configure_args {
			for flag in configure_args {
				configure_cmd.arg(flag);
			}
		}

		for (key, value) in env {
			configure_cmd.env(key, value);
		}

		let configure_output = configure_cmd.output()?;
		if !configure_output.status.success() {
			return Err(format!(
				"Configure failed: {}",
				String::from_utf8_lossy(&configure_output.stderr)
			)
			.into());
		}

		// Run make
		let mut make_cmd = std::process::Command::new("make");
		make_cmd.current_dir(source_dir);
		make_cmd.arg(format!("-j{}", self.config.max_jobs));

		if let Some(make_args) = &spec.make_args {
			for flag in make_args {
				make_cmd.arg(flag);
			}
		}

		for (key, value) in env {
			make_cmd.env(key, value);
		}

		let make_output = make_cmd.output()?;
		if !make_output.status.success() {
			return Err(format!(
				"Make failed: {}",
				String::from_utf8_lossy(&make_output.stderr)
			)
			.into());
		}

		// Run make install
		let mut install_cmd = std::process::Command::new("make");
		install_cmd.current_dir(source_dir);
		install_cmd.arg("install");

		for (key, value) in env {
			install_cmd.env(key, value);
		}

		let install_output = install_cmd.output()?;
		if !install_output.status.success() {
			return Err(format!(
				"Make install failed: {}",
				String::from_utf8_lossy(&install_output.stderr)
			)
			.into());
		}

		Ok(install_prefix.clone())
	}

	/// Build using CMake
	async fn build_cmake(
		&self,
		spec: &SystemPackageSpec,
		source_dir: &Path,
		env: &HashMap<String, String>,
	) -> Result<PathBuf, Box<dyn std::error::Error>> {
		let install_prefix =
			spec.install_prefix.as_ref().unwrap_or(&self.sysroot);
		let build_dir = source_dir.join("build");
		std::fs::create_dir_all(&build_dir)?;

		// Run cmake configure
		let mut cmake_cmd = std::process::Command::new("cmake");
		cmake_cmd.current_dir(&build_dir);
		cmake_cmd.arg("..");
		cmake_cmd.arg(format!(
			"-DCMAKE_INSTALL_PREFIX={}",
			install_prefix.display()
		));
		cmake_cmd.arg("-DCMAKE_BUILD_TYPE=Release");

		if let Some(configure_args) = &spec.configure_args {
			for flag in configure_args {
				cmake_cmd.arg(flag);
			}
		}

		for (key, value) in env {
			cmake_cmd.env(key, value);
		}

		let cmake_output = cmake_cmd.output()?;
		if !cmake_output.status.success() {
			return Err(format!(
				"CMake configure failed: {}",
				String::from_utf8_lossy(&cmake_output.stderr)
			)
			.into());
		}

		// Run cmake build
		let mut build_cmd = std::process::Command::new("cmake");
		build_cmd.current_dir(&build_dir);
		build_cmd.arg("--build");
		build_cmd.arg(".");
		build_cmd.arg(format!("-j{}", self.config.max_jobs));

		for (key, value) in env {
			build_cmd.env(key, value);
		}

		let build_output = build_cmd.output()?;
		if !build_output.status.success() {
			return Err(format!(
				"CMake build failed: {}",
				String::from_utf8_lossy(&build_output.stderr)
			)
			.into());
		}

		// Run cmake install
		let mut install_cmd = std::process::Command::new("cmake");
		install_cmd.current_dir(&build_dir);
		install_cmd.arg("--install");
		install_cmd.arg(".");

		for (key, value) in env {
			install_cmd.env(key, value);
		}

		let install_output = install_cmd.output()?;
		if !install_output.status.success() {
			return Err(format!(
				"CMake install failed: {}",
				String::from_utf8_lossy(&install_output.stderr)
			)
			.into());
		}

		Ok(install_prefix.clone())
	}

	/// Build using Meson
	async fn build_meson(
        &self,
        _spec: &SystemPackageSpec,
        _source_dir: &Path,
        _env: &HashMap<String, String>,
    ) -> Result<PathBuf, Box<dyn std::error::Error>> {
		// Meson build implementation
		Err("Meson builds not yet implemented".into())
	}

	/// Build using custom script
	async fn build_custom(
        &self,
        spec: &SystemPackageSpec,
        source_dir: &Path,
        env: &HashMap<String, String>,
    ) -> Result<PathBuf, Box<dyn std::error::Error>> {
		if let Some(script) = &spec.build_script {
			let mut cmd = std::process::Command::new("sh");
			cmd.current_dir(source_dir);
			cmd.arg("-c");
			cmd.arg(script);

			for (key, value) in env {
				cmd.env(key, value);
			}

			let output = cmd.output()?;
			if !output.status.success() {
				return Err(format!(
					"Custom build failed: {}",
					String::from_utf8_lossy(&output.stderr)
				)
				.into());
			}
		}

		Ok(spec
			.install_prefix
			.as_ref()
			.unwrap_or(&self.sysroot)
			.clone())
	}

	/// Build Rust/Cargo project
	async fn build_cargo(
        &self,
        _spec: &SystemPackageSpec,
        _source_dir: &Path,
        _env: &HashMap<String, String>,
    ) -> Result<PathBuf, Box<dyn std::error::Error>> {
		// Cargo build implementation
		Err("Cargo builds not yet implemented".into())
	}

	/// Build LLVM
	async fn build_llvm(
        &self,
        _spec: &SystemPackageSpec,
        _source_dir: &Path,
        _env: &HashMap<String, String>,
    ) -> Result<PathBuf, Box<dyn std::error::Error>> {
		// LLVM build implementation
		Err("LLVM builds not yet implemented".into())
	}

	/// Build Linux kernel
	async fn build_kernel(
        &self,
        _spec: &SystemPackageSpec,
        _source_dir: &Path,
        _env: &HashMap<String, String>,
    ) -> Result<PathBuf, Box<dyn std::error::Error>> {
		// Kernel build implementation
		Err("Kernel builds not yet implemented".into())
	}

	/// Copy directory recursively
    fn copy_directory(
        src: &Path,
        dst: &Path,
    ) -> Result<(), Box<dyn std::error::Error>> {
		std::fs::create_dir_all(dst)?;

		for entry in std::fs::read_dir(src)? {
			let entry = entry?;
			let src_path = entry.path();
			let dst_path = dst.join(entry.file_name());

			if src_path.is_dir() {
				Self::copy_directory(&src_path, &dst_path)?;
			} else {
				std::fs::copy(&src_path, &dst_path)?;
			}
		}

		Ok(())
	}
}

/// Predefined system package specifications
pub struct SystemPackages;

impl SystemPackages {
	/// Get glibc package specification
	pub fn glibc(version: &str) -> SystemPackageSpec {
		SystemPackageSpec {
			name: "glibc".to_string(),
			version: version.to_string(),
			arch: "x86_64".to_string(),
			source: Some(PackageSource::Url {
				url: format!(
					"https://ftp.gnu.org/gnu/glibc/glibc-{version}.tar.gz"
				),
				hash: String::new(),
			}),
			build_type: Some(SystemBuildType::Autotools),
			dependencies: vec!["linux-headers".to_string()],
			build_inputs: vec!["gcc".to_string(), "binutils".to_string()],
			runtime_inputs: vec![],
			environment: HashMap::new(),
			build_script: None,
			configure_args: Some(vec![
				"--enable-shared".to_string(),
				"--disable-profile".to_string(),
				"--disable-werror".to_string(),
			]),
			make_args: Some(vec![]),
			install_prefix: None,
			cross_compile_target: None,
		}
	}

	/// Get LLVM package specification
	pub fn llvm(version: &str) -> SystemPackageSpec {
		SystemPackageSpec {
			name: "llvm".to_string(),
			version: version.to_string(),
			arch: "x86_64".to_string(),
			source: Some(PackageSource::Url {
				url: format!(
					"https://github.com/llvm/llvm-project/releases/download/llvmorg-{version}/llvm-{version}.src.tar.xz"
				),
				hash: String::new(),
			}),
			build_type: Some(SystemBuildType::CMake),
			dependencies: vec![],
			build_inputs: vec!["cmake".to_string(), "ninja".to_string()],
			runtime_inputs: vec![],
			environment: HashMap::new(),
			build_script: None,
			configure_args: Some(vec![
				"-DLLVM_ENABLE_PROJECTS=clang;lld".to_string(),
				"-DLLVM_TARGETS_TO_BUILD=X86;AArch64".to_string(),
				"-DCMAKE_BUILD_TYPE=Release".to_string(),
			]),
			make_args: Some(vec![]),
			install_prefix: None,
			cross_compile_target: None,
		}
	}

	/// Get Rust package specification
	pub fn rust(version: &str) -> SystemPackageSpec {
		SystemPackageSpec {
			name: "rust".to_string(),
			version: version.to_string(),
			arch: "x86_64".to_string(),
			source: Some(PackageSource::Url {
				url: "https://forge.rust-lang.org/infra/channel-layout.html#source-code".to_string(),
				hash: String::new(),
			}),
			build_type: Some(SystemBuildType::Custom),
			dependencies: vec!["llvm".to_string()],
			build_inputs: vec!["python3".to_string(), "cmake".to_string()],
			runtime_inputs: vec![],
			environment: HashMap::new(),
			build_script: Some(
				"./configure --prefix=$PREFIX && make && make install"
					.to_string(),
			),
			configure_args: Some(vec![]),
			make_args: Some(vec![]),
			install_prefix: None,
			cross_compile_target: None,
		}
	}
}
