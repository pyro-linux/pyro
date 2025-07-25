// main.rs

use ratatui::{
	prelude::*,
	widgets::{Block, Borders, Paragraph, Wrap},
};
use std::time::{Duration, Instant};

/// Represents the status of a task in the tree.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NodeStatus {
	Waiting,
	Building,
	Done,
	Failed,
}

impl NodeStatus {
	/// Returns the character symbol for the status.
	fn to_symbol(&self) -> &'static str {
		match self {
			NodeStatus::Waiting => "⌛",
			NodeStatus::Building => "🔨",
			NodeStatus::Done => "✅",
			NodeStatus::Failed => "❌",
		}
	}

	/// Returns the color style for the status.
	fn to_style(&self) -> Style {
		match self {
			NodeStatus::Waiting => Style::default().fg(Color::Yellow),
			NodeStatus::Building => Style::default().fg(Color::Cyan),
			NodeStatus::Done => Style::default().fg(Color::Green),
			NodeStatus::Failed => Style::default().fg(Color::Red),
		}
	}
}

/// Represents a single node in the progress tree.
#[derive(Clone, Debug)]
pub struct Node {
	pub name: Option<String>,
	pub status: NodeStatus,
	pub status_text: String,
	pub children: Vec<Node>,
	pub start_time: Instant,
	pub duration: Duration,
}

impl Node {
	pub fn new(name: Option<&str>) -> Self {
		Node {
			name: name.map(|s| s.to_string()),
			status: NodeStatus::Waiting,
			status_text: "waiting".to_string(),
			children: vec![],
			start_time: Instant::now(),
			duration: Duration::from_secs(0),
		}
	}

	pub fn with_children(mut self, children: Vec<Node>) -> Self {
		self.children = children;
		self
	}
}

/// The main application state.
pub struct ProgressTree {
	pub tree_root: Node,
	pub logs: Vec<String>,
	pub scroll_offset: u16,
}

impl ProgressTree {
	pub fn new(root: Node) -> ProgressTree {
		ProgressTree {
			tree_root: root,
			logs: vec!["Log output will appear here...".to_string()],
			scroll_offset: 0,
		}
	}

	/// Handles the application's state updates on each tick.
	/// This is now a stub; simulation logic is handled in main.rs
	pub fn on_tick(&mut self) {
		// Progress updates are now handled by main.rs
	}
}

/// Renders the user interface.
pub fn tree_ui(f: &mut Frame, app: &ProgressTree) {
	// Create two chunks: one for the tree, one for the logs
	let chunks = Layout::default()
		.direction(Direction::Vertical)
		.constraints([Constraint::Min(1), Constraint::Length(10)].as_ref())
		.split(f.area());

	// Render the tree
	draw_tree(f, chunks[0], app);

	// Render the logs
	draw_logs(f, chunks[1], app);
}

/// Draws the progress tree view. This is our custom widget logic.
fn draw_tree(f: &mut Frame, area: Rect, app: &ProgressTree) {
	let tree_block = Block::default()
		.borders(Borders::ALL)
		.title("Dependency Graph");
	let inner_area = tree_block.inner(area);
	f.render_widget(tree_block, area);

	// --- Custom Tree Rendering ---
	// First, we flatten the tree structure into a list of lines to be rendered.
	// This involves a depth-first traversal.
	let mut lines_to_render = Vec::new();
	flatten_tree_for_render(
		&app.tree_root,
		0,
		&mut vec![],
		&mut lines_to_render,
	);

	// Create a Paragraph from the flattened lines.
	// We can't use a List because we need fine-grained control over the indentation characters.
	let tree_paragraph =
		Paragraph::new(lines_to_render).scroll((app.scroll_offset, 0));

	f.render_widget(tree_paragraph, inner_area);
}

/// Recursively traverses the node tree and flattens it into a Vec of `Line`s for rendering.
fn flatten_tree_for_render<'a>(
	node: &'a Node,
	depth: usize,
	last_stack: &mut Vec<bool>,
	lines: &mut Vec<Line<'a>>,
) {
	// --- 1. Build the prefix string for the current line ---
	// This part draws the `│  `, `├─-`, and `└─-` characters.
	let mut prefix_spans = Vec::new();
	for &is_last in last_stack.iter() {
		let span = if is_last {
			Span::raw("   ") // Parent was the last child, so no vertical line needed
		} else {
			Span::raw("│  ") // Parent was not the last, so draw a vertical line
		};
		prefix_spans.push(span);
	}

	if depth > 0 {
		let connector = if *last_stack.last().unwrap_or(&false) {
			"└─ "
		} else {
			"├─ "
		};
		prefix_spans.push(Span::raw(connector));
	}

	// --- 2. Build the main content of the line ---
	let mut content_spans = vec![Span::styled(
		node.status.to_symbol(),
		node.status.to_style(),
	)];

	if let Some(name) = &node.name {
		content_spans
			.push(Span::styled(name, Style::default().fg(Color::White)));
	}

	content_spans.push(Span::raw(" "));

	content_spans.push(Span::styled(
		format!("🕛 {:.1}s", node.duration.as_secs_f32()),
		Style::default().fg(Color::Gray),
	));

	// --- 3. Combine prefix and content into a single Line ---
	prefix_spans.extend(content_spans);
	lines.push(Line::from(prefix_spans));

	// --- 4. Recurse into children ---
	let children_count = node.children.len();
	for (i, child) in node.children.iter().enumerate() {
		let is_last_child = i == children_count - 1;
		last_stack.push(is_last_child);
		flatten_tree_for_render(child, depth + 1, last_stack, lines);
		last_stack.pop();
	}
}

/// Draws the log output panel.
fn draw_logs(f: &mut Frame, area: Rect, app: &ProgressTree) {
	let log_lines: Vec<Line> =
		app.logs.iter().map(|msg| Line::from(msg.clone())).collect();
	let log_paragraph = Paragraph::new(log_lines)
		.block(Block::default().borders(Borders::ALL).title("Logs"))
		.wrap(Wrap { trim: true });
	f.render_widget(log_paragraph, area);
}
