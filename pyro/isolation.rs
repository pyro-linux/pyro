//! Environment isolation for Pyro package manager
//! Handles creating isolated environments for VMs and containers

use crate::config::PyroConfig;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IsolatedEnvironment {
	pub name: String,
	pub base_image: Option<String>,
	pub packages: Vec<String>,
	pub environment_vars: HashMap<String, String>,
	pub mount_points: Vec<MountPoint>,
	pub network_config: NetworkConfig,
	pub resource_limits: ResourceLimits,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MountPoint {
	pub host_path: PathBuf,
	pub container_path: PathBuf,
	pub read_only: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkConfig {
	pub isolated: bool,
	pub ports: Vec<PortMapping>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortMapping {
	pub host_port: u16,
	pub container_port: u16,
	pub protocol: String, // tcp, udp
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceLimits {
	pub memory_mb: Option<u64>,
	pub cpu_cores: Option<f64>,
	pub disk_mb: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct EnvironmentBuilder {
	config: PyroConfig,
	store_path: PathBuf,
}

impl EnvironmentBuilder {
	pub fn new(config: PyroConfig, store_path: PathBuf) -> Self {
		Self { config, store_path }
	}

	/// Create an isolated environment directory
	pub async fn create_environment(
		&self,
		env_spec: &IsolatedEnvironment,
	) -> Result<PathBuf, Box<dyn std::error::Error>> {
		let output_dir = &self.store_path.join("environments");
		std::fs::create_dir_all(output_dir)?;
		self.create_environment_with_output(env_spec, output_dir)
			.await
	}

	/// Create an isolated environment directory with custom output
	pub async fn create_environment_with_output(
		&self,
		env_spec: &IsolatedEnvironment,
		output_dir: &Path,
	) -> Result<PathBuf, Box<dyn std::error::Error>> {
		println!("Creating isolated environment: {}", env_spec.name);

		// Create environment directory structure
		let env_dir = output_dir.join(&env_spec.name);
		std::fs::create_dir_all(&env_dir)?;

		// Create standard Unix directory structure
		self.create_unix_structure(&env_dir)?;

		// Install packages into the environment
		self.install_packages_to_env(&env_spec.packages, &env_dir)
			.await?;

		// Set up environment configuration
		self.setup_environment_config(env_spec, &env_dir)?;

		// Generate container/VM configuration files
		self.generate_container_configs(env_spec, &env_dir)?;

		println!(
			"✅ Environment '{}' created at: {}",
			env_spec.name,
			env_dir.display()
		);
		Ok(env_dir)
	}

	/// Create standard Unix directory structure
	fn create_unix_structure(
		&self,
		env_dir: &Path,
	) -> Result<(), Box<dyn std::error::Error>> {
		let dirs = [
			"bin",
			"sbin",
			"lib",
			"lib64",
			"usr/bin",
			"usr/sbin",
			"usr/lib",
			"usr/lib64",
			"usr/include",
			"usr/share",
			"etc",
			"var",
			"tmp",
			"home",
			"root",
			"dev",
			"proc",
			"sys",
		];

		for dir in &dirs {
			std::fs::create_dir_all(env_dir.join(dir))?;
		}

		// Create essential symlinks
		#[cfg(unix)]
		{
			let lib64_path = env_dir.join("lib64");
			let lib_path = env_dir.join("lib");
			if !lib64_path.exists() {
				std::os::unix::fs::symlink(&lib_path, &lib64_path)?;
			}
		}

		Ok(())
	}

	/// Install packages into the isolated environment
	async fn install_packages_to_env(
		&self,
		packages: &[String],
		env_dir: &Path,
	) -> Result<(), Box<dyn std::error::Error>> {
		for package in packages {
			println!("Installing package '{package}' into environment");

			// Find package in store
			let package_path = self.find_package_in_store(package)?;

			// Copy package contents to environment
			self.copy_package_to_env(&package_path, env_dir)?;
		}

		Ok(())
	}

	/// Find a package in the Pyro store
	fn find_package_in_store(
		&self,
		package_name: &str,
	) -> Result<PathBuf, Box<dyn std::error::Error>> {
		// Search through store for package
		let store_entries = std::fs::read_dir(&self.store_path)?;

		for entry in store_entries {
			let entry = entry?;
			let path = entry.path();

			if path.is_dir() {
				let metadata_path = path.join("metadata.json");
				if metadata_path.exists() {
					let metadata_content =
						std::fs::read_to_string(&metadata_path)?;
					if metadata_content
						.contains(&format!("\"name\":\"{package_name}\""))
					{
						return Ok(path);
					}
				}
			}
		}

		Err(format!("Package '{package_name}' not found in store").into())
	}

	/// Copy package contents to environment
	fn copy_package_to_env(
		&self,
		package_path: &Path,
		env_dir: &Path,
	) -> Result<(), Box<dyn std::error::Error>> {
		// Copy binaries
		let src_bin = package_path.join("bin");
		let dst_bin = env_dir.join("usr/bin");
		if src_bin.exists() {
			Self::copy_directory_contents(&src_bin, &dst_bin)?;
		}

		// Copy libraries
		let src_lib = package_path.join("lib");
		let dst_lib = env_dir.join("usr/lib");
		if src_lib.exists() {
			Self::copy_directory_contents(&src_lib, &dst_lib)?;
		}

		// Copy shared data
		let src_share = package_path.join("share");
		let dst_share = env_dir.join("usr/share");
		if src_share.exists() {
			Self::copy_directory_contents(&src_share, &dst_share)?;
		}

		Ok(())
	}

	/// Copy directory contents
	fn copy_directory_contents(
		src: &Path,
		dst: &Path,
	) -> Result<(), Box<dyn std::error::Error>> {
		std::fs::create_dir_all(dst)?;

		for entry in std::fs::read_dir(src)? {
			let entry = entry?;
			let src_path = entry.path();
			let dst_path = dst.join(entry.file_name());

			if src_path.is_dir() {
				Self::copy_directory_contents(&src_path, &dst_path)?;
			} else {
				std::fs::copy(&src_path, &dst_path)?;

				// Preserve executable permissions
				#[cfg(unix)]
				{
					use std::os::unix::fs::PermissionsExt;
					let src_perms = std::fs::metadata(&src_path)?.permissions();
					std::fs::set_permissions(&dst_path, src_perms)?;
				}
			}
		}

		Ok(())
	}

	/// Set up environment configuration
	fn setup_environment_config(
		&self,
		env_spec: &IsolatedEnvironment,
		env_dir: &Path,
	) -> Result<(), Box<dyn std::error::Error>> {
		// Create /etc/environment
		let env_file = env_dir.join("etc/environment");
		let mut env_content = String::new();

		// Add standard PATH
		env_content.push_str(
			"PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin\n",
		);

		// Add custom environment variables
		for (key, value) in &env_spec.environment_vars {
			env_content.push_str(&format!("{key}={value}\n"));
		}

		std::fs::write(&env_file, env_content)?;

		// Create basic /etc/passwd
		let passwd_file = env_dir.join("etc/passwd");
		let passwd_content = "root:x:0:0:root:/root:/bin/sh\nnobody:x:65534:65534:nobody:/nonexistent:/usr/sbin/nologin\n";
		std::fs::write(&passwd_file, passwd_content)?;

		// Create basic /etc/group
		let group_file = env_dir.join("etc/group");
		let group_content = "root:x:0:\nnogroup:x:65534:\n";
		std::fs::write(&group_file, group_content)?;

		Ok(())
	}

	/// Generate container/VM configuration files
	fn generate_container_configs(
		&self,
		env_spec: &IsolatedEnvironment,
		env_dir: &Path,
	) -> Result<(), Box<dyn std::error::Error>> {
		// Generate Dockerfile
		self.generate_dockerfile(env_spec, env_dir)?;

		// Generate Docker Compose
		self.generate_docker_compose(env_spec, env_dir)?;

		// Generate VM startup script
		self.generate_vm_script(env_spec, env_dir)?;

		Ok(())
	}

	/// Generate Dockerfile
	fn generate_dockerfile(
		&self,
		env_spec: &IsolatedEnvironment,
		env_dir: &Path,
	) -> Result<(), Box<dyn std::error::Error>> {
		let dockerfile_path = env_dir.join("Dockerfile");
		let mut dockerfile_content = String::new();

		// Base image
		let base_image = env_spec.base_image.as_deref().unwrap_or("scratch");
		dockerfile_content.push_str(&format!("FROM {base_image}\n\n"));

		// Copy environment
		dockerfile_content.push_str("COPY . /\n\n");

		// Set environment variables
		for (key, value) in &env_spec.environment_vars {
			dockerfile_content.push_str(&format!("ENV {key}={value}\n"));
		}

		// Set PATH
		dockerfile_content.push_str(
			"ENV PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin\n\n",
		);

		// Expose ports
		for port in &env_spec.network_config.ports {
			dockerfile_content
				.push_str(&format!("EXPOSE {}\n", port.container_port));
		}

		dockerfile_content.push_str("\nCMD [\"/bin/sh\"]\n");

		std::fs::write(&dockerfile_path, dockerfile_content)?;
		Ok(())
	}

	/// Generate Docker Compose file
	fn generate_docker_compose(
		&self,
		env_spec: &IsolatedEnvironment,
		env_dir: &Path,
	) -> Result<(), Box<dyn std::error::Error>> {
		let compose_path = env_dir.join("docker-compose.yml");
		let mut compose_content = String::new();

		compose_content.push_str("version: '3.8'\n\n");
		compose_content.push_str("services:\n");
		compose_content.push_str(&format!("  {}:\n", env_spec.name));
		compose_content.push_str("    build: .\n");

		// Port mappings
		if !env_spec.network_config.ports.is_empty() {
			compose_content.push_str("    ports:\n");
			for port in &env_spec.network_config.ports {
				compose_content.push_str(&format!(
					"      - \"{}:{}\"\n",
					port.host_port, port.container_port
				));
			}
		}

		// Volume mounts
		if !env_spec.mount_points.is_empty() {
			compose_content.push_str("    volumes:\n");
			for mount in &env_spec.mount_points {
				let ro = if mount.read_only { ":ro" } else { "" };
				compose_content.push_str(&format!(
					"      - \"{}:{}{}\"\n",
					mount.host_path.display(),
					mount.container_path.display(),
					ro
				));
			}
		}

		// Resource limits
		if env_spec.resource_limits.memory_mb.is_some()
			|| env_spec.resource_limits.cpu_cores.is_some()
		{
			compose_content.push_str("    deploy:\n");
			compose_content.push_str("      resources:\n");
			compose_content.push_str("        limits:\n");

			if let Some(memory) = env_spec.resource_limits.memory_mb {
				compose_content
					.push_str(&format!("          memory: {memory}M\n"));
			}

			if let Some(cpus) = env_spec.resource_limits.cpu_cores {
				compose_content
					.push_str(&format!("          cpus: '{cpus}'\n"));
			}
		}

		std::fs::write(&compose_path, compose_content)?;
		Ok(())
	}

	/// Generate VM startup script
	fn generate_vm_script(
		&self,
		env_spec: &IsolatedEnvironment,
		env_dir: &Path,
	) -> Result<(), Box<dyn std::error::Error>> {
		let script_path = env_dir.join("start-vm.sh");
		let mut script_content = String::new();

		script_content.push_str("#!/bin/bash\n\n");
		script_content.push_str(&format!(
			"# VM startup script for {}\n\n",
			env_spec.name
		));

		// QEMU command example
		script_content.push_str("qemu-system-x86_64 \\\n");
		script_content.push_str("  -m 1024 \\\n");
		script_content.push_str("  -smp 2 \\\n");
		script_content.push_str("  -kernel vmlinuz \\\n");
		script_content.push_str("  -initrd initrd.img \\\n");
		script_content
			.push_str("  -append \"root=/dev/ram0 init=/sbin/init\" \\\n");
		script_content.push_str("  -netdev user,id=net0 \\\n");
		script_content.push_str("  -device e1000,netdev=net0\n");

		std::fs::write(&script_path, script_content)?;

		// Make script executable
		#[cfg(unix)]
		{
			use std::os::unix::fs::PermissionsExt;
			let mut perms = std::fs::metadata(&script_path)?.permissions();
			perms.set_mode(0o755);
			std::fs::set_permissions(&script_path, perms)?;
		}

		Ok(())
	}

	/// Create a minimal Linux environment
	pub async fn create_minimal_linux(
		&self,
		output_dir: &Path,
	) -> Result<PathBuf, Box<dyn std::error::Error>> {
		let env_spec = IsolatedEnvironment {
			name: "minimal-linux".to_string(),
			base_image: Some("scratch".to_string()),
			packages: vec![
				"glibc".to_string(),
				"busybox".to_string(),
				"linux-headers".to_string(),
			],
			environment_vars: HashMap::new(),
			mount_points: vec![],
			network_config: NetworkConfig {
				isolated: true,
				ports: vec![],
			},
			resource_limits: ResourceLimits {
				memory_mb: Some(512),
				cpu_cores: Some(1.0),
				disk_mb: Some(1024),
			},
		};

		self.create_environment_with_output(&env_spec, output_dir)
			.await
	}

	/// Enter an isolated environment
	pub async fn enter_environment(
		&self,
		name: &str,
	) -> Result<(), Box<dyn std::error::Error>> {
		let env_dir = self.store_path.join("environments").join(name);

		if !env_dir.exists() {
			return Err(format!("Environment '{name}' does not exist").into());
		}

		println!("To enter the environment, run:");
		println!(
			"  docker run -it --rm -v {}:/workspace {}",
			env_dir.display(),
			name
		);
		println!("Or use the generated scripts in: {}", env_dir.display());

		Ok(())
	}

	/// Remove an isolated environment
	pub async fn remove_environment(
		&self,
		name: &str,
	) -> Result<(), Box<dyn std::error::Error>> {
		let env_dir = self.store_path.join("environments").join(name);

		if !env_dir.exists() {
			return Err(format!("Environment '{name}' does not exist").into());
		}

		std::fs::remove_dir_all(&env_dir)?;
		println!("Removed environment: {name}");

		Ok(())
	}

	/// List all isolated environments
	pub async fn list_environments(
		&self,
	) -> Result<Vec<IsolatedEnvironment>, Box<dyn std::error::Error>> {
		let envs_dir = self.store_path.join("environments");
		let mut environments = Vec::new();

		if !envs_dir.exists() {
			return Ok(environments);
		}

		for entry in std::fs::read_dir(&envs_dir)? {
			let entry = entry?;
			let path = entry.path();

			if path.is_dir() {
				let name = path
					.file_name()
					.and_then(|n| n.to_str())
					.unwrap_or("unknown")
					.to_string();

				// Try to read environment metadata
				let metadata_path = path.join("environment.json");
				if metadata_path.exists() {
					let metadata_content =
						std::fs::read_to_string(&metadata_path)?;
					if let Ok(env) = serde_json::from_str::<IsolatedEnvironment>(
						&metadata_content,
					) {
						environments.push(env);
						continue;
					}
				}

				// Fallback: create basic environment info
				environments.push(IsolatedEnvironment {
					name,
					base_image: None,
					packages: vec![],
					environment_vars: HashMap::new(),
					mount_points: vec![],
					network_config: NetworkConfig {
						isolated: true,
						ports: vec![],
					},
					resource_limits: ResourceLimits {
						memory_mb: None,
						cpu_cores: None,
						disk_mb: None,
					},
				});
			}
		}

		Ok(environments)
	}

	/// Create a development environment
	pub async fn create_dev_environment(
		&self,
		output_dir: &Path,
	) -> Result<PathBuf, Box<dyn std::error::Error>> {
		let env_spec = IsolatedEnvironment {
			name: "dev-environment".to_string(),
			base_image: Some("scratch".to_string()),
			packages: vec![
				"glibc".to_string(),
				"gcc".to_string(),
				"llvm".to_string(),
				"rust".to_string(),
				"git".to_string(),
				"make".to_string(),
				"cmake".to_string(),
			],
			environment_vars: {
				let mut env = HashMap::new();
				env.insert("CC".to_string(), "gcc".to_string());
				env.insert("CXX".to_string(), "g++".to_string());
				env
			},
			mount_points: vec![MountPoint {
				host_path: PathBuf::from("/tmp"),
				container_path: PathBuf::from("/tmp"),
				read_only: false,
			}],
			network_config: NetworkConfig {
				isolated: false,
				ports: vec![],
			},
			resource_limits: ResourceLimits {
				memory_mb: Some(2048),
				cpu_cores: Some(2.0),
				disk_mb: Some(4096),
			},
		};

		self.create_environment_with_output(&env_spec, output_dir)
			.await
	}
}
