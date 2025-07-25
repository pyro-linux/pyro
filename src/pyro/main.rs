use clap::Parser;

use crate::cli::{Cli, PyroApp};

mod builder;
mod cli;
mod config;
mod dependency;
mod request;
mod store;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
	tracing_subscriber::fmt::init();
	let cli = Cli::parse();

	// Run CLI mode
	let mut app = PyroApp::new(&cli).await?;
	app.run(cli.command).await?;

	Ok(())
}
