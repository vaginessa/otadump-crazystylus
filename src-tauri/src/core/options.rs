use std::path::PathBuf;

#[derive(Debug)]
pub struct Options {
    /// OTA file path, either a .zip file or a payload.bin.
    pub payload_file: PathBuf,

    /// Output directory for the extracted files.
    pub output_dir: PathBuf,
}
