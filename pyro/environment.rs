//! Environment management for Pyro package manager
//! Handles PATH setup and environment variables for installed packages

use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct EnvironmentManager {
	store_path: PathBuf,
}

impl EnvironmentManager {
	pub fn new(store_path: PathBuf) -> Self {
		Self { store_path }
	}

	/// Get the PATH entries that should be added to the user's environment
	pub fn get_path_entries(&self) -> Vec<PathBuf> {
		vec![self.store_path.join("bin"), self.store_path.join("lib")]
	}

	/// Generate shell script to set up environment
	pub fn generate_shell_setup(
		&self,
		shell: &str,
	) -> Result<String, Box<dyn std::error::Error>> {
		let bin_path = self.store_path.join("bin");
		let lib_path = self.store_path.join("lib");

		match shell {
			"bash" | "zsh" => Ok(format!(
				r#"# Pyro package manager environment setup
export PATH="{}:$PATH"
export LD_LIBRARY_PATH="{}:$LD_LIBRARY_PATH"

# Add Pyro completion (if available)
if [ -f "{}/share/completions/pyro.bash" ]; then
    source "{}/share/completions/pyro.bash"
fi
"#,
				bin_path.display(),
				lib_path.display(),
				self.store_path.display(),
				self.store_path.display()
			)),
			"fish" => Ok(format!(
				r#"# Pyro package manager environment setup
set -gx PATH "{}" $PATH
set -gx LD_LIBRARY_PATH "{}" $LD_LIBRARY_PATH

# Add Pyro completion (if available)
if test -f "{}/share/completions/pyro.fish"
    source "{}/share/completions/pyro.fish"
end
"#,
				bin_path.display(),
				lib_path.display(),
				self.store_path.display(),
				self.store_path.display()
			)),
			"powershell" | "pwsh" => Ok(format!(
				r#"# Pyro package manager environment setup
$env:PATH = "{};" + $env:PATH

# Add Pyro completion (if available)
if (Test-Path "{}/share/completions/pyro.ps1") {{
    . "{}/share/completions/pyro.ps1"
}}
"#,
				bin_path.display(),
				self.store_path.display(),
				self.store_path.display()
			)),
			"cmd" => Ok(format!(
				r#"@echo off
REM Pyro package manager environment setup
set "PATH={};%PATH%"
"#,
				bin_path.display()
			)),
			_ => Err(format!("Unsupported shell: {shell}").into()),
		}
	}

	/// Install shell integration
	pub async fn install_shell_integration(
		&self,
		shell: &str,
	) -> Result<(), Box<dyn std::error::Error>> {
		let setup_script = self.generate_shell_setup(shell)?;

		let home_dir =
			dirs::home_dir().ok_or("Could not determine home directory")?;

		let config_file = match shell {
			"bash" => home_dir.join(".bashrc"),
			"zsh" => home_dir.join(".zshrc"),
			"fish" => home_dir.join(".config/fish/config.fish"),
			"powershell" | "pwsh" => {
				// PowerShell profile path
				let profile_dir = home_dir.join("Documents/PowerShell");
				std::fs::create_dir_all(&profile_dir)?;
				profile_dir.join("Microsoft.PowerShell_profile.ps1")
			}
			_ => return Err(format!("Unsupported shell: {shell}").into()),
		};

		// Create config directory if it doesn't exist
		if let Some(parent) = config_file.parent() {
			std::fs::create_dir_all(parent)?;
		}

		// Check if Pyro setup is already in the config file
		let existing_content = if config_file.exists() {
			std::fs::read_to_string(&config_file)?
		} else {
			String::new()
		};

		if !existing_content
			.contains("# Pyro package manager environment setup")
		{
			// Append Pyro setup to the config file
			let mut content = existing_content;
			if !content.is_empty() && !content.ends_with('\n') {
				content.push('\n');
			}
			content.push('\n');
			content.push_str(&setup_script);

			std::fs::write(&config_file, content)?;
			println!("✅ Shell integration installed for {shell}");
			println!("   Added to: {}", config_file.display());
			println!(
				"   Please restart your shell or run: source {}",
				config_file.display()
			);
		} else {
			println!("ℹ️  Shell integration already installed for {shell}");
		}

		Ok(())
	}

