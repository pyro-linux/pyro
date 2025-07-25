use flate2::read::GzDecoder;
use futures_util::StreamExt;
use gix::revision;
use reqwest::Client;
use std::{
	io::{self, Read},
	path::Path,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt, DuplexStream};

use super::FetchError;

pub struct ReqwestFetcher {
	client: Client,
}

impl ReqwestFetcher {
	pub fn new(client: Client) -> Self {
		ReqwestFetcher { client }
	}
}

#[async_trait::async_trait]
impl super::Fetcher for ReqwestFetcher {
	/// Fetch the content from the given URL and extract it to the specified destination.
	#[tracing::instrument(skip(self))]
	async fn fetch_and_extract(
		&self,
		url: &str,
		destination: &Path,
		_revision: Option<&str>,
	) -> Result<(), FetchError> {
		let response = self.client.get(url).send().await?;

		if !response.status().is_success() {
			return Err(FetchError::HttpRequestFailed(format!(
				"Failed to download: {}",
				response.status()
			)));
		}

		let (mut writer, reader) = tokio::io::duplex(4096);

		let writer_handle = tokio::spawn(async move {
			let mut stream = response.bytes_stream();
			while let Some(chunk_result) = stream.next().await {
				match chunk_result {
					Ok(chunk) => {
						if let Err(e) = writer.write_all(&chunk).await {
							tracing::error!(
								"Error writing to duplex stream: {}",
								e
							);
							break;
						}
					}
					Err(e) => {
						tracing::error!(
							"Error reading from response stream: {}",
							e
						);
						break;
					}
				}
			}
		});

		let destination = destination.to_owned();
		let decompress_handle = tokio::task::spawn_blocking(move || {
			let sync_reader = DuplexStreamSyncReader::new(reader);
			let mut decoder = GzDecoder::new(sync_reader);
			let mut decompressed_data = Vec::new();

			match decoder.read_to_end(&mut decompressed_data) {
				Ok(_) => {
					// Write the decompressed data to the destination file
					std::fs::write(&destination, decompressed_data)
						.map_err(|e| io::Error::new(io::ErrorKind::Other, e))
				}
				Err(e) => Err(io::Error::new(io::ErrorKind::Other, e)),
			}
		});

		// Wait for both tasks to complete.
		let (writer_result, decompress_result) =
			tokio::join!(writer_handle, decompress_handle);

		writer_result?; // Check for panics in the writer task
		decompress_result??; // Check for errors/panics in the decompressor task

		Ok(())
	}
}

struct DuplexStreamSyncReader {
	async_reader: DuplexStream,
	buffer: Vec<u8>,
	buffer_pos: usize,
	buffer_len: usize,
}

impl DuplexStreamSyncReader {
	fn new(async_reader: DuplexStream) -> Self {
		DuplexStreamSyncReader {
			async_reader,
			buffer: vec![0; 4096],
			buffer_pos: 0,
			buffer_len: 0,
		}
	}
}

impl Read for DuplexStreamSyncReader {
	fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
		if self.buffer_pos < self.buffer_len {
			let bytes_to_copy =
				(self.buffer_len - self.buffer_pos).min(buf.len());
			buf[..bytes_to_copy].copy_from_slice(
				&self.buffer[self.buffer_pos..self.buffer_pos + bytes_to_copy],
			);
			self.buffer_pos += bytes_to_copy;
			return Ok(bytes_to_copy);
		}

		let rt_handle = tokio::runtime::Handle::current();
		let bytes_read = rt_handle.block_on(async {
			self.async_reader.read(&mut self.buffer).await
		})?;

		self.buffer_pos = 0;
		self.buffer_len = bytes_read;

		if bytes_read == 0 {
			return Ok(0);
		}

		let bytes_to_copy = self.buffer_len.min(buf.len());
		buf[..bytes_to_copy].copy_from_slice(&self.buffer[..bytes_to_copy]);
		self.buffer_pos += bytes_to_copy;
		Ok(bytes_to_copy)
	}
}
