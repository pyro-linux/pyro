#![allow(dead_code)]
use clap::Parser;
use std::fs::create_dir_all;
use std::io;
use std::time::Instant;

use crate::cli::{Cli, Commands, PyroApp};
use crate::dependency::Package;
use crate::ui::{Node, NodeStatus, ProgressTree, tree_ui};
use petgraph::graph::DiGraph;
use petgraph::visit::Topo;
use ratatui::crossterm::event::{DisableMouseCapture, EnableMouseCapture};
use ratatui::crossterm::execute;
use ratatui::crossterm::terminal::{
	Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen,
	enable_raw_mode,
};
use ratatui::prelude::CrosstermBackend;
use ratatui::{Frame, Terminal};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::fs::create_dir_all;
use std::path::Path;

mod builder;
mod cli;
mod config;
mod dependency;
mod store;
mod ui;

#[derive(Debug)]
enum ProgressMsg {
	Status(usize, io::Result<()>),
	Log(String),
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
	let cli = Cli::parse();
	
	// Check if we should run in TUI mode (no command specified)
	if matches!(cli.command, Commands::Install { .. } | Commands::Remove { .. } | 
				 Commands::Update { .. } | Commands::Search { .. } | Commands::Show { .. } |
				 Commands::List { .. } | Commands::Gc { .. } | Commands::StoreInfo |
				 Commands::Build { .. } | Commands::Init { .. } | Commands::Graph { .. }) {
		// Run CLI mode
		let mut app = PyroApp::new(&cli).await?;
		app.run(cli.command).await?;
		return Ok(());
	}
	
	// Fallback to TUI mode for demonstration
	run_tui_mode().await
}

