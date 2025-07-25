use crate::builder::PyroBuilder;
use crate::config::PyroConfig;
use crate::store::PyroStore;
use clap::{Parser, Subcommand};
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
	#[arg(short, long, default_value = "pyro.luau")]
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
	Build {
		/// Package specification file
		flake_path: PathBuf,
	},
	/// Show dependency graph
	Graph {
		/// Package name
		package: String,
		/// Output format (dot, json)
		#[arg(short, long, default_value = "dot")]
		format: String,
	},
	/// Environment management
	Shell {
		package: String,
		shell: Option<String>,
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
		let store = Arc::new(tokio::sync::Mutex::new(
			PyroStore::new(store_config.clone()).await?,
		));

		// Initialize builder
		let builder =
			PyroBuilder::new(config.build_config.clone(), store.clone());

		Ok(PyroApp {
			config,
			store,
			builder,
		})
	}

	pub async fn run(
		&mut self,
		command: Commands,
	) -> Result<(), Box<dyn std::error::Error>> {
		match command {
			// Commands::List { user } => self.list_packages(user).await,
			Commands::Gc { dry_run } => self.garbage_collect(dry_run).await,
			Commands::StoreInfo => self.show_store_info().await,
			Commands::Build { flake_path } => {
				// self.build_package(flake_path).await
				Ok(())
			}
			Commands::Graph { package, format } => {
				// self.show_dependency_graph(package, format).await
				Ok(())
			}
			Commands::List { user } => {
				let mut store = self.store.lock().await;
				// let packages = store.list_packages(user).await?;
				// if packages.is_empty() {
				// 	println!("No packages found.");
				// } else {
				// 	for package in packages {
				// 		println!("{}", package);
				// 	}
				// }
				Ok(())
			}
			Commands::Shell { package, shell } => {
				// self.env_manager
				// 	.handle_shell_command(package_spec, shell)
				// 	.await
				Ok(())
			}
		}
	}

	async fn garbage_collect(
		&mut self,
		dry_run: bool,
	) -> Result<(), Box<dyn std::error::Error>> {
		println!("Garbage collecting unused packages...");

		let mut store = self.store.lock().await;
		let removed = store.garbage_collect(dry_run).await?;

		if dry_run {
			println!("Would remove {} packages:", removed.len());
		} else {
			println!("Removed {} packages:", removed.len());
		}

		for package in removed {
			println!("  - {package}");
		}

		Ok(())
	}

	async fn show_store_info(&self) -> Result<(), Box<dyn std::error::Error>> {
		let store = self.store.lock().await;
		let stats = store.get_stats();

		println!("Store Information:");
		println!("  Path: {}", stats.store_path.display());
		println!("  Total packages: {}", stats.total_packages);
		println!(
			"  Total size: {:.2} MB",
			stats.total_size as f64 / 1024.0 / 1024.0
		);

		Ok(())
	}
}
