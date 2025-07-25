use clap::{Parser, Subcommand};
use crate::config::{PackageSource, PackageSpec, PyroConfig};
use crate::store::PyroStore;
use crate::builder::PyroBuilder;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Parser)]
#[command(name = "pyro")]
#[command(about = "A Nix-like package manager for Rust")]
#[command(version = "0.1.0")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
    
    /// Configuration file path
    #[arg(short, long, default_value = "pyro.toml")]
    pub config: PathBuf,
    
    /// Store path
    #[arg(short, long)]
    pub store_path: Option<PathBuf>,
    
    /// Verbose output
    #[arg(short, long)]
    pub verbose: bool,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Install packages
    Install {
        /// Package specifications
        packages: Vec<String>,
        /// Install to user profile
        #[arg(long)]
        user: bool,
    },
    /// Remove packages
    Remove {
        /// Package names to remove
        packages: Vec<String>,
    },
    /// Update packages
    Update {
        /// Specific packages to update (all if none specified)
        packages: Vec<String>,
    },
    /// Search for packages
    Search {
        /// Search query
        query: String,
    },
    /// Show package information
    Show {
        /// Package name
        package: String,
    },
    /// List installed packages
    List {
        /// Show only user packages
        #[arg(long)]
        user: bool,
    },
    /// Garbage collect unused packages
    Gc {
        /// Dry run (don't actually remove)
        #[arg(long)]
        dry_run: bool,
    },
    /// Show store statistics
    StoreInfo,
    /// Build package from source
    Build {
        /// Package specification file
        spec_file: PathBuf,
    },
    /// Initialize new configuration
    Init {
        /// Configuration file path
        #[arg(short, long, default_value = "pyro.toml")]
        config: PathBuf,
    },
    /// Show dependency graph
    Graph {
        /// Package name
        package: String,
        /// Output format (dot, json)
        #[arg(short, long, default_value = "dot")]
        format: String,
    },
}

pub struct PyroApp {
    config: PyroConfig,
    store: Arc<tokio::sync::Mutex<PyroStore>>,
    builder: PyroBuilder,
}

impl PyroApp {
    pub async fn new(cli: &Cli) -> Result<Self, Box<dyn std::error::Error>> {
        // Load or create configuration
        let config = if cli.config.exists() {
            PyroConfig::from_file(&cli.config)?
        } else {
            PyroConfig::default()
        };

        // Override store path if provided
        let mut store_config = config.store_config.clone();
        if let Some(store_path) = &cli.store_path {
            store_config.store_path = store_path.clone();
        }

        // Initialize store
        let store = Arc::new(tokio::sync::Mutex::new(PyroStore::new(store_config)?));
        
        // Initialize builder
        let builder = PyroBuilder::new(config.build_config.clone(), store.clone());

        Ok(PyroApp {
            config,
            store,
            builder,
        })
    }

    pub async fn run(&mut self, command: Commands) -> Result<(), Box<dyn std::error::Error>> {
        match command {
            Commands::Install { packages, user } => {
                self.install_packages(packages, user).await
            }
            Commands::Remove { packages } => {
                self.remove_packages(packages).await
            }
            Commands::Update { packages } => {
                self.update_packages(packages).await
            }
            Commands::Search { query } => {
                self.search_packages(query).await
            }
            Commands::Show { package } => {
                self.show_package(package).await
            }
            Commands::List { user } => {
                self.list_packages(user).await
            }
            Commands::Gc { dry_run } => {
                self.garbage_collect(dry_run).await
            }
            Commands::StoreInfo => {
                self.show_store_info().await
            }
            Commands::Build { spec_file } => {
                self.build_package(spec_file).await
            }
            Commands::Init { config } => {
                self.init_config(config).await
            }
            Commands::Graph { package, format } => {
                self.show_dependency_graph(package, format).await
            }
        }
    }

    async fn install_packages(&mut self, packages: Vec<String>, _user: bool) -> Result<(), Box<dyn std::error::Error>> {
        println!("Installing packages: {:?}", packages);
        
        for package_str in packages {
            let spec = self.parse_package_spec(&package_str)?;
            println!("Building package: {}", spec.name);
            
            let result = self.builder.build_package(&spec).await;
            match result {
                Ok(build_result) => {
                    if build_result.success {
                        println!("✅ Successfully installed {}", spec.name);
                        
                        // Add to store
                        let mut store = self.store.lock().await;
                        store.add_package(&spec, build_result)?;
                    } else {
                        println!("❌ Failed to install {}", spec.name);
                        println!("Build log:\n{}", build_result.build_log);
                    }
                }
                Err(e) => {
                    println!("❌ Error installing {}: {}", spec.name, e);
                }
            }
        }
        
        Ok(())
    }

