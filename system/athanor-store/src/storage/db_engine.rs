//! Athanor OS — Asynchronous io_uring Database & Storage Engine (Fase 10)
//!
//! Replaces POSIX standard I/O with high-performance Linux `io_uring` zero-copy asynchronous storage operations.
//! Supports background asynchronous DB snapshot submission and panic-free concurrency.

use crate::storage::io_uring_engine::{IoRequest, IoUringEngine, NVME_SECTOR_ALIGNMENT};
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::alloc::{alloc_zeroed, dealloc, Layout};
use std::fs::{File, OpenOptions};
use std::os::unix::io::AsRawFd;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tracing::info;

/// Aligned memory buffer for NVMe Direct I/O and io_uring alignment requirements (4KB boundaries).
pub struct AlignedBuffer {
    ptr: *mut u8,
    layout: Layout,
    capacity: usize,
}

// SAFETY: AlignedBuffer owns its heap-allocated memory and is safe to send.
unsafe impl Send for AlignedBuffer {}
// SAFETY: The internal pointer is only accessed through safe mutable and immutable slice borrows, ensuring thread safety.
unsafe impl Sync for AlignedBuffer {}

impl AlignedBuffer {
    /// Creates a new aligned memory buffer rounded up to a multiple of `NVME_SECTOR_ALIGNMENT` (4KB).
    pub fn new(size: usize) -> Result<Self> {
        let align = NVME_SECTOR_ALIGNMENT;
        let capacity = if size == 0 {
            align
        } else {
            (size + align - 1) & !(align - 1)
        };
        let layout = Layout::from_size_align(capacity, align)
            .map_err(|e| anyhow::anyhow!("Invalid memory layout alignment: {}", e))?;

        // SAFETY: The layout is guaranteed to have non-zero size and valid alignment.
        let ptr = unsafe { alloc_zeroed(layout) };
        if ptr.is_null() {
            bail!("Out of memory allocating aligned buffer of {} bytes", capacity);
        }

        Ok(Self {
            ptr,
            layout,
            capacity,
        })
    }

    pub fn as_slice(&self) -> &[u8] {
        unsafe { std::slice::from_raw_parts(self.ptr, self.capacity) }
    }

    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        unsafe { std::slice::from_raw_parts_mut(self.ptr, self.capacity) }
    }

    pub fn ptr(&self) -> *mut u8 {
        self.ptr
    }

    pub fn capacity(&self) -> usize {
        self.capacity
    }
}

impl Drop for AlignedBuffer {
    fn drop(&mut self) {
        // SAFETY: The pointer was allocated using alloc_zeroed with the exact same layout.
        unsafe {
            dealloc(self.ptr, self.layout);
        }
    }
}

/// Reads file bytes asynchronously using Linux `io_uring` zero-copy interface.
pub async fn read_bytes_io_uring(path: &Path, engine: &IoUringEngine) -> Result<Vec<u8>> {
    let file = File::open(path)
        .with_context(|| format!("Failed to open file for io_uring read: {}", path.display()))?;
    let fd = file.as_raw_fd();
    let metadata = file.metadata()
        .with_context(|| format!("Failed to query metadata for file: {}", path.display()))?;
    let file_len = metadata.len() as usize;

    if file_len == 0 {
        return Ok(Vec::new());
    }

    let aligned_buf = AlignedBuffer::new(file_len)?;
    let request = IoRequest::Read {
        fd,
        offset: 0,
        buffer_ptr: aligned_buf.ptr(),
        len: aligned_buf.capacity() as u32,
        user_data: 1001,
    };

    let results = engine.execute_batch_async(vec![request]).await
        .map_err(|e| anyhow::anyhow!("io_uring batch execution error during read: {}", e))?;

    if let Some(res) = results.first() {
        match res.result {
            Ok(bytes_read) => {
                let actual_len = bytes_read.min(file_len);
                Ok(aligned_buf.as_slice()[..actual_len].to_vec())
            }
            Err(errno) => {
                bail!("io_uring read operation failed on {} with errno: {}", path.display(), errno);
            }
        }
    } else {
        bail!("io_uring read returned no result entries for {}", path.display());
    }
}

