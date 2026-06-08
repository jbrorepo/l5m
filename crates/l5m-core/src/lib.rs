//! L5M core: security-gated, memory-mapped 5D memory retrieval.
//!
//! Memory safety: the crate denies `unsafe` everywhere except a single audited
//! block (the read-only mmap in `segment.rs`), which carries a `SAFETY` note and
//! an explicit `#[allow(unsafe_code)]`. All other crates in the workspace
//! `forbid(unsafe_code)`.
#![deny(unsafe_code)]

pub mod bitset;
pub mod capsule;
pub mod compiler;
pub mod error;
pub mod frame;
pub mod index;
pub mod probe;
pub mod product;
pub mod relation;
pub mod retrieve;
pub mod scoring;
pub mod segment;
pub mod store;

pub use capsule::MemoryCapsule;
pub use compiler::{compile_segment, CompileOptions};
pub use error::{L5mError, Result};
pub use frame::{CoverageReport, FrameCapsule, MemoryFrame};
pub use probe::MemoryProbe;
pub use product::{
    compile_product_memories, segment_paths_from_product_dir, ProductCompileManifest,
    ProductCompileOptions, ProductViewManifest,
};
pub use relation::{RelationEdge, RelationKind};
pub use retrieve::{retrieve, retrieve_with_timings, RetrievalConfig, RetrievalTimings};
pub use segment::Segment;
pub use store::{MemoryStore, QueryRequest, QueryResponse, RetrievalMode, SegmentMetadata};
