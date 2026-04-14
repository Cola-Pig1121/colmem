use crate::agent::{AgentProfile, EvolutionPatch, EvolutionSignal};
use crate::capability::{CapabilityDescriptor, CapabilityRegistry};
use crate::context::ContextPack;
use crate::facts::Fact;
use crate::harness::{CapabilitySelection, HarnessSnapshot, TaskIntent};
use crate::project::ProjectScope;
use crate::retrieval::{QueryRequest, RetrievalPlan, SearchHit};

pub trait RecordIngestor {}

pub trait ChunkIndexer {}

pub trait SpaceResolver {}

pub trait Retriever {
    fn plan(&self, request: &QueryRequest) -> RetrievalPlan;
}

pub trait FactStore {
    fn facts_for_query(&self, query: &str) -> Vec<Fact>;
}

pub trait ContextBuilder {
    fn build(&self, agent: &AgentProfile, hits: &[SearchHit], facts: &[Fact]) -> ContextPack;
}

pub trait AgentStore {
    fn get_agent(&self, id: &str) -> Option<AgentProfile>;
}

pub trait CapabilityProvider {
    fn capabilities(&self) -> Vec<CapabilityDescriptor>;
}

pub trait CapabilityRegistryTrait {
    fn registry(&self) -> &CapabilityRegistry;
}

pub trait BindingResolver {
    fn resolve(
        &self,
        agent: &AgentProfile,
        project: &ProjectScope,
        task: &TaskIntent,
    ) -> CapabilitySelection;
}

pub trait EvolutionEngine {
    fn evolve(&self, agent: &AgentProfile, signal: &EvolutionSignal) -> EvolutionPatch;
}

pub trait HarnessRuntime {
    fn prepare_run(
        &self,
        agent: &AgentProfile,
        project: &ProjectScope,
        task: &TaskIntent,
    ) -> HarnessSnapshot;
}
