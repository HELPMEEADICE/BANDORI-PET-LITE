//! Model archive handling for BANDORI-PET-LITE.
//!
//! Bandori ships each character as a single `models/<character>.zst` file: a
//! zstd-compressed tar where every costume is a top-level directory holding a
//! Cubism model. Neocari's loaders are path based, so the pet process extracts the
//! one costume it needs into a cache directory and then hands Neocari the real
//! `model.json` / `*.model3.json` path.
//!
//! * [`index`] — one-shot scan of `models/*.zst`, cached in
//!   `models/.bandori_model_index.json` and invalidated by `size`+`mtime`.
//! * [`cache`] — extraction of a costume into
//!   `%LOCALAPPDATA%\BandoriPetLite\cache\<archive-stem>\<costume>\` with an
//!   8-entry LRU.
//! * [`catalog`] — characters/costumes joined with `band.json` and `outfit.json`
//!   display names for the settings window.
//! * [`virtual_path`] — the `models/kasumi.zst::live_default/model.json` syntax.

pub mod cache;
pub mod catalog;
pub mod download;
pub mod index;
pub mod virtual_path;

pub use cache::{CacheError, ModelCache};
pub use catalog::{Catalog, CatalogCharacter, CatalogCostume};
pub use index::{ArchiveEntry, ArchiveScan, CostumeEntry, ModelIndex, ModelIndexError};
pub use virtual_path::{ModelPath, VirtualPathError};

/// Name of the per-archive member list written next to the tar contents.
pub const ARCHIVE_INDEX_MEMBER: &str = ".bandori_zst_index.json";
/// Name of the scan cache inside `models/`.
pub const MODEL_INDEX_FILE: &str = ".bandori_model_index.json";
/// Extension of a Bandori model archive.
pub const ARCHIVE_EXTENSION: &str = "zst";