/// Run the original TUI mode for package visualization
async fn run_tui_mode() -> Result<(), Box<dyn std::error::Error>> {
	let packages = vec![
		Package {
			name: "flate2".to_string(),
			version: "1.1.2".to_string(),
			dependencies: vec![],
		},
		Package {
			name: "crc32fast".to_string(),
			version: "1.5.0".to_string(),
			dependencies: vec!["cfg-if".to_string()],
		},
		Package {
			name: "cfg-if".to_string(),
			version: "1.0.1".to_string(),
			dependencies: vec![],
		},
		Package {
			name: "libz-rs-sys".to_string(),
			version: "0.5.1".to_string(),
			dependencies: vec!["zlib-rs".to_string()],
		},
		Package {
			name: "zlib-rs".to_string(),
			version: "0.5.1".to_string(),
			dependencies: vec![],
		},
		Package {
			name: "ruzstd".to_string(),
			version: "0.8.1".to_string(),
			dependencies: vec![],
		},
		Package {
			name: "tar".to_string(),
			version: "0.4.44".to_string(),
			dependencies: vec![
				"filetime".to_string(),
				"libc".to_string(),
				"xattr".to_string(),
			],
		},
		Package {
			name: "filetime".to_string(),
			version: "0.2.25".to_string(),
			dependencies: vec!["cfg-if".to_string(), "libc".to_string()],
		},
		Package {
			name: "libc".to_string(),
			version: "0.2.174".to_string(),
			dependencies: vec![],
		},
		Package {
			name: "xattr".to_string(),
			version: "1.5.1".to_string(),
			dependencies: vec!["rustix".to_string()],
		},
		Package {
			name: "rustix".to_string(),
			version: "1.0.8".to_string(),
			dependencies: vec![
				"bitflags".to_string(),
				"linux-raw-sys".to_string(),
			],
		},
		Package {
			name: "bitflags".to_string(),
			version: "2.9.1".to_string(),
			dependencies: vec![],
		},
		Package {
			name: "linux-raw-sys".to_string(),
			version: "0.9.4".to_string(),
			dependencies: vec![],
		},
	];

	let target_dir = std::path::absolute(Path::new("target/crates"))?;
	create_dir_all(&target_dir)?;

	enable_raw_mode()?;
	let mut stdout = io::stdout();
	execute!(
		stdout,
		EnterAlternateScreen,
		EnableMouseCapture,
		Clear(ClearType::All)
	)?;
	let backend = CrosstermBackend::new(stdout);
	let mut terminal = Terminal::new(backend)?;

	// Build dependency graph and resolve topological order
	let dep_graph = build_dependency_graph(&packages);
	let mut topo = Topo::new(&dep_graph);
	let mut ordered_indices = Vec::new();
	while let Some(node) = topo.next(&dep_graph) {
		ordered_indices.push(node);
	}
	// Map node indices back to package names
	let ordered_names: Vec<String> = ordered_indices
		.iter()
		.map(|&idx| dep_graph[idx].clone())
		.collect();
	// Reorder packages for fetching/extracting
	let mut ordered_packages = Vec::new();
	for name in &ordered_names {
		if let Some(pkg) = packages.iter().find(|p| &p.name == name) {
			ordered_packages.push(pkg.clone());
		}
	}
	// Build clean tree from ordered_packages
	let root_node = Node::new(None).with_children(
		ordered_packages
			.iter()
			.map(|pkg| {
				Node::new(Some(&pkg.name)).with_children(
					pkg.dependencies
						.iter()
						.map(|dep| Node::new(Some(dep)))
						.collect(),
				)
			})
			.collect(),
	);
	let mut app = App {
		jobs: std::thread::available_parallelism().map_or(1, |n| n.get()),
		packages: ordered_packages.clone(),
		progress_bar: ProgressTree::new(root_node),
		should_quit: false,
		last_tick: Instant::now(),
	};

	// Spawn background task for fetching/extracting packages

	let (tx, mut rx) = tokio::sync::mpsc::channel(app.packages.len() * 2);
	let target_dir_clone = target_dir.clone();
	let jobs = app.jobs;
	let ordered_packages_clone = ordered_packages.clone();
	tokio::spawn(async move {
		let semaphore = Arc::new(tokio::sync::Semaphore::new(jobs));
		let mut handles = Vec::new();

		app.progress_bar.tree_root.start_time = Instant::now();

		for (i, pkg) in ordered_packages_clone.iter().enumerate() {
			let sem = semaphore.clone();
			let tx = tx.clone();
			let target_dir = target_dir_clone.clone();
			let pkg = pkg.clone();
			let handle = tokio::spawn(async move {
				let _permit = sem.acquire().await.unwrap();
				let crate_path = target_dir
					.join(format!("{}-{}.crate", pkg.name, pkg.version));
				let url = format!(
					"https://static.crates.io/crates/{}/{}-{}.crate",
					pkg.name, pkg.name, pkg.version
				);
				// Fetch
				if !crate_path.exists() {
					let target_dir_clone = target_dir.clone();
					let url_clone = url.clone();
					let tx_log = tx.clone();
					let fetch_result = tokio::task::spawn_blocking(move || {
						let mut command = Command::new("curl");
						command.stdout(Stdio::piped());
						command.stderr(Stdio::piped());
						command.current_dir(&target_dir_clone);
						command.arg("-LO");
						command.arg(&url_clone);
						match command.spawn() {
							Ok(mut child) => {
								if let Some(mut stderr) = child.stderr.take() {
									let tx_log2 = tx_log.clone();
									std::thread::spawn(move || {
										use std::io::{BufRead, BufReader};
										let reader =
											BufReader::new(&mut stderr);
										for line in
											reader.lines().map_while(Result::ok)
										{
											// Send log line to UI
											let _ = tx_log2.blocking_send(
												ProgressMsg::Log(line),
											);
										}
									});
								}

								if let Some(mut stdout) = child.stdout.take() {
									let tx_log2 = tx_log.clone();
									std::thread::spawn(move || {
										use std::io::{BufRead, BufReader};
										let reader =
											BufReader::new(&mut stdout);
										for line in
											reader.lines().map_while(Result::ok)
										{
											// Send log line to UI
											let _ = tx_log2.blocking_send(
												ProgressMsg::Log(line),
											);
										}
									});
								}

								child.wait()
							}
							Err(e) => Err(e),
						}
					})
					.await;

					let fetch_result = match fetch_result {
						Ok(Ok(status)) => status,
						Ok(Err(e)) => {
							let _ = tx
								.send(ProgressMsg::Status(
									i,
									Err(io::Error::other(format!(
										"Failed to run curl: {e}"
									))),
								))
								.await;
							return;
						}
						Err(e) => {
							let _ = tx
								.send(ProgressMsg::Status(
									i,
									Err(io::Error::other(format!(
										"Join error: {e}"
									))),
								))
								.await;
							return;
						}
					};
					if !fetch_result.success() {
						let _ = tx
							.send(ProgressMsg::Status(
								i,
								Err(io::Error::other(
									"Failed to download crate",
								)),
							))
							.await;
						return;
					}
				}

				// Extract
				let target_dir_clone = target_dir.clone();
				let crate_path_clone = crate_path.clone();
				let extract_result = tokio::task::spawn_blocking(move || {
					Command::new("tar")
						.arg("-xf")
						.arg(&crate_path_clone)
						.current_dir(&target_dir_clone)
						.status()
				})
				.await;
				let extract_result = match extract_result {
					Ok(Ok(status)) => status,
					Ok(Err(e)) => {
						let _ = tx
							.send(ProgressMsg::Status(
								i,
								Err(io::Error::other(format!(
									"Failed to run tar: {e}"
								))),
							))
							.await;
						return;
					}
					Err(e) => {
						let _ = tx
							.send(ProgressMsg::Status(
								i,
								Err(io::Error::other(format!(
									"Join error: {e}"
								))),
							))
							.await;
						return;
					}
				};
				if !extract_result.success() {
					let _ = tx
						.send(ProgressMsg::Status(
							i,
							Err(io::Error::other("Failed to extract crate")),
						))
						.await;
					return;
				}
				let _ = tx.send(ProgressMsg::Status(i, Ok(()))).await;
			});

			handles.push(handle);
		}

		// Wait for all tasks to complete
		for handle in handles {
			if let Err(e) = handle.await {
				let _ = tx
					.send(ProgressMsg::Status(
						0,
						Err(io::Error::other(format!("Task failed: {e}"))),
					))
					.await;
			}
		}

		let _ = tx.send(ProgressMsg::Status(0, Ok(()))).await;
	});

	// Pass terminal to run_app_with_progress
	run_app_with_progress(&mut terminal, &mut app, &mut rx).await?;
	// Cleanup after UI loop
	execute!(
		terminal.backend_mut(),
		LeaveAlternateScreen,
		DisableMouseCapture
	)?;
	terminal.show_cursor()?;

	Ok(())
}

