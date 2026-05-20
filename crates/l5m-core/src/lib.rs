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
pub use retrieve::{retrieve, RetrievalConfig};
pub use segment::Segment;
pub use store::{MemoryStore, QueryRequest, QueryResponse, RetrievalMode, SegmentMetadata};
