use std::cmp::Reverse;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read};
use std::ops::{Div as _, Mul as _};
use std::path::{Path, PathBuf};
use std::slice;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use anyhow::{bail, ensure, Context as _, Result};
use bzip2::read::BzDecoder;
use lzma::LzmaReader;
use memmap2::{Mmap, MmapMut};
use prost::Message as _;
use rayon::ThreadPoolBuilder;
use sha2::{Digest as _, Sha256};
use sync_unsafe_cell::SyncUnsafeCell;
use zip::result::ZipError;
use zip::ZipArchive;

use crate::core::chromeos_update_engine::install_operation::Type;
use crate::core::chromeos_update_engine::{
    DeltaArchiveManifest, InstallOperation, PartitionUpdate,
};
use crate::core::payload::Payload;
use crate::core::reporter::Reporter;

#[derive(Debug)]
pub struct ExtractOptions {
    /// OTA file path, either a .zip file or a payload.bin.
    pub payload_file: PathBuf,

    /// Output directory for the extracted files.
    pub output_dir: PathBuf,
}

impl ExtractOptions {
    pub fn extract(self, reporter: Box<dyn Reporter>) {
        let reporter = reporter.into();
        if let Err(e) = self.extract_helper(Arc::clone(&reporter)) {
            reporter.report_error(e.into());
        }
    }

    fn extract_helper(&self, reporter: Arc<dyn Reporter>) -> Result<()> {
        let payload_file = Self::open_payload_file(&self.payload_file)?;
        let payload = &Payload::parse(&payload_file)?;

        let mut manifest =
            DeltaArchiveManifest::decode(payload.manifest).context("unable to parse manifest")?;
        let block_size = manifest.block_size.context("block_size not defined")? as usize;

        // Verification is slow for large partitions, and cannot be parallelized.
        // Extracting the largest partition first allows us to start verifying
        // it as early as possible.
        manifest.partitions.sort_unstable_by_key(|partition| {
            Reverse(partition.new_partition_info.as_ref().and_then(|info| info.size).unwrap_or(0))
        });

        let output_dir = &self.output_dir;
        fs::create_dir_all(output_dir)
            .with_context(|| format!("could not create output directory: {output_dir:?}"))?;

        let threadpool = ThreadPoolBuilder::new().build().context("unable to start threadpool")?;

        threadpool.in_place_scope_fifo(|scope| {
            let total_ops =
                manifest.partitions.iter().map(|update| update.operations.len()).sum::<usize>();
            let total_ops_completed = Arc::new(AtomicUsize::new(0));

            for update in &manifest.partitions {
                let partition_file = Self::open_partition_file(update, output_dir)?;
                let partition_len = partition_file.len();
                let partition_file = Arc::new(SyncUnsafeCell::new(partition_file));

                let partition_ops = update.operations.len();
                let partition_ops_completed = Arc::new(AtomicUsize::new(0));

                for op in update.operations.iter() {
                    let partition_file = Arc::clone(&partition_file);
                    let total_ops_completed = Arc::clone(&total_ops_completed);
                    let partition_ops_completed = Arc::clone(&partition_ops_completed);
                    let reporter = Arc::clone(&reporter);

                    scope.spawn_fifo(move |_| {
                        let partition = unsafe { (*partition_file.get()).as_mut_ptr() };
                        if let Err(e) =
                            Self::run_op(op, payload, partition, partition_len, block_size)
                                .context("error running operation")
                        {
                            reporter.report_error(e.into());
                            return;
                        }

                        // If this is the last operation of the partition, verify the output.
                        let partition_ops_completed =
                            partition_ops_completed.fetch_add(1, Ordering::AcqRel) + 1;
                        if partition_ops_completed == partition_ops {
                            update
                                .new_partition_info
                                .as_ref()
                                .and_then(|info| info.hash.as_ref())
                                .inspect(|hash| {
                                    let partition = unsafe { (*partition_file.get()).as_ref() };
                                    if let Err(e) = Self::verify_sha256(partition, hash)
                                        .context("output verification failed")
                                    {
                                        reporter.report_error(e.into());
                                    }
                                });
                        }

                        // Report overall progress of the extraction.
                        let total_ops_completed =
                            total_ops_completed.fetch_add(1, Ordering::AcqRel) + 1;
                        if total_ops_completed == total_ops {
                            reporter.report_complete();
                        } else {
                            let progress = total_ops_completed as f64 / total_ops as f64;
                            reporter.report_progress(progress);
                        }
                    });
                }
            }
            Ok(())
        })
    }