pub struct App {
	packages: Vec<Package>,
	progress_bar: ProgressTree,
	should_quit: bool,
	last_tick: Instant,
	jobs: usize,
}

impl App {
	/// Renders the user interface.
	pub fn ui(&mut self, f: &mut Frame) {
		tree_ui(f, &self.progress_bar);
	}

	/// Handles the application's state updates on each tick.
	pub fn on_tick(&mut self) {
		self.progress_bar.on_tick();
	}

	pub fn new(packages: Vec<Package>) -> Self {
		let root_node = Node::new(None).with_children(
			packages
				.iter()
				.map(|pkg| {
					Node::new(Some(&pkg.name)).with_children(
						pkg.dependencies
							.iter()
							.map(|dep| Node::new(Some(dep)))
							.collect(),
					)
				})
				.collect(),
		);

		App {
			jobs: std::thread::available_parallelism().map_or(1, |n| n.get()),
			packages,
			progress_bar: ProgressTree::new(root_node),
			should_quit: false,
			last_tick: Instant::now(),
		}
	}
}

/// Recursively update all nodes in the tree with the given name
fn update_node_status_recursive(
	node: &mut Node,
	name: &str,
	status: NodeStatus,
	status_text: &str,
) {
	node.duration = Instant::now().duration_since(node.start_time);

	if node.name.is_some() && node.name.as_deref() == Some(name) {
		node.status = status.clone();
		node.status_text = status_text.to_string();
	}

	for child in &mut node.children {
		update_node_status_recursive(child, name, status.clone(), status_text);
	}
}

/// Main application loop with progress updates from background task.
async fn run_app_with_progress(
	terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
	app: &mut App,
	rx: &mut tokio::sync::mpsc::Receiver<ProgressMsg>,
) -> io::Result<()> {
	use ratatui::crossterm::event::{self, Event, KeyCode};
	use std::time::Duration;
	let tick_rate = Duration::from_millis(250);
	app.last_tick = Instant::now();
	loop {
		terminal.draw(|f| app.ui(f))?;
		if event::poll(tick_rate)? {
			if let Event::Key(key) = event::read()? {
				match key.code {
					KeyCode::Char('q') => app.should_quit = true,
					KeyCode::Down => {
						app.progress_bar.scroll_offset =
							app.progress_bar.scroll_offset.saturating_add(1)
					}
					KeyCode::Up => {
						app.progress_bar.scroll_offset =
							app.progress_bar.scroll_offset.saturating_sub(1)
					}
					_ => {}
				}
			}
		}
		if Instant::now().duration_since(app.last_tick) >= tick_rate {
			app.on_tick();
			// Check for progress and log updates from background task
			while let Ok(msg) = rx.try_recv() {
				match msg {
					ProgressMsg::Status(idx, result) => {
						let pkg_name = &app.packages[idx].name;
						match result {
							Ok(()) => {
								update_node_status_recursive(
									&mut app.progress_bar.tree_root,
									pkg_name,
									NodeStatus::Done,
									"done",
								);
							}
							Err(e) => {
								update_node_status_recursive(
									&mut app.progress_bar.tree_root,
									pkg_name,
									NodeStatus::Failed,
									&format!("failed: {e}"),
								);
							}
						}

						// check and update root node status + time
						if app.progress_bar.tree_root.children.iter().all(|n| {
							n.status == NodeStatus::Done
								|| n.status == NodeStatus::Failed
						}) {
							app.progress_bar.tree_root.status =
								NodeStatus::Done;
							app.progress_bar.tree_root.status_text =
								"done".to_string();
							app.progress_bar.tree_root.duration =
								Instant::now().duration_since(
									app.progress_bar.tree_root.start_time,
								);
						}
					}
					ProgressMsg::Log(line) => {
						app.progress_bar.logs.push(line);
						if app.progress_bar.logs.len() > 1000 {
							app.progress_bar
								.logs
								.drain(0..app.progress_bar.logs.len() - 1000);
						}
					}
				}
			}
			// After processing updates, check if all package nodes are Done or Failed
			let all_done =
				app.progress_bar.tree_root.children.iter().all(|n| {
					n.status == NodeStatus::Done
						|| n.status == NodeStatus::Failed
				});
			if all_done && app.progress_bar.tree_root.status != NodeStatus::Done
			{
				app.progress_bar.tree_root.status = NodeStatus::Done;
				app.progress_bar.tree_root.status_text = "done".to_string();
			}
		}
		if app.should_quit {
			return Ok(());
		}
	}
}

fn build_dependency_graph(packages: &[Package]) -> DiGraph<String, ()> {
	let mut graph = DiGraph::<String, ()>::new();
	let mut node_indices = std::collections::HashMap::new();
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
