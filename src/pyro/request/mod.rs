use std::{io, path::Path};

use thiserror::Error;

pub mod git;
pub mod request;

#[derive(Error, Debug)]
pub enum FetchError {
	#[error("Failed to download: {0}")]
	Reqwest(#[from] reqwest::Error),
	#[error("HTTP Request failed: {0}")]
	HttpRequestFailed(String),
	#[error("Failed to decompress: {0}")]
	Decompress(#[from] io::Error),
	#[error("Task failed: {0}")]
	TaskJoin(#[from] tokio::task::JoinError),
	#[error("Failed to parse gix URL: {0}")]
	GitParseUrl(#[from] gix::url::parse::Error),
	#[error("Couldn't clone repository: {0}")]
	GitFetch(#[from] gix::clone::fetch::Error),
	#[error("Couldn't clone repository: {0}")]
	CloneError(#[from] gix::clone::Error),
	#[error("Failed to checkout worktree: {0}")]
	CheckoutError(#[from] gix::clone::checkout::main_worktree::Error),
	#[error("Failed to checkout revision: {0}")]
	GitDecodeHash(#[from] gix_hash::decode::Error),
	#[error("Failed to find object: {0}")]
	GitFindObject(#[from] gix::object::find::existing::with_conversion::Error),
	#[error("Failed to find commit: {0}")]
	GitFindCommit(#[from] gix::object::commit::Error),
	#[error("Failed to initialize index from tree: {0}")]
	GitIndexInitFromTree(#[from] gix_index::init::from_tree::Error),
	#[error("Failed to create validate checkout options: {0}")]
	GitValidateCheckoutOptions(#[from] gix::config::checkout_options::Error),
	#[error("Failed to checkout: {0}")]
	GitCheckout(#[from] gix_worktree_state::checkout::Error),
}

#[async_trait::async_trait]
pub trait Fetcher {
	/// Fetch the content from the given URL and extract it to the specified destination.
	async fn fetch_and_extract(
		&self,
		url: &str,
		destination: &Path,
		revision: Option<&str>,
	) -> Result<(), FetchError>;
}
