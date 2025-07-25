use crate::config::{BuildConfig, PackageSpec, PackageSource};
use crate::store::{BuildResult, PyroStore, StorePath};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Instant, SystemTime};
use tokio::sync::Semaphore;
use std::sync::Arc;

/// Nix-like package builder with sandboxing and reproducibility
#[derive(Debug)]
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
    DependencyNotFound(String),
    BuildFailed(String),
    SandboxViolation(String),
    IoError(std::io::Error),
}

impl std::fmt::Display for BuildError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BuildError::DependencyNotFound(dep) => write!(f, "Dependency not found: {}", dep),
            BuildError::BuildFailed(msg) => write!(f, "Build failed: {}", msg),
            BuildError::SandboxViolation(msg) => write!(f, "Sandbox violation: {}", msg),
            BuildError::IoError(e) => write!(f, "IO error: {}", e),
        }
    }
}

impl std::error::Error for BuildError {}

impl From<std::io::Error> for BuildError {
    fn from(error: std::io::Error) -> Self {
        BuildError::IoError(error)
    }
}

impl PyroBuilder {
    pub fn new(config: BuildConfig, store: Arc<tokio::sync::Mutex<PyroStore>>) -> Self {
        let build_semaphore = Arc::new(Semaphore::new(config.max_jobs));
        
        PyroBuilder {
            config,
            store,
            build_semaphore,
        }
    }

    /// Build a package with all its dependencies
    pub async fn build_package(&self, spec: &PackageSpec) -> Result<BuildResult, BuildError> {
        Box::pin(self.build_package_impl(spec)).await
    }

