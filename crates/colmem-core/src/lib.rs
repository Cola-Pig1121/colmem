pub mod agent;
pub mod capability;
pub mod context;
pub mod contracts;
pub mod facts;
pub mod harness;
pub mod host;
pub mod ingest;
pub mod mcp;
pub mod model;
pub mod project;
pub mod record;
pub mod rerank;
pub mod retrieval;
#[cfg(feature = "semantic-embeddings")]
pub mod semantic;
pub mod space;
pub mod standard;
pub mod storage;
pub mod utils;

pub use agent::{
    AgentHabitat, AgentProfile, EvolutionPatch, EvolutionSignal, PersonaProfile, PersonaShift,
    SkillProfile,
};
pub use capability::{BindingMode, CapabilityDescriptor, CapabilityRegistry};
pub use context::{ContextPack, ContextPackBuilder, ContextSection};
pub use facts::{Fact, InMemoryFactStore};
pub use harness::{CapabilitySelection, HarnessRuntimeEngine, HarnessSnapshot, TaskIntent};
pub use host::{HostContext, HostDescriptor};
pub use ingest::{
    IngestPolicy, IngestSummary, build_project_index, build_project_index_with_policy,
};
pub use model::{AgentRole, CapabilityKind, HostId, TaskKind, TransportKind};
pub use project::{ProjectHostPolicy, ProjectIngestPolicy, ProjectScope};
pub use record::{
    Chunk, FullTextIndex, IndexState, Record, RecordSourceType, TokenPosting, VectorChunk,
    VectorIndex,
};
pub use rerank::{
    ExternalRerankModel, LightweightReranker, QueryFeatureScore, RerankCandidate, RerankFactHint,
    RerankModelCandidate, RerankModelRequest, RerankModelScore, RerankResult, SourceKind,
    query_feature_score,
};
pub use retrieval::{HybridRetriever, QueryRequest, RetrievalMode, RetrievalPlan, SearchHit};
pub use space::{SpaceGraph, SpaceLink, SpaceLinkKind, SpaceNode};
pub use storage::{EvolutionRecord, WorkspacePaths, WorkspaceState, WorkspaceStateStore};