	/// Remove shell integration (async)
	pub async fn remove_shell_integration_async(
		&self,
		shell: &str,
	) -> Result<(), Box<dyn std::error::Error>> {
		let home_dir =
			dirs::home_dir().ok_or("Could not determine home directory")?;

		let config_file = match shell {
			"bash" => home_dir.join(".bashrc"),
			"zsh" => home_dir.join(".zshrc"),
			"fish" => home_dir.join(".config/fish/config.fish"),
			"powershell" | "pwsh" => home_dir
				.join("Documents/PowerShell/Microsoft.PowerShell_profile.ps1"),
			_ => return Err(format!("Unsupported shell: {shell}").into()),
		};

		if !config_file.exists() {
			println!("ℹ️  No shell configuration found for {shell}");
			return Ok(());
		}

		let content = std::fs::read_to_string(&config_file)?;
		let lines: Vec<&str> = content.lines().collect();

		let mut new_lines = Vec::new();
		let mut skip_pyro_section = false;

		for line in lines {
			if line.contains("# Pyro package manager environment setup") {
				skip_pyro_section = true;
				continue;
			}

			if skip_pyro_section {
				// Skip lines until we find an empty line or a different comment
				if line.trim().is_empty()
					|| (line.starts_with('#') && !line.contains("Pyro"))
				{
					skip_pyro_section = false;
					if !line.trim().is_empty() {
						new_lines.push(line);
					}
				}
				continue;
			}

			new_lines.push(line);
		}

		let new_content = new_lines.join("\n");
		std::fs::write(&config_file, new_content)?;

		println!("✅ Shell integration removed for {shell}");
		println!("   Please restart your shell to apply changes");

		Ok(())
	}

	/// Get environment variables for a specific package
	pub fn get_package_environment(
		&self,
		package_hash: &str,
	) -> HashMap<String, String> {
		let mut env = HashMap::new();

		let package_path = self.store_path.join(package_hash);
		let bin_path = package_path.join("bin");
		let lib_path = package_path.join("lib");

		// Add package-specific paths
		env.insert(
			"PYRO_PACKAGE_PATH".to_string(),
			package_path.to_string_lossy().to_string(),
		);
		env.insert(
			"PYRO_PACKAGE_BIN".to_string(),
			bin_path.to_string_lossy().to_string(),
		);
		env.insert(
			"PYRO_PACKAGE_LIB".to_string(),
			lib_path.to_string_lossy().to_string(),
		);

		env
	}

	/// Get global environment variables
	pub fn get_environment_variables(&self) -> HashMap<String, String> {
		let mut env = HashMap::new();

		let bin_path = self.store_path.join("bin");
		let lib_path = self.store_path.join("lib");

		env.insert(
			"PYRO_STORE_PATH".to_string(),
			self.store_path.to_string_lossy().to_string(),
		);
		env.insert(
			"PYRO_BIN_PATH".to_string(),
			bin_path.to_string_lossy().to_string(),
		);
		env.insert(
			"PYRO_LIB_PATH".to_string(),
			lib_path.to_string_lossy().to_string(),
		);

		// Add current PATH modification
		if let Ok(current_path) = std::env::var("PATH") {
			let path_separator = if cfg!(windows) { ";" } else { ":" };
			let new_path = format!(
				"{}{}{}",
				bin_path.to_string_lossy(),
				path_separator,
				current_path
			);
			env.insert("PATH".to_string(), new_path);
		}

		env
	}

	/// Detect the current shell
	pub fn detect_shell(&self) -> String {
		Self::detect_shell_static()
	}

	/// Static method to detect shell
	pub fn detect_shell_static() -> String {
		// Try to detect from SHELL environment variable
		if let Ok(shell_path) = std::env::var("SHELL") {
			if let Some(shell_name) =
				std::path::Path::new(&shell_path).file_name()
			{
				return shell_name.to_string_lossy().to_string();
			}
		}

		// On Windows, try to detect PowerShell
		#[cfg(windows)]
		{
			if std::env::var("PSModulePath").is_ok() {
				return "powershell".to_string();
			}
			"cmd".to_string()
		}

		// Default to bash on Unix systems
		#[cfg(unix)]
		{
			return "bash".to_string();
		}
	}

	/// Set up shell integration
	pub fn setup_shell_integration(
		&self,
		shell: &str,
	) -> Result<(), Box<dyn std::error::Error>> {
		let rt = tokio::runtime::Runtime::new()?;
		rt.block_on(self.install_shell_integration(shell))
	}

	/// Remove shell integration
	pub fn remove_shell_integration(
		&self,
		shell: &str,
	) -> Result<(), Box<dyn std::error::Error>> {
		let rt = tokio::runtime::Runtime::new()?;
		rt.block_on(self.remove_shell_integration_async(shell))
	}

	/// Generate shell script
	pub fn generate_shell_script(
		&self,
		shell: &str,
	) -> Result<String, Box<dyn std::error::Error>> {
		self.generate_shell_setup(shell)
	}
}