    async fn build_package_impl(&self, spec: &PackageSpec) -> Result<BuildResult, BuildError> {
        // Check if package already exists in store
        {
            let mut store = self.store.lock().await;
            if store.package_exists(spec) {
                if let Some(store_path) = store.get_package(spec) {
                    return Ok(BuildResult {
                        store_path: store_path.clone(),
                        build_log: "Package already exists in store".to_string(),
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
            let dep_result = Box::pin(self.build_package_impl(&dep_spec)).await?;
            if dep_result.success {
                dependencies.push(dep_result.store_path);
            } else {
                return Err(BuildError::DependencyNotFound(dep_name.clone()));
            }
        }

        // Create build context
        let build_context = self.create_build_context(spec, dependencies).await?;

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
        let build_dir = std::env::temp_dir().join(format!("pyro-build-{}", spec.name));
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
        if self.config.pure_builds {
            environment.insert("SOURCE_DATE_EPOCH".to_string(), "1".to_string());
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
    async fn execute_build(&self, context: &BuildContext) -> Result<BuildResult, BuildError> {
        let _start_time = Instant::now();
        let mut build_log = String::new();

        // Download/prepare source
        self.prepare_source(context, &mut build_log).await?;

        // Execute build script
        let success = if let Some(build_script) = &context.package.build_script {
            self.run_build_script(context, build_script, &mut build_log).await?
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
                dependencies: context.dependencies.iter().map(|d| d.hash.clone()).collect(),
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
    async fn prepare_source(&self, context: &BuildContext, build_log: &mut String) -> Result<(), BuildError> {
        build_log.push_str(&format!("Preparing source for {}\n", context.package.name));

        match &context.package.source {
            PackageSource::Crates { name, version } => {
                let url = format!("https://static.crates.io/crates/{}/{}-{}.crate", name, name, version);
                self.download_and_extract(&url, &context.build_dir, build_log).await?
            }
            PackageSource::Git { url, rev } => {
                self.clone_git_repo(url, rev.as_deref(), &context.build_dir, build_log).await?
            }
            PackageSource::Path { path } => {
                self.copy_local_path(path, &context.build_dir, build_log).await?
            }
            PackageSource::Url { url, hash: _ } => {
                self.download_and_extract(url, &context.build_dir, build_log).await?
            }
        }

        Ok(())
    }

    /// Download and extract source archive
    async fn download_and_extract(&self, url: &str, dest: &Path, build_log: &mut String) -> Result<(), BuildError> {
        build_log.push_str(&format!("Downloading from {}\n", url));
        
        let output = Command::new("curl")
            .arg("-L")
            .arg("-o")
            .arg("source.tar.gz")
            .arg(url)
            .current_dir(dest)
            .output()?;

        if !output.status.success() {
            return Err(BuildError::BuildFailed("Failed to download source".to_string()));
        }

        build_log.push_str("Extracting source archive\n");
        let output = Command::new("tar")
            .arg("-xzf")
            .arg("source.tar.gz")
            .current_dir(dest)
            .output()?;

        if !output.status.success() {
            return Err(BuildError::BuildFailed("Failed to extract source".to_string()));
        }

        Ok(())
    }

    /// Clone git repository
    async fn clone_git_repo(&self, url: &str, rev: Option<&str>, dest: &Path, build_log: &mut String) -> Result<(), BuildError> {
        build_log.push_str(&format!("Cloning git repository {}\n", url));
        
        let mut cmd = Command::new("git");
        cmd.arg("clone").arg(url).arg("source").current_dir(dest);
        
        let output = cmd.output()?;
        if !output.status.success() {
            return Err(BuildError::BuildFailed("Failed to clone repository".to_string()));
        }

        if let Some(rev) = rev {
            build_log.push_str(&format!("Checking out revision {}\n", rev));
            let output = Command::new("git")
                .arg("checkout")
                .arg(rev)
                .current_dir(dest.join("source"))
                .output()?;

            if !output.status.success() {
                return Err(BuildError::BuildFailed("Failed to checkout revision".to_string()));
            }
        }

        Ok(())
    }

    /// Copy local path
    async fn copy_local_path(&self, src: &Path, dest: &Path, build_log: &mut String) -> Result<(), BuildError> {
        build_log.push_str(&format!("Copying from local path {}\n", src.display()));
        
        let output = Command::new("cp")
            .arg("-r")
            .arg(src)
            .arg(dest.join("source"))
            .output()?;

        if !output.status.success() {
            return Err(BuildError::BuildFailed("Failed to copy local path".to_string()));
        }

        Ok(())
    }

    /// Run custom build script
    async fn run_build_script(&self, context: &BuildContext, script: &str, build_log: &mut String) -> Result<bool, BuildError> {
        build_log.push_str("Running custom build script\n");
        
        let mut cmd = Command::new("sh");
        cmd.arg("-c").arg(script)
            .current_dir(&context.build_dir)
            .envs(&context.environment);

        if self.config.sandbox {
            // Add sandboxing flags (simplified)
            cmd.env("PATH", "/usr/bin:/bin");
        }

        let output = cmd.output()?;
        build_log.push_str(&String::from_utf8_lossy(&output.stdout));
        build_log.push_str(&String::from_utf8_lossy(&output.stderr));

        Ok(output.status.success())
    }

    /// Run default build (cargo build for Rust packages)
    async fn run_default_build(&self, context: &BuildContext, build_log: &mut String) -> Result<bool, BuildError> {
        build_log.push_str("Running default build (cargo)\n");
        
        let source_dir = context.build_dir.join("source");
        let mut cmd = Command::new("cargo");
        cmd.arg("build")
            .arg("--release")
            .current_dir(&source_dir)
            .envs(&context.environment);

        let output = cmd.output()?;
        build_log.push_str(&String::from_utf8_lossy(&output.stdout));
        build_log.push_str(&String::from_utf8_lossy(&output.stderr));

        Ok(output.status.success())
    }

    /// Install package to store path
    async fn install_package(&self, context: &BuildContext, build_log: &mut String) -> Result<(), BuildError> {
        build_log.push_str(&format!("Installing to {}\n", context.store_path.display()));
        
        let source_dir = context.build_dir.join("source");
        let target_dir = source_dir.join("target/release");
        
        if target_dir.exists() {
            let output = Command::new("cp")
                .arg("-r")
                .arg(&target_dir)
                .arg(&context.store_path)
                .output()?;

            if !output.status.success() {
                return Err(BuildError::BuildFailed("Failed to install package".to_string()));
            }
        }

        Ok(())
    }

    /// Resolve dependency specification
    async fn resolve_dependency(&self, dep_name: &str) -> Result<PackageSpec, BuildError> {
        // Simplified dependency resolution
        // In a real implementation, this would query a package registry
        Ok(PackageSpec {
            name: dep_name.to_string(),
            version: Some("latest".to_string()),
            source: PackageSource::Crates {
                name: dep_name.to_string(),
                version: "latest".to_string(),
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
}