    async fn remove_packages(&mut self, packages: Vec<String>) -> Result<(), Box<dyn std::error::Error>> {
        println!("Removing packages: {:?}", packages);
        // Implementation would remove packages from store and update configuration
        Ok(())
    }

    async fn update_packages(&mut self, packages: Vec<String>) -> Result<(), Box<dyn std::error::Error>> {
        if packages.is_empty() {
            println!("Updating all packages");
        } else {
            println!("Updating packages: {:?}", packages);
        }
        // Implementation would check for updates and rebuild packages
        Ok(())
    }

    async fn search_packages(&self, query: String) -> Result<(), Box<dyn std::error::Error>> {
        println!("Searching for: {}", query);
        // Implementation would search package registry
        println!("Search functionality not yet implemented");
        Ok(())
    }

    async fn show_package(&self, package: String) -> Result<(), Box<dyn std::error::Error>> {
        println!("Package information for: {}", package);
        // Implementation would show detailed package information
        Ok(())
    }

    async fn list_packages(&self, _user: bool) -> Result<(), Box<dyn std::error::Error>> {
        let store = self.store.lock().await;
        let stats = store.get_stats();
        
        println!("Installed packages: {}", stats.total_packages);
        println!("Total size: {} bytes", stats.total_size);
        println!("Store path: {}", stats.store_path.display());
        
        Ok(())
    }

    async fn garbage_collect(&mut self, dry_run: bool) -> Result<(), Box<dyn std::error::Error>> {
        println!("Garbage collecting unused packages...");
        
        let mut store = self.store.lock().await;
        let removed = store.garbage_collect()?;
        
        if dry_run {
            println!("Would remove {} packages:", removed.len());
            for package in removed {
                println!("  - {}", package);
            }
        } else {
            println!("Removed {} packages:", removed.len());
            for package in removed {
                println!("  - {}", package);
            }
        }
        
        Ok(())
    }

    async fn show_store_info(&self) -> Result<(), Box<dyn std::error::Error>> {
        let store = self.store.lock().await;
        let stats = store.get_stats();
        
        println!("Store Information:");
        println!("  Path: {}", stats.store_path.display());
        println!("  Total packages: {}", stats.total_packages);
        println!("  Total size: {:.2} MB", stats.total_size as f64 / 1024.0 / 1024.0);
        
        Ok(())
    }

    async fn build_package(&mut self, spec_file: PathBuf) -> Result<(), Box<dyn std::error::Error>> {
        println!("Building package from: {}", spec_file.display());
        
        let content = std::fs::read_to_string(spec_file)?;
        let spec: PackageSpec = toml::from_str(&content)?;
        
        let result = self.builder.build_package(&spec).await;
        match result {
            Ok(build_result) => {
                if build_result.success {
                    println!("✅ Successfully built {}", spec.name);
                    println!("Store path: {}", build_result.store_path.path.display());
                } else {
                    println!("❌ Failed to build {}", spec.name);
                }
                println!("Build log:\n{}", build_result.build_log);
            }
            Err(e) => {
                println!("❌ Error building package: {}", e);
            }
        }
        
        Ok(())
    }

    async fn init_config(&self, config_path: PathBuf) -> Result<(), Box<dyn std::error::Error>> {
        if config_path.exists() {
            println!("Configuration file already exists: {}", config_path.display());
            return Ok(());
        }
        
        let default_config = PyroConfig::default();
        default_config.to_file(&config_path)?;
        
        println!("Initialized configuration file: {}", config_path.display());
        Ok(())
    }

    async fn show_dependency_graph(&self, package: String, format: String) -> Result<(), Box<dyn std::error::Error>> {
        println!("Dependency graph for {} (format: {})", package, format);
        // Implementation would generate and display dependency graph
        println!("Dependency graph functionality not yet implemented");
        Ok(())
    }

    fn parse_package_spec(&self, package_str: &str) -> Result<PackageSpec, Box<dyn std::error::Error>> {
        // Simple parsing: name@version or just name
        let parts: Vec<&str> = package_str.split('@').collect();
        let name = parts[0].to_string();
        let version = if parts.len() > 1 {
            Some(parts[1].to_string())
        } else {
            Some("latest".to_string())
        };
        
        Ok(PackageSpec {
            name: name.clone(),
            version: version.clone(),
            source: PackageSource::Crates {
                name,
                version: version.unwrap_or_else(|| "latest".to_string()),
            },
            build_inputs: vec![],
            runtime_inputs: vec![],
            environment: HashMap::new(),
            build_script: None,
        })
    }
}