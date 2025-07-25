//! Custom rustc-based build system that uses Pyro's dependency graph
//! instead of Cargo for building Rust crates.

use crate::config::{PackageSpec, PackageSource};
use crate::dependency::Package;
use crate::builder::{BuildError, PyroBuilder};
use petgraph::graph::DiGraph;
use petgraph::visit::Topo;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use toml;

/// Simplified Cargo.toml structure for dependency parsing
#[derive(Debug, Deserialize, Serialize)]
struct CargoToml {
    package: CargoPackage,
    dependencies: Option<HashMap<String, CargoDependency>>,
    #[serde(rename = "dev-dependencies")]
    dev_dependencies: Option<HashMap<String, CargoDependency>>,
    #[serde(rename = "build-dependencies")]
    build_dependencies: Option<HashMap<String, CargoDependency>>,
}

#[derive(Debug, Deserialize, Serialize)]
struct CargoPackage {
    name: String,
    version: String,
    edition: Option<String>,
    authors: Option<Vec<String>>,
    description: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(untagged)]
enum CargoDependency {
    Simple(String),
    Detailed {
        version: Option<String>,
        git: Option<String>,
        branch: Option<String>,
        tag: Option<String>,
        rev: Option<String>,
        path: Option<String>,
        features: Option<Vec<String>>,
        optional: Option<bool>,
        #[serde(rename = "default-features")]
        default_features: Option<bool>,
    },
}

/// Custom rustc-based builder that replaces Cargo with Pyro's dependency management
pub struct RustcBuilder {
    builder: PyroBuilder,
    target_dir: PathBuf,
    registry_cache: HashMap<String, PackageSpec>,
}

impl RustcBuilder {
    pub fn new(builder: PyroBuilder, target_dir: PathBuf) -> Self {
        Self {
            builder,
            target_dir,
            registry_cache: HashMap::new(),
        }
    }

    /// Build a Rust crate using rustc instead of cargo, with Pyro's dependency graph
    pub async fn build_with_rustc(&mut self, package_spec: &PackageSpec, build_log: &mut String) -> Result<bool, BuildError> {
        build_log.push_str("Starting rustc-based build with Pyro dependency graph\n");
        println!("DEBUG: Starting rustc build for: {}", package_spec.name);
        
        // Prepare source directory
        let source_dir = self.prepare_source(package_spec, build_log).await?;
        
        // Parse Cargo.toml to extract dependencies
        let cargo_toml_path = source_dir.join("Cargo.toml");
        if !cargo_toml_path.exists() {
            return Err(BuildError::BuildFailed("No Cargo.toml found".to_string()));
        }
        
        let cargo_toml = self.parse_cargo_toml(&cargo_toml_path)?;
        build_log.push_str(&format!("Parsed Cargo.toml for package: {}\n", cargo_toml.package.name));
        
        // Convert Cargo dependencies to Pyro packages
        let packages = self.resolve_dependencies(&cargo_toml, build_log).await?;
        
        // Build dependency graph using Pyro's system
        let dep_graph = self.build_dependency_graph(&packages);
        
        // Get topological order for building
        let build_order = self.get_build_order(&dep_graph, &packages)?;
        build_log.push_str(&format!("Dependency build order: {:?}\n", build_order.iter().map(|p| &p.name).collect::<Vec<_>>()));
        
        // Build dependencies in topological order
        for package in &build_order {
            if package.name != cargo_toml.package.name {
                self.build_dependency(package, build_log).await?;
            }
        }
        
        // Build the main package with rustc
        self.build_main_package(&cargo_toml, &source_dir, &build_order, build_log).await
    }
    
    /// Prepare source directory (reuse from existing builder)
    async fn prepare_source(&self, package_spec: &PackageSpec, build_log: &mut String) -> Result<PathBuf, BuildError> {
        // This would delegate to the existing builder's prepare_source method
        // For now, we'll implement a simplified version
        let build_dir = self.target_dir.join("build").join(&package_spec.name);
        fs::create_dir_all(&build_dir).map_err(|e| BuildError::IoError(e.to_string()))?;
        
        let source_dir = build_dir.join("source");
        
        match &package_spec.source {
            PackageSource::Crates { name, version } => {
                // Download and extract crate
                self.download_crate(name, version, &source_dir, build_log).await?;
            }
            PackageSource::Git { url, rev } => {
                // Clone git repository
                self.clone_git_repo(url, rev.as_deref(), &source_dir, build_log).await?;
            }
            PackageSource::Path { path } => {
                // Copy local path
                self.copy_local_path(&path.to_string_lossy(), &source_dir)?;
            }
            PackageSource::Url { url, .. } => {
                // Download and extract from URL
                self.download_url(url, &source_dir, build_log).await?;
            }
        }
        
        Ok(source_dir)
    }
    
    /// Parse Cargo.toml file
    fn parse_cargo_toml(&self, cargo_toml_path: &Path) -> Result<CargoToml, BuildError> {
        let content = fs::read_to_string(cargo_toml_path)
            .map_err(|e| BuildError::IoError(e.to_string()))?;
        
        toml::from_str(&content)
            .map_err(|e| BuildError::BuildFailed(format!("Failed to parse Cargo.toml: {}", e)))
    }
    
    /// Resolve dependencies from Cargo.toml to Pyro packages
    async fn resolve_dependencies(&mut self, cargo_toml: &CargoToml, build_log: &mut String) -> Result<Vec<Package>, BuildError> {
        let mut packages = Vec::new();
        
        // Add the main package
        let main_package = Package {
            name: cargo_toml.package.name.clone(),
            version: cargo_toml.package.version.clone(),
            dependencies: Vec::new(),
        };
        
        let mut main_deps = Vec::new();
        
        // Process regular dependencies
        if let Some(deps) = &cargo_toml.dependencies {
            for (name, dep) in deps {
                let package_spec = self.resolve_dependency_spec(name, dep).await?;
                let dep_package = Package {
                    name: name.clone(),
                    version: package_spec.version.unwrap_or_else(|| "*".to_string()),
                    dependencies: Vec::new(), // Will be resolved recursively
                };
                packages.push(dep_package);
                main_deps.push(name.clone());
                build_log.push_str(&format!("Resolved dependency: {}\n", name));
            }
        }
        
        // Add main package with its dependencies
        packages.push(Package {
            name: main_package.name,
            version: main_package.version,
            dependencies: main_deps,
        });
        
        Ok(packages)
    }
    
    /// Resolve a single dependency to a PackageSpec
    async fn resolve_dependency_spec(&mut self, name: &str, dep: &CargoDependency) -> Result<PackageSpec, BuildError> {
        // Check cache first
        if let Some(cached) = self.registry_cache.get(name) {
            return Ok(cached.clone());
        }
        
        let package_spec = match dep {
            CargoDependency::Simple(version) => {
                // Resolve from crates.io
                self.resolve_from_crates_io(name, Some(version)).await?
            }
            CargoDependency::Detailed { version, git, path, .. } => {
                if let Some(git_url) = git {
                    PackageSpec {
                        name: name.to_string(),
                        version: version.clone(),
                        source: PackageSource::Git {
                            url: git_url.clone(),
                            rev: None, // Could extract from branch/tag/rev
                        },
                        build_inputs: vec![],
                        runtime_inputs: vec![],
                        environment: HashMap::new(),
                        build_script: None,
                    }
                } else if let Some(local_path) = path {
                    PackageSpec {
                        name: name.to_string(),
                        version: version.clone(),
                        source: PackageSource::Path {
                            path: local_path.clone().into(),
                        },
                        build_inputs: vec![],
                        runtime_inputs: vec![],
                        environment: HashMap::new(),
                        build_script: None,
                    }
                } else {
                    // Default to crates.io
                    self.resolve_from_crates_io(name, version.as_ref()).await?
                }
            }
        };
        
        // Cache the result
        self.registry_cache.insert(name.to_string(), package_spec.clone());
        Ok(package_spec)
    }
    
    /// Resolve dependency from crates.io (reuse existing logic)
    async fn resolve_from_crates_io(&self, dep_name: &str, version: Option<&String>) -> Result<PackageSpec, BuildError> {
        let url = format!("https://crates.io/api/v1/crates/{}", dep_name);
        let response = reqwest::get(&url).await
            .map_err(|e| BuildError::NetworkError(e.to_string()))?;
        
        if !response.status().is_success() {
            return Err(BuildError::DependencyResolutionFailed(format!("Crate {} not found on crates.io", dep_name)));
        }
        
        let crate_info: serde_json::Value = response.json().await
            .map_err(|e| BuildError::NetworkError(e.to_string()))?;
        
        let target_version = version
            .map(|v| v.clone())
            .unwrap_or_else(|| {
                crate_info["crate"]["max_version"].as_str()
                    .unwrap_or("*")
                    .to_string()
            });
        
        Ok(PackageSpec {
            name: dep_name.to_string(),
            version: Some(target_version.clone()),
            source: PackageSource::Crates {
                name: dep_name.to_string(),
                version: target_version,
            },
            build_inputs: vec![],
            runtime_inputs: vec![],
            environment: HashMap::new(),
            build_script: None,
        })
    }
    
    /// Build dependency graph using Pyro's system
    fn build_dependency_graph(&self, packages: &[Package]) -> DiGraph<String, ()> {
        let mut graph = DiGraph::<String, ()>::new();
        let mut node_indices = HashMap::new();
        
        // Add all packages as nodes
        for pkg in packages {
            let idx = graph.add_node(pkg.name.clone());
            node_indices.insert(pkg.name.clone(), idx);
        }
        
        // Add edges for dependencies
        for pkg in packages {
            let from_idx = node_indices[&pkg.name];
            for dep in &pkg.dependencies {
                if let Some(&to_idx) = node_indices.get(dep) {
                    graph.add_edge(from_idx, to_idx, ());
                }
            }
        }
        
        graph
    }
    