/// Writes file bytes asynchronously using Linux `io_uring` zero-copy interface + fsync.
pub async fn write_bytes_io_uring(path: &Path, data: &[u8], engine: &IoUringEngine) -> Result<()> {
    if let Some(parent) = path.parent()
        && !parent.exists() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create parent directories for {}", path.display()))?;
        }

    let file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(path)
        .with_context(|| format!("Failed to open file for io_uring write: {}", path.display()))?;
    let fd = file.as_raw_fd();

    let mut aligned_buf = AlignedBuffer::new(data.len())?;
    aligned_buf.as_mut_slice()[..data.len()].copy_from_slice(data);

    let write_req = IoRequest::Write {
        fd,
        offset: 0,
        buffer_ptr: aligned_buf.ptr() as *const u8,
        len: aligned_buf.capacity() as u32,
        user_data: 2001,
    };

    let fsync_req = IoRequest::Fsync {
        fd,
        user_data: 2002,
    };

    let results = engine.execute_batch_async(vec![write_req, fsync_req]).await
        .map_err(|e| anyhow::anyhow!("io_uring batch execution error during write: {}", e))?;

    for res in &results {
        if let Err(errno) = res.result {
            bail!(
                "io_uring write/fsync operation failed on {} (request_id: {}, errno: {})",
                path.display(),
                res.user_data,
                errno
            );
        }
    }

    Ok(())
}

/// Database State Snapshot structure for Athanor Store state persistence.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatabaseSnapshot {
    pub version: u32,
    pub timestamp: u64,
    pub installed_packages: Vec<String>,
    pub registry_url: String,
    pub integrity_hash: String,
}

impl Default for DatabaseSnapshot {
    fn default() -> Self {
        Self {
            version: 1,
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            installed_packages: Vec::new(),
            registry_url: "ghcr.io/ars-regia/athanor-store".to_string(),
            integrity_hash: "sha256:0000000000000000000000000000000000000000000000000000000000000000".to_string(),
        }
    }
}

/// High-level Athanor OS Storage Database Manager using `io_uring`.
pub struct DatabaseEngine {
    io_engine: Arc<IoUringEngine>,
    snapshot_path: PathBuf,
}

impl DatabaseEngine {
    /// Creates a new `DatabaseEngine` backed by `io_uring`.
    pub fn new(sq_depth: u32, cq_depth: u32, snapshot_path: PathBuf) -> Result<Self> {
        let io_engine = Arc::new(
            IoUringEngine::new(sq_depth, cq_depth)
                .map_err(|e| anyhow::anyhow!("Failed to initialize io_uring storage engine: {}", e))?,
        );

        Ok(Self {
            io_engine,
            snapshot_path,
        })
    }

    pub fn io_engine(&self) -> &Arc<IoUringEngine> {
        &self.io_engine
    }

    pub fn snapshot_path(&self) -> &Path {
        &self.snapshot_path
    }

    /// Reads any file via `io_uring` interface.
    pub async fn read_file_io_uring(&self, path: &Path) -> Result<Vec<u8>> {
        read_bytes_io_uring(path, &self.io_engine).await
    }

    /// Writes any file via `io_uring` interface.
    pub async fn write_file_io_uring(&self, path: &Path, data: &[u8]) -> Result<()> {
        write_bytes_io_uring(path, data, &self.io_engine).await
    }

    /// Loads the DB snapshot asynchronously from disk using `io_uring`.
    pub async fn load_snapshot(&self) -> Result<DatabaseSnapshot> {
        if !self.snapshot_path.exists() {
            info!("Database snapshot file not found at {}. Returning default state.", self.snapshot_path.display());
            return Ok(DatabaseSnapshot::default());
        }

        let bytes = self.read_file_io_uring(&self.snapshot_path).await?;
        if bytes.is_empty() {
            return Ok(DatabaseSnapshot::default());
        }

        let snapshot: DatabaseSnapshot = serde_json::from_slice(&bytes)
            .with_context(|| format!("Failed to parse DB snapshot JSON from {}", self.snapshot_path.display()))?;

        info!("Successfully loaded DB snapshot via io_uring ({} packages)", snapshot.installed_packages.len());
        Ok(snapshot)
    }

    /// Asynchronously submits a DB snapshot write operation in a background task via `io_uring`.
    pub fn write_snapshot_background(&self, snapshot: DatabaseSnapshot) -> tokio::task::JoinHandle<Result<()>> {
        let engine = Arc::clone(&self.io_engine);
        let path = self.snapshot_path.clone();

        tokio::spawn(async move {
            info!("Submitting DB snapshot asynchronously to io_uring background task...");
            let data = serde_json::to_vec_pretty(&snapshot)
                .context("Failed to serialize DB snapshot to JSON")?;

            write_bytes_io_uring(&path, &data, &engine).await?;
            info!("Background DB snapshot saved successfully via io_uring to {}", path.display());
            Ok(())
        })
    }
}
