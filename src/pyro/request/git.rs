use std::{path::Path, str::FromStr as _};

use gix::bstr::BStr;

use super::FetchError;

pub struct GixFetcher;

impl GixFetcher {
	pub fn new() -> Self {
		Self
	}
}

#[async_trait::async_trait]
impl super::Fetcher for GixFetcher {
	#[tracing::instrument(skip(self))]
	async fn fetch_and_extract(
		&self,
		url: &str,
		destination: &Path,
		revision: Option<&str>,
	) -> Result<(), FetchError> {
		std::fs::create_dir_all(destination)?;
		let url = gix::url::parse(BStr::new(url))?;
		let mut prepare_clone = gix::prepare_clone(url, destination)?;
		let (mut prepare_checkout, _) = prepare_clone.fetch_then_checkout(
			gix::progress::Discard,
			&gix::interrupt::IS_INTERRUPTED,
		)?;

		let (repository, _) = prepare_checkout.main_worktree(
			gix::progress::Discard,
			&gix::interrupt::IS_INTERRUPTED,
		)?;

		if let Some(revision) = revision {
			tracing::debug!("Checking out revision: {}", revision);
			let commit_id = gix::hash::ObjectId::from_str(revision)?;

			let index_state = gix_index::State::from_tree(
				&commit_id,
				&repository.objects,
				gix_validate::path::component::Options::default(),
			)?;
			let mut index = gix_index::File::from_state(
				index_state,
				repository.index_path(),
			);

			let mut opts = repository.checkout_options(
				gix_worktree::stack::state::attributes::Source::IdMapping,
			)?;
			opts.destination_is_initially_empty = false;

			let mut file_progress = gix::progress::Discard;
			let mut byte_progress = gix::progress::Discard;
			let should_interrupt = std::sync::atomic::AtomicBool::new(false);

			let workdir = repository.workdir().expect("Not a bare repo");
			gix_worktree_state::checkout(
				&mut index,
				workdir,
				repository.objects.clone().into_arc()?,
				&mut file_progress,
				&mut byte_progress,
				&should_interrupt,
				opts,
			)?;
		}

		Ok(())
	}
}
