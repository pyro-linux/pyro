use std::process::Command;

#[derive(Debug, thiserror::Error)]
pub enum FetchError {
	#[error("Git error: {0}")]
	GitError(String),
	#[error("IO error: {0}")]
	IoError(String),
	#[error("Unsupported source type")]
	UnsupportedSource,
}

pub(crate) fn fetch_source(
	config: &crate::config::BuildConfig,
	source: &crate::config::PackageSource,
	build_dir: &std::path::PathBuf,
) -> Result<(), FetchError> {
	match source {
		crate::config::PackageSource::Git { url, rev } => {
			let git_command = config.git_command.as_deref().unwrap_or("git");

			let mut cmd = Command::new(git_command);
			cmd.arg("clone").arg("--depth=1").arg(url).arg(build_dir);
			if !cmd
				.status()
				.map_err(|e| FetchError::GitError(e.to_string()))?
				.success()
			{
				return Err(FetchError::GitError(String::from(
					"Failed to clone git repository",
				)));
			}

			if let Some(revision) = rev {
				let command = Command::new(git_command)
					.arg("checkout")
					.arg(revision)
					.current_dir(build_dir)
					.output()
					.map_err(|e| FetchError::GitError(e.to_string()))?;

				if !command.status.success() {
					return Err(FetchError::GitError(
						String::from_utf8_lossy(&command.stderr).to_string(),
					));
				}
			}
		}
		crate::config::PackageSource::Path { path } => {
			std::fs::copy(path, build_dir)
				.map_err(|e| FetchError::IoError(e.to_string()))?;
		}
		crate::config::PackageSource::Url { url, hash } => {
			let fetch_command =
				config.fetch_command.as_deref().unwrap_or("curl");

			let output = Command::new(fetch_command)
				.arg("-L")
				.arg(url)
				.stdout(std::process::Stdio::piped())
				.spawn()
				.map_err(|e| FetchError::IoError(e.to_string()))?;
			let mut child = output.stdout.ok_or_else(|| {
				FetchError::IoError("Failed to get stdout".to_string())
			})?;
			let mut archive = std::process::Command::new("bsdtar")
				.arg("-x")
				.arg("-C")
				.arg(build_dir)
				.stdin(std::process::Stdio::from(child))
				.spawn()
				.map_err(|e| FetchError::IoError(e.to_string()))?;

			let status = archive
				.wait()
				.map_err(|e| FetchError::IoError(e.to_string()))?;
			if !status.success() {
				return Err(FetchError::IoError(String::from(
					"Failed to extract archive from URL",
				)));
			}
		}
	}

	Ok(())
}
