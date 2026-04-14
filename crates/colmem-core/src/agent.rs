use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::capability::BindingMode;
use crate::model::AgentRole;
use crate::utils::{clamp_u8, json_array, json_object, quote};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PersonaProfile {
    pub voice: String,
    pub initiative: u8,
    pub risk_appetite: u8,
    pub explanation_depth: u8,
}

impl PersonaProfile {
    pub fn to_json(&self) -> String {
        json_object([
            ("voice".to_string(), quote(&self.voice)),
            ("initiative".to_string(), self.initiative.to_string()),
            ("risk_appetite".to_string(), self.risk_appetite.to_string()),
            (
                "explanation_depth".to_string(),
                self.explanation_depth.to_string(),
            ),
        ])
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct SkillProfile {
    pub domains: BTreeMap<String, u8>,
    pub preferred_capabilities: BTreeSet<String>,
}

impl SkillProfile {
    pub fn to_json(&self) -> String {
        json_object([
            (
                "domains".to_string(),
                json_object(
                    self.domains
                        .iter()
                        .map(|(name, weight)| (name.clone(), weight.to_string())),
                ),
            ),
            (
                "preferred_capabilities".to_string(),
                json_array(self.preferred_capabilities.iter().map(|id| quote(id))),
            ),
        ])
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AgentHabitat {
    pub home_space: String,
    pub accessible_spaces: BTreeSet<String>,
    pub watch_spaces: BTreeSet<String>,
}

impl AgentHabitat {
    pub fn accessible_space_ids(&self) -> BTreeSet<String> {
        let mut spaces = self.accessible_spaces.clone();
        spaces.insert(self.home_space.clone());
        spaces
    }

    pub fn to_json(&self) -> String {
        json_object([
            ("home_space".to_string(), quote(&self.home_space)),
            (
                "accessible_spaces".to_string(),
                json_array(self.accessible_spaces.iter().map(|id| quote(id))),
            ),
            (
                "watch_spaces".to_string(),
                json_array(self.watch_spaces.iter().map(|id| quote(id))),
            ),
        ])
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AgentProfile {
    pub id: String,
    pub display_name: String,
    pub role: AgentRole,
    pub mission: String,
    pub persona: PersonaProfile,
    pub habitat: AgentHabitat,
    pub skill_profile: SkillProfile,
    pub memory_priorities: BTreeMap<String, u8>,
    pub manual_capability_modes: BTreeMap<String, BindingMode>,
}

impl AgentProfile {
    pub fn apply_patch(&mut self, patch: &EvolutionPatch) {
        if let Some(persona) = &patch.persona {
            if let Some(voice) = &persona.voice_override {
                self.persona.voice = voice.clone();
            }
            self.persona.initiative = clamp_u8(self.persona.initiative, persona.initiative_delta);
            self.persona.risk_appetite = clamp_u8(self.persona.risk_appetite, persona.risk_delta);
            self.persona.explanation_depth =
                clamp_u8(self.persona.explanation_depth, persona.explanation_delta);
        }

        for (skill, delta) in &patch.skill_deltas {
            let current = self.skill_profile.domains.get(skill).copied().unwrap_or(50);
            self.skill_profile
                .domains
                .insert(skill.clone(), clamp_u8(current, *delta));
        }

        for capability_id in &patch.preferred_capability_additions {
            self.skill_profile
                .preferred_capabilities
                .insert(capability_id.clone());
        }

        for space_id in &patch.watch_space_additions {
            self.habitat.watch_spaces.insert(space_id.clone());
            self.habitat.accessible_spaces.insert(space_id.clone());
        }

        for (priority, delta) in &patch.memory_priority_deltas {
            let current = self.memory_priorities.get(priority).copied().unwrap_or(50);
            self.memory_priorities
                .insert(priority.clone(), clamp_u8(current, *delta));
        }
    }

    pub fn to_json(&self) -> String {
        json_object([
            ("id".to_string(), quote(&self.id)),
            ("display_name".to_string(), quote(&self.display_name)),
            ("role".to_string(), quote(self.role.as_str())),
            ("mission".to_string(), quote(&self.mission)),
            ("persona".to_string(), self.persona.to_json()),
            ("habitat".to_string(), self.habitat.to_json()),
            ("skill_profile".to_string(), self.skill_profile.to_json()),
            (
                "memory_priorities".to_string(),
                json_object(
                    self.memory_priorities
                        .iter()
                        .map(|(name, weight)| (name.clone(), weight.to_string())),
                ),
            ),
        ])
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct PersonaShift {
    pub voice_override: Option<String>,
    pub initiative_delta: i8,
    pub risk_delta: i8,
    pub explanation_delta: i8,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct EvolutionSignal {
    pub successful_capabilities: BTreeSet<String>,
    pub failed_capabilities: BTreeSet<String>,
    pub promoted_skills: BTreeSet<String>,
    pub discouraged_skills: BTreeSet<String>,
    pub watch_space_additions: BTreeSet<String>,
    pub persona_shift: PersonaShift,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct EvolutionPatch {
    pub persona: Option<PersonaShift>,
    pub skill_deltas: BTreeMap<String, i8>,
    pub preferred_capability_additions: BTreeSet<String>,
    pub watch_space_additions: BTreeSet<String>,
    pub memory_priority_deltas: BTreeMap<String, i8>,
}

impl EvolutionPatch {
    pub fn from_signal(signal: &EvolutionSignal) -> Self {
        let mut patch = Self::default();

        if signal.persona_shift.voice_override.is_some()
            || signal.persona_shift.initiative_delta != 0
            || signal.persona_shift.risk_delta != 0
            || signal.persona_shift.explanation_delta != 0
        {
            patch.persona = Some(signal.persona_shift.clone());
        }

        for skill in &signal.promoted_skills {
            patch.skill_deltas.insert(skill.clone(), 6);
        }
        for skill in &signal.discouraged_skills {
            patch.skill_deltas.insert(skill.clone(), -6);
        }
        for capability_id in &signal.successful_capabilities {
            patch
                .preferred_capability_additions
                .insert(capability_id.clone());
        }
        for capability_id in &signal.failed_capabilities {
            patch
                .memory_priority_deltas
                .insert(format!("avoid::{capability_id}"), 12);
        }
        patch.watch_space_additions = signal.watch_space_additions.clone();
        patch
    }
}
