use std::collections::BTreeSet;

use crate::model::{CapabilityKind, HostId, TransportKind};
use crate::utils::{json_array, json_object, quote};

#[derive(Clone, Debug)]
pub struct HostDescriptor {
    pub id: HostId,
    pub display_name: &'static str,
    pub transport: TransportKind,
    pub supports_stateful_plugins: bool,
    pub supported_capability_kinds: BTreeSet<CapabilityKind>,
    pub install_hint: &'static str,
}

impl HostDescriptor {
    pub fn supports_kind(&self, kind: &CapabilityKind) -> bool {
        self.supported_capability_kinds.contains(kind)
    }

    pub fn to_json(&self) -> String {
        json_object([
            ("id".to_string(), quote(self.id.as_str())),
            ("display_name".to_string(), quote(self.display_name)),
            ("transport".to_string(), quote(self.transport.as_str())),
            (
                "supports_stateful_plugins".to_string(),
                self.supports_stateful_plugins.to_string(),
            ),
            (
                "supported_capability_kinds".to_string(),
                json_array(
                    self.supported_capability_kinds
                        .iter()
                        .map(|kind| quote(kind.as_str())),
                ),
            ),
            ("install_hint".to_string(), quote(self.install_hint)),
        ])
    }
}

pub trait HostAdapter {
    fn descriptor(&self) -> HostDescriptor;
}

#[derive(Clone, Debug)]
pub struct HostContext {
    pub descriptor: HostDescriptor,
    pub allows_manual_overrides: bool,
}

impl HostContext {
    pub fn new(descriptor: HostDescriptor) -> Self {
        Self {
            descriptor,
            allows_manual_overrides: true,
        }
    }

    pub fn host_id(&self) -> &HostId {
        &self.descriptor.id
    }

    pub fn supports_kind(&self, kind: &CapabilityKind) -> bool {
        self.descriptor.supports_kind(kind)
    }
}