    fn run_op(
        op: &InstallOperation,
        payload: &Payload,
        partition: *mut u8,
        partition_len: usize,
        block_size: usize,
    ) -> Result<()> {
        let mut dst_extents = Self::extract_dst_extents(op, partition, partition_len, block_size)
            .context("error extracting dst_extents")?;

        match Type::from_i32(op.r#type) {
            Some(Type::Replace) => {
                let mut data = Self::extract_data(op, payload).context("error extracting data")?;
                Self::run_op_replace(&mut data, &mut dst_extents, block_size)
                    .context("error in REPLACE operation")
            }
            Some(Type::ReplaceBz) => {
                let data = Self::extract_data(op, payload).context("error extracting data")?;
                let mut decoder = BzDecoder::new(data);
                Self::run_op_replace(&mut decoder, &mut dst_extents, block_size)
                    .context("error in REPLACE_BZ operation")
            }
            Some(Type::ReplaceXz) => {
                let data = Self::extract_data(op, payload).context("error extracting data")?;
                let mut decoder = LzmaReader::new_decompressor(data)
                    .context("unable to initialize lzma decoder")?;
                Self::run_op_replace(&mut decoder, &mut dst_extents, block_size)
                    .context("error in REPLACE_XZ operation")
            }
            Some(Type::Zero) => Ok(()), // This is a no-op since the partition is already zeroed
            Some(op) => bail!("unimplemented operation: {op:?}"),
            None => bail!("invalid operation"),
        }
    }

    fn run_op_replace(
        reader: &mut impl Read,
        dst_extents: &mut [&mut [u8]],
        block_size: usize,
    ) -> Result<()> {
        let mut bytes_read = 0usize;

        let dst_len = dst_extents.iter().map(|extent| extent.len()).sum::<usize>();
        for extent in dst_extents.iter_mut() {
            bytes_read += io::copy(reader, extent).context("failed to write to buffer")? as usize;
        }
        ensure!(reader.bytes().next().is_none(), "read fewer bytes than expected");

        // Align number of bytes read to block size. The formula for alignment is:
        // ((operand + alignment - 1) / alignment) * alignment
        let bytes_read_aligned = (bytes_read + block_size - 1).div(block_size).mul(block_size);
        ensure!(bytes_read_aligned == dst_len, "more dst blocks than data, even with padding");

        Ok(())
    }

    fn extract_data<'a>(op: &InstallOperation, payload: &'a Payload) -> Result<&'a [u8]> {
        let data_len = op.data_length.context("data_length not defined")? as usize;
        let data = {
            let offset = op.data_offset.context("data_offset not defined")? as usize;
            payload
                .data
                .get(offset..offset + data_len)
                .context("data offset exceeds payload size")?
        };
        if let Some(hash) = &op.data_sha256_hash {
            Self::verify_sha256(data, hash).context("input verification failed")?;
        }
        Ok(data)
    }

    fn extract_dst_extents(
        op: &InstallOperation,
        partition: *mut u8,
        partition_len: usize,
        block_size: usize,
    ) -> Result<Vec<&'static mut [u8]>> {
        op.dst_extents
            .iter()
            .map(|extent| {
                let start_block =
                    extent.start_block.context("start_block not defined in extent")? as usize;
                let num_blocks =
                    extent.num_blocks.context("num_blocks not defined in extent")? as usize;

                let partition_offset = start_block * block_size;
                let extent_len = num_blocks * block_size;

                ensure!(
                    partition_offset + extent_len <= partition_len,
                    "extent exceeds partition size"
                );
                let extent = unsafe {
                    slice::from_raw_parts_mut(partition.add(partition_offset), extent_len)
                };

                Ok(extent)
            })
            .collect()
    }

    fn open_payload_file(path: impl AsRef<Path>) -> Result<Mmap> {
        let path = path.as_ref();
        let file = File::open(path)
            .with_context(|| format!("unable to open file for reading: {path:?}"))?;

        // Assume the file is a zip archive. If it's not, we get an
        // InvalidArchive error, and we can treat it as a payload.bin file.
        match ZipArchive::new(&file) {
            Ok(mut archive) => {
                // TODO: add progress indicator while zip file is being
                // extracted.
                let mut zipfile = archive
                    .by_name("payload.bin")
                    .context("could not find payload.bin file in archive")?;

                let mut file = tempfile::tempfile().context("failed to create temporary file")?;
                let _ = file.set_len(zipfile.size());
                io::copy(&mut zipfile, &mut file).context("failed to write to temporary file")?;

                unsafe { Mmap::map(&file) }.context("failed to mmap temporary file")
            }
            Err(ZipError::InvalidArchive(_)) => unsafe { Mmap::map(&file) }
                .with_context(|| format!("failed to mmap file: {path:?}")),
            Err(e) => Err(e).context("failed to open zip archive"),
        }
    }

    fn open_partition_file(
        update: &PartitionUpdate,
        partition_dir: impl AsRef<Path>,
    ) -> Result<MmapMut> {
        let partition_len = update
            .new_partition_info
            .as_ref()
            .and_then(|info| info.size)
            .context("unable to determine output file size")?;

        let filename = Path::new(&update.partition_name).with_extension("img");
        let path = &partition_dir.as_ref().join(filename);

        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(path)
            .with_context(|| format!("unable to open file for writing: {path:?}"))?;
        file.set_len(partition_len)?;

        unsafe { MmapMut::map_mut(&file) }.with_context(|| format!("failed to mmap file: {path:?}"))
    }

    fn verify_sha256(data: &[u8], exp_hash: &[u8]) -> Result<()> {
        let got_hash = Sha256::digest(data);
        ensure!(
            got_hash.as_slice() == exp_hash,
            "hash mismatch: expected {}, got {got_hash:x}",
            hex::encode(exp_hash)
        );
        Ok(())
    }
}
