use core::fmt;

#[derive(Clone, Debug)]
pub struct Package {
	pub name: String,
	pub version: String,
	pub dependencies: Vec<String>,
}

impl fmt::Display for Package {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(f, "{} v{}", self.name, self.version)
	}
}