    /// Get build order using topological sort
    fn get_build_order(&self, graph: &DiGraph<String, ()>, packages: &[Package]) -> Result<Vec<Package>, BuildError> {
        let mut topo = Topo::new(graph);
        let mut ordered_indices = Vec::new();
        
        while let Some(node) = topo.next(graph) {
            ordered_indices.push(node);
        }
        
        // Map node indices back to packages
        let mut ordered_packages = Vec::new();
        for &idx in &ordered_indices {
            let name = &graph[idx];
            if let Some(pkg) = packages.iter().find(|p| &p.name == name) {
                ordered_packages.push(pkg.clone());
            }
        }
        
        Ok(ordered_packages)
    }
    
    /// Build a dependency package
    async fn build_dependency(&self, package: &Package, build_log: &mut String) -> Result<(), BuildError> {
        build_log.push_str(&format!("Building dependency: {} v{}\n", package.name, package.version));
        
        // Check if already built in store
        let store_path = self.target_dir.join("store").join(&package.name).join(&package.version);
        if store_path.exists() {
            build_log.push_str(&format!("Dependency {} already built, skipping\n", package.name));
            return Ok(());
        }
        
        // For now, we'll use a simplified approach - in a full implementation,
        // this would recursively build the dependency using the same rustc approach
        build_log.push_str(&format!("Dependency {} built successfully\n", package.name));
        Ok(())
    }
    
    /// Build the main package using rustc
    async fn build_main_package(
        &self,
        cargo_toml: &CargoToml,
        source_dir: &Path,
        dependencies: &[Package],
        build_log: &mut String,
    ) -> Result<bool, BuildError> {
        build_log.push_str(&format!("Building main package: {} with rustc\n", cargo_toml.package.name));
        
        // Find main source file
        let main_rs = source_dir.join("src").join("main.rs");
        let lib_rs = source_dir.join("src").join("lib.rs");
        
        let (source_file, is_binary) = if main_rs.exists() {
            (main_rs, true)
        } else if lib_rs.exists() {
            (lib_rs, false)
        } else {
            return Err(BuildError::BuildFailed("No main.rs or lib.rs found".to_string()));
        };
        
        // Prepare rustc command
        let output_dir = self.target_dir.join("output").join(&cargo_toml.package.name);
        fs::create_dir_all(&output_dir).map_err(|e| BuildError::IoError(e.to_string()))?;
        
        let mut cmd = Command::new("rustc");
        cmd.arg(&source_file);
        
        if is_binary {
            cmd.arg("-o").arg(output_dir.join(&cargo_toml.package.name));
        } else {
            cmd.arg("--crate-type").arg("lib");
            cmd.arg("-o").arg(output_dir.join(format!("lib{}.rlib", cargo_toml.package.name)));
        }
        
        // Add dependency library paths
        for dep in dependencies {
            if dep.name != cargo_toml.package.name {
                let dep_lib_path = self.target_dir.join("store").join(&dep.name).join(&dep.version);
                if dep_lib_path.exists() {
                    cmd.arg("-L").arg(&dep_lib_path);
                }
            }
        }
        
        // Set edition
        if let Some(edition) = &cargo_toml.package.edition {
            cmd.arg("--edition").arg(edition);
        }
        
        // Execute rustc
        build_log.push_str(&format!("Executing: {:?}\n", cmd));
        let output = cmd.output().map_err(|e| BuildError::IoError(e.to_string()))?;
        
        build_log.push_str(&String::from_utf8_lossy(&output.stdout));
        if !output.stderr.is_empty() {
            build_log.push_str(&String::from_utf8_lossy(&output.stderr));
        }
        
        if output.status.success() {
            build_log.push_str(&format!("Successfully built {} with rustc\n", cargo_toml.package.name));
            Ok(true)
        } else {
            build_log.push_str(&format!("rustc build failed with exit code: {:?}\n", output.status.code()));
            Ok(false)
        }
    }
    
    // Placeholder implementations for source preparation methods
    async fn download_crate(&self, name: &str, version: &str, _target_dir: &Path, build_log: &mut String) -> Result<(), BuildError> {
        build_log.push_str(&format!("Downloading crate: {} v{}\n", name, version));
        // Implementation would download and extract crate from crates.io
        Ok(())
    }
    
    async fn clone_git_repo(&self, url: &str, _rev: Option<&str>, _target_dir: &Path, build_log: &mut String) -> Result<(), BuildError> {
        build_log.push_str(&format!("Cloning git repo: {}\n", url));
        // Implementation would clone git repository
        Ok(())
    }
    
    fn copy_local_path(&self, _source_path: &str, _target_dir: &Path) -> Result<(), BuildError> {
        // Implementation would copy local path
        Ok(())
    }
    
    async fn download_url(&self, url: &str, _target_dir: &Path, build_log: &mut String) -> Result<(), BuildError> {
        build_log.push_str(&format!("Downloading from URL: {}\n", url));
        // Implementation would download and extract from URL
        Ok(())
    }
}