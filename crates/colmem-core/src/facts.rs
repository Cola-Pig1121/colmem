use std::collections::{BTreeMap, BTreeSet};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::rerank::RerankFactHint;
use crate::utils::{is_meaningful_token, json_array, json_object, quote};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FactQueryScope {
    Active,
    History,
    Scheduled,
    All,
}

impl FactQueryScope {
    fn matches_status(self, status: &str) -> bool {
        match self {
            Self::Active => status == "active",
            Self::History => status == "closed",
            Self::Scheduled => status == "scheduled",
            Self::All => true,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Fact {
    pub subject: String,
    pub predicate: String,
    pub object: String,
    pub valid_from: Option<String>,
    pub valid_to: Option<String>,
    pub confidence: u8,
    pub evidence_ids: Vec<String>,
}

impl Fact {
    fn tokenize(text: &str) -> BTreeSet<String> {
        text.to_ascii_lowercase()
            .split(|c: char| !c.is_ascii_alphanumeric() && c != '_' && c != '-')
            .filter(|token| is_meaningful_token(token))
            .map(|token| token.to_string())
            .collect()
    }

    fn query_match_score(&self, query: &str) -> Option<usize> {
        let query_tokens = Self::tokenize(query);
        if query_tokens.is_empty() {
            return None;
        }

        let haystack = format!(
            "{} {} {}",
            self.subject.to_ascii_lowercase(),
            self.predicate.to_ascii_lowercase(),
            self.object.to_ascii_lowercase()
        );
        if haystack.contains(query.trim().to_ascii_lowercase().as_str()) {
            return Some(query_tokens.len() + 2);
        }

        let fact_tokens = Self::tokenize(&haystack);
        let overlap = query_tokens
            .iter()
            .filter(|token| fact_tokens.contains(*token))
            .count();
        let required_overlap = match query_tokens.len() {
            0 => 0,
            1 => 1,
            2 => 2,
            len => ((len * 2) + 2) / 3,
        };

        if overlap >= required_overlap {
            Some(overlap)
        } else {
            None
        }
    }

    pub fn matches_query(&self, query: &str) -> bool {
        self.query_match_score(query).is_some()
    }

    pub fn summary(&self) -> String {
        format!("{} {} {}", self.subject, self.predicate, self.object)
    }

    pub fn status_on(&self, reference_date: &str) -> &'static str {
        if self
            .valid_from
            .as_deref()
            .is_some_and(|valid_from| valid_from > reference_date)
        {
            "scheduled"
        } else if self.is_active_on(reference_date) {
            "active"
        } else if self.valid_to.is_some() {
            "closed"
        } else {
            "inactive"
        }
    }

    pub fn to_json(&self) -> String {
        json_object([
            ("subject".to_string(), quote(&self.subject)),
            ("predicate".to_string(), quote(&self.predicate)),
            ("object".to_string(), quote(&self.object)),
            (
                "valid_from".to_string(),
                self.valid_from
                    .as_ref()
                    .map(|value| quote(value))
                    .unwrap_or_else(|| "null".to_string()),
            ),
            (
                "valid_to".to_string(),
                self.valid_to
                    .as_ref()
                    .map(|value| quote(value))
                    .unwrap_or_else(|| "null".to_string()),
            ),
            ("confidence".to_string(), self.confidence.to_string()),
            (
                "evidence_ids".to_string(),
                json_array(self.evidence_ids.iter().map(|id| quote(id))),
            ),
        ])
    }

    pub fn to_json_with_status(&self, reference_date: &str) -> String {
        json_object([
            ("subject".to_string(), quote(&self.subject)),
            ("predicate".to_string(), quote(&self.predicate)),
            ("object".to_string(), quote(&self.object)),
            (
                "valid_from".to_string(),
                self.valid_from
                    .as_ref()
                    .map(|value| quote(value))
                    .unwrap_or_else(|| "null".to_string()),
            ),
            (
                "valid_to".to_string(),
                self.valid_to
                    .as_ref()
                    .map(|value| quote(value))
                    .unwrap_or_else(|| "null".to_string()),
            ),
            ("status".to_string(), quote(self.status_on(reference_date))),
            ("reference_date".to_string(), quote(reference_date)),
            ("confidence".to_string(), self.confidence.to_string()),
            (
                "evidence_ids".to_string(),
                json_array(self.evidence_ids.iter().map(|id| quote(id))),
            ),
        ])
    }

    fn valid_from_or_min(&self) -> &str {
        self.valid_from.as_deref().unwrap_or("0000-00-00")
    }

    pub fn is_active_on(&self, reference_date: &str) -> bool {
        let starts_before = self
            .valid_from
            .as_deref()
            .map(|value| value <= reference_date)
            .unwrap_or(true);
        let ends_after = self
            .valid_to
            .as_deref()
            .map(|value| value >= reference_date)
            .unwrap_or(true);
        starts_before && ends_after
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct InMemoryFactStore {
    #[serde(default)]
    facts: Vec<Fact>,
    #[serde(default)]
    audit_log: Vec<FactAuditEvent>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FactAuditEvent {
    pub timestamp: String,
    pub action: String,
    pub subject: String,
    pub predicate: String,
    pub object: String,
    pub effective_at: Option<String>,
    pub note: Option<String>,
}

impl FactAuditEvent {
    fn matches_query(&self, query: &str) -> bool {
        let query_tokens = Fact::tokenize(query);
        if query_tokens.is_empty() {
            return false;
        }
        let haystack = format!(
            "{} {} {} {}",
            self.subject, self.predicate, self.object, self.action
        )
        .to_ascii_lowercase();
        query_tokens
            .iter()
            .all(|token| haystack.contains(token.as_str()))
    }

    pub fn to_json(&self) -> String {
        json_object([
            ("timestamp".to_string(), quote(&self.timestamp)),
            ("action".to_string(), quote(&self.action)),
            ("subject".to_string(), quote(&self.subject)),
            ("predicate".to_string(), quote(&self.predicate)),
            ("object".to_string(), quote(&self.object)),
            (
                "effective_at".to_string(),
                self.effective_at
                    .as_ref()
                    .map(|value| quote(value))
                    .unwrap_or_else(|| "null".to_string()),
            ),
            (
                "note".to_string(),
                self.note
                    .as_ref()
                    .map(|value| quote(value))
                    .unwrap_or_else(|| "null".to_string()),
            ),
        ])
    }
}

pub trait FactStoreBackend {
    fn summary_json(&self, reference_date: &str) -> String;
    fn add_fact(&mut self, fact: Fact);
    fn facts_for_query_scoped(
        &self,
        query: &str,
        scope: FactQueryScope,
        reference_date: &str,
    ) -> Vec<Fact>;
    fn facts_scoped(&self, scope: FactQueryScope, reference_date: &str) -> Vec<Fact>;
    fn audit_log(&self) -> &[FactAuditEvent];
    fn audit_events_for_query(&self, query: &str) -> Vec<FactAuditEvent>;
    fn invalidate_matching(
        &mut self,
        subject: &str,
        predicate: &str,
        object: Option<&str>,
        effective_date: &str,
    ) -> usize;
    fn replace_fact(&mut self, fact: Fact, effective_date: &str) -> usize;
    fn rerank_hints_for_query_scoped(
        &self,
        query: &str,
        scope: FactQueryScope,
        reference_date: &str,
    ) -> Vec<RerankFactHint>;
}

impl FactStoreBackend for InMemoryFactStore {
    fn summary_json(&self, reference_date: &str) -> String {
        InMemoryFactStore::summary_json(self, reference_date)
    }

    fn add_fact(&mut self, fact: Fact) {
        InMemoryFactStore::add_fact(self, fact);
    }

    fn facts_for_query_scoped(
        &self,
        query: &str,
        scope: FactQueryScope,
        reference_date: &str,
    ) -> Vec<Fact> {
        InMemoryFactStore::facts_for_query_scoped(self, query, scope, reference_date)
    }

    fn facts_scoped(&self, scope: FactQueryScope, reference_date: &str) -> Vec<Fact> {
        InMemoryFactStore::facts_scoped(self, scope, reference_date)
    }

    fn audit_log(&self) -> &[FactAuditEvent] {
        InMemoryFactStore::audit_log(self)
    }

    fn audit_events_for_query(&self, query: &str) -> Vec<FactAuditEvent> {
        InMemoryFactStore::audit_events_for_query(self, query)
    }

    fn invalidate_matching(
        &mut self,
        subject: &str,
        predicate: &str,
        object: Option<&str>,
        effective_date: &str,
    ) -> usize {
        InMemoryFactStore::invalidate_matching(self, subject, predicate, object, effective_date)
    }

    fn replace_fact(&mut self, fact: Fact, effective_date: &str) -> usize {
        InMemoryFactStore::replace_fact(self, fact, effective_date)
    }

    fn rerank_hints_for_query_scoped(
        &self,
        query: &str,
        scope: FactQueryScope,
        reference_date: &str,
    ) -> Vec<RerankFactHint> {
        InMemoryFactStore::rerank_hints_for_query_scoped(self, query, scope, reference_date)
    }
}

impl InMemoryFactStore {
    pub fn summary_json(&self, reference_date: &str) -> String {
        let mut active = 0usize;
        let mut history = 0usize;
        let mut scheduled = 0usize;
        let mut inactive = 0usize;
        for fact in &self.facts {
            match fact.status_on(reference_date) {
                "active" => active += 1,
                "closed" => history += 1,
                "scheduled" => scheduled += 1,
                _ => inactive += 1,
            }
        }
        json_object([
            ("reference_date".to_string(), quote(reference_date)),
            ("total".to_string(), self.facts.len().to_string()),
            ("active".to_string(), active.to_string()),
            ("history".to_string(), history.to_string()),
            ("scheduled".to_string(), scheduled.to_string()),
            ("inactive".to_string(), inactive.to_string()),
            ("audit_events".to_string(), self.audit_log.len().to_string()),
        ])
    }

    pub fn add_fact(&mut self, fact: Fact) {
        self.audit_log.push(FactAuditEvent {
            timestamp: fact.valid_from.clone().unwrap_or_else(Self::today_iso_utc),
            action: "created".to_string(),
            subject: fact.subject.clone(),
            predicate: fact.predicate.clone(),
            object: fact.object.clone(),
            effective_at: fact.valid_from.clone(),
            note: None,
        });
        self.facts.push(fact);
    }

    pub fn facts_for_query(&self, query: &str) -> Vec<Fact> {
        self.facts_for_query_scoped(query, FactQueryScope::All, &Self::today_iso_utc())
    }

    pub fn facts_for_query_scoped(
        &self,
        query: &str,
        scope: FactQueryScope,
        reference_date: &str,
    ) -> Vec<Fact> {
        let mut matched = self
            .facts
            .iter()
            .filter_map(|fact| {
                let status = fact.status_on(reference_date);
                if !scope.matches_status(status) {
                    return None;
                }
                fact.query_match_score(query)
                    .map(|score| (score, fact.confidence, fact.clone()))
            })
            .collect::<Vec<_>>();
        matched.sort_by(|left, right| {
            right
                .0
                .cmp(&left.0)
                .then_with(|| right.1.cmp(&left.1))
                .then_with(|| left.2.subject.cmp(&right.2.subject))
                .then_with(|| left.2.predicate.cmp(&right.2.predicate))
                .then_with(|| left.2.object.cmp(&right.2.object))
        });
        matched.into_iter().map(|(_, _, fact)| fact).collect()
    }

    pub fn facts_scoped(&self, scope: FactQueryScope, reference_date: &str) -> Vec<Fact> {
        let mut facts = self
            .facts
            .iter()
            .filter(|fact| scope.matches_status(fact.status_on(reference_date)))
            .cloned()
            .collect::<Vec<_>>();
        facts.sort_by(|left, right| {
            right
                .valid_from
                .cmp(&left.valid_from)
                .then_with(|| right.confidence.cmp(&left.confidence))
                .then_with(|| left.subject.cmp(&right.subject))
                .then_with(|| left.predicate.cmp(&right.predicate))
                .then_with(|| left.object.cmp(&right.object))
        });
        facts
    }

    pub fn all(&self) -> &[Fact] {
        &self.facts
    }

    pub fn all_mut(&mut self) -> &mut [Fact] {
        &mut self.facts
    }

    pub fn best_match_score(&self, query: &str) -> Option<usize> {
        self.facts
            .iter()
            .filter_map(|fact| fact.query_match_score(query))
            .max()
    }

    pub fn audit_log(&self) -> &[FactAuditEvent] {
        &self.audit_log
    }

    pub fn audit_events_for_query(&self, query: &str) -> Vec<FactAuditEvent> {
        let mut events = self
            .audit_log
            .iter()
            .filter(|event| event.matches_query(query))
            .cloned()
            .collect::<Vec<_>>();
        events.sort_by(|left, right| {
            right
                .timestamp
                .cmp(&left.timestamp)
                .then_with(|| left.subject.cmp(&right.subject))
                .then_with(|| left.predicate.cmp(&right.predicate))
                .then_with(|| left.object.cmp(&right.object))
        });
        events
    }

    pub fn ensure_audit_baseline(&mut self) -> bool {
        let mut changed = false;

        for fact in &self.facts {
            let effective_at = fact.valid_from.clone().unwrap_or_else(Self::today_iso_utc);
            let has_baseline = self.audit_log.iter().any(|event| {
                (event.action == "created" || event.action == "imported")
                    && event.subject == fact.subject
                    && event.predicate == fact.predicate
                    && event.object == fact.object
                    && event.effective_at.as_deref() == Some(effective_at.as_str())
            });
            if has_baseline {
                continue;
            }

            self.audit_log.push(FactAuditEvent {
                timestamp: effective_at.clone(),
                action: "imported".to_string(),
                subject: fact.subject.clone(),
                predicate: fact.predicate.clone(),
                object: fact.object.clone(),
                effective_at: Some(effective_at),
                note: Some("baseline fact import".to_string()),
            });
            changed = true;
        }

        changed
    }

    pub fn merge_duplicate_facts(&mut self) -> bool {
        let original_len = self.facts.len();
        let mut merged = Vec::<Fact>::new();
        let mut changed = false;

        for fact in self.facts.drain(..) {
            if let Some(existing) = merged.iter_mut().find(|candidate| {
                candidate.subject == fact.subject
                    && candidate.predicate == fact.predicate
                    && candidate.object == fact.object
                    && candidate.valid_from == fact.valid_from
                    && candidate.valid_to == fact.valid_to
                    && candidate.confidence == fact.confidence
            }) {
                changed = true;
                let mut evidence = existing
                    .evidence_ids
                    .iter()
                    .cloned()
                    .collect::<BTreeSet<_>>();
                let before = evidence.len();
                evidence.extend(fact.evidence_ids.iter().cloned());
                if evidence.len() != before {
                    existing.evidence_ids = evidence.into_iter().collect();
                }
            } else {
                merged.push(fact);
            }
        }

        let removed_duplicates = merged.len() != original_len;
        self.facts = merged;
        changed || removed_duplicates
    }

    pub fn today_iso_utc() -> String {
        let days = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_secs() / 86_400)
            .unwrap_or(0);
        let z = days as i64 + 719_468;
        let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
        let doe = z - era * 146_097;
        let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
        let y = yoe + era * 400;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let d = doy - (153 * mp + 2) / 5 + 1;
        let m = mp + if mp < 10 { 3 } else { -9 };
        let year = y + if m <= 2 { 1 } else { 0 };
        format!("{year:04}-{m:02}-{d:02}")
    }

    pub fn invalidate_matching(
        &mut self,
        subject: &str,
        predicate: &str,
        object: Option<&str>,
        effective_date: &str,
    ) -> usize {
        let mut changed = 0;

        for fact in &mut self.facts {
            if fact.subject != subject || fact.predicate != predicate {
                continue;
            }
            if object.is_some_and(|expected| fact.object != expected) {
                continue;
            }

            let already_closed = fact
                .valid_to
                .as_deref()
                .is_some_and(|existing| existing <= effective_date);
            if already_closed {
                continue;
            }

            if fact
                .valid_from
                .as_deref()
                .is_some_and(|valid_from| valid_from > effective_date)
            {
                fact.valid_from = Some(effective_date.to_string());
            }
            fact.valid_to = Some(effective_date.to_string());
            self.audit_log.push(FactAuditEvent {
                timestamp: effective_date.to_string(),
                action: "invalidated".to_string(),
                subject: fact.subject.clone(),
                predicate: fact.predicate.clone(),
                object: fact.object.clone(),
                effective_at: Some(effective_date.to_string()),
                note: None,
            });
            changed += 1;
        }

        changed
    }

    pub fn replace_fact(&mut self, fact: Fact, effective_date: &str) -> usize {
        let replacement_summary = fact.summary();
        let mut invalidated = 0;
        for candidate in &mut self.facts {
            if candidate.subject != fact.subject || candidate.predicate != fact.predicate {
                continue;
            }

            let already_closed = candidate
                .valid_to
                .as_deref()
                .is_some_and(|existing| existing <= effective_date);
            if already_closed {
                continue;
            }

            if candidate
                .valid_from
                .as_deref()
                .is_some_and(|valid_from| valid_from > effective_date)
            {
                candidate.valid_from = Some(effective_date.to_string());
            }
            candidate.valid_to = Some(effective_date.to_string());
            self.audit_log.push(FactAuditEvent {
                timestamp: effective_date.to_string(),
                action: "superseded".to_string(),
                subject: candidate.subject.clone(),
                predicate: candidate.predicate.clone(),
                object: candidate.object.clone(),
                effective_at: Some(effective_date.to_string()),
                note: Some(format!("replaced_by={replacement_summary}")),
            });
            invalidated += 1;
        }
        self.add_fact(fact);
        self.merge_duplicate_facts();
        invalidated
    }

    pub fn rerank_hints_for_query(&self, query: &str) -> Vec<RerankFactHint> {
        let reference_date = Self::today_iso_utc();
        self.rerank_hints_for_query_scoped(query, FactQueryScope::All, &reference_date)
    }

    pub fn rerank_hints_for_query_scoped(
        &self,
        query: &str,
        scope: FactQueryScope,
        reference_date: &str,
    ) -> Vec<RerankFactHint> {
        let facts = self.facts_for_query_scoped(query, scope, reference_date);
        let mut latest_active_by_relation = BTreeMap::<(String, String), String>::new();
        let mut conflict_count_by_relation = BTreeMap::<(String, String), usize>::new();

        for fact in &facts {
            let key = (fact.subject.clone(), fact.predicate.clone());
            *conflict_count_by_relation.entry(key.clone()).or_insert(0) += 1;
            if fact.is_active_on(&reference_date) {
                let current = latest_active_by_relation
                    .entry(key)
                    .or_insert_with(|| fact.valid_from_or_min().to_string());
                if fact.valid_from_or_min() > current.as_str() {
                    *current = fact.valid_from_or_min().to_string();
                }
            }
        }

        facts
            .into_iter()
            .map(|fact| {
                let key = (fact.subject.clone(), fact.predicate.clone());
                let active = fact.is_active_on(&reference_date);
                let latest_active = latest_active_by_relation.get(&key);
                let conflicting = conflict_count_by_relation.get(&key).copied().unwrap_or(0) > 1;

                let mut adjusted_confidence = i32::from(fact.confidence);
                let mut notes = Vec::new();

                if active {
                    adjusted_confidence += 8;
                    notes.push("currently active".to_string());
                } else if fact.valid_to.is_some() {
                    adjusted_confidence -= 14;
                    notes.push("expired or inactive".to_string());
                }

                if let Some(latest_active) = latest_active {
                    if active && fact.valid_from_or_min() == latest_active {
                        adjusted_confidence += 10;
                        if conflicting {
                            notes.push("latest active fact in conflicting relation".to_string());
                        } else {
                            notes.push("latest active fact".to_string());
                        }
                    } else if conflicting && active {
                        adjusted_confidence -= 8;
                        notes.push("active but superseded by newer fact".to_string());
                    } else if conflicting && !active {
                        adjusted_confidence -= 12;
                        notes.push("older conflicting fact".to_string());
                    }
                }

                RerankFactHint {
                    summary: format!("{} {} {}", fact.subject, fact.predicate, fact.object),
                    tokens: format!("{} {} {}", fact.subject, fact.predicate, fact.object)
                        .to_ascii_lowercase()
                        .split(|c: char| !c.is_ascii_alphanumeric() && c != '_' && c != '-')
                        .filter(|token| is_meaningful_token(token))
                        .map(|token| token.to_string())
                        .collect(),
                    confidence: adjusted_confidence.clamp(1, 99) as u8,
                    reason: if notes.is_empty() {
                        None
                    } else {
                        Some(notes.join(", "))
                    },
                }
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::{Fact, FactQueryScope, FactStoreBackend, InMemoryFactStore};

    #[test]
    fn fact_query_requires_meaningful_overlap() {
        let mut store = InMemoryFactStore::default();
        store.add_fact(Fact {
            subject: "colmem".to_string(),
            predicate: "replaces".to_string(),
            object: "sdk runtime".to_string(),
            valid_from: Some("2026-04-09".to_string()),
            valid_to: None,
            confidence: 90,
            evidence_ids: vec!["replace".to_string()],
        });
        store.add_fact(Fact {
            subject: "colmem".to_string(),
            predicate: "supports".to_string(),
            object: "mcp".to_string(),
            valid_from: Some("2026-04-09".to_string()),
            valid_to: None,
            confidence: 85,
            evidence_ids: vec!["supports".to_string()],
        });

        let matched = store.facts_for_query("colmem supports mcp");
        assert_eq!(matched.len(), 1);
        assert_eq!(matched[0].predicate, "supports");
        assert_eq!(matched[0].object, "mcp");
    }

    #[test]
    fn fact_query_still_matches_partial_semantic_phrase() {
        let mut store = InMemoryFactStore::default();
        store.add_fact(Fact {
            subject: "colmem".to_string(),
            predicate: "prefers".to_string(),
            object: "hybrid retrieval".to_string(),
            valid_from: Some("2026-04-09".to_string()),
            valid_to: None,
            confidence: 93,
            evidence_ids: vec!["retrieval".to_string()],
        });

        let matched = store.facts_for_query("hybrid retrieval design");
        assert_eq!(matched.len(), 1);
        assert_eq!(matched[0].predicate, "prefers");
    }

    #[test]
    fn in_memory_store_satisfies_fact_backend_contract() {
        fn count_active_backend_matches(
            backend: &dyn FactStoreBackend,
            query: &str,
            reference_date: &str,
        ) -> usize {
            backend
                .facts_for_query_scoped(query, FactQueryScope::Active, reference_date)
                .len()
        }

        let mut store = InMemoryFactStore::default();
        FactStoreBackend::add_fact(
            &mut store,
            Fact {
                subject: "colmem".to_string(),
                predicate: "supports".to_string(),
                object: "memory maps".to_string(),
                valid_from: Some("2026-04-13".to_string()),
                valid_to: None,
                confidence: 88,
                evidence_ids: vec!["memory-map".to_string()],
            },
        );

        assert_eq!(
            count_active_backend_matches(&store, "colmem supports memory maps", "2026-04-13"),
            1
        );
        assert_eq!(FactStoreBackend::audit_log(&store).len(), 1);
        assert!(FactStoreBackend::summary_json(&store, "2026-04-13").contains("\"active\": 1"));
    }

    #[test]
    fn newer_active_conflicting_fact_gets_higher_hint_confidence() {
        let mut store = InMemoryFactStore::default();
        store.add_fact(Fact {
            subject: "colmem".to_string(),
            predicate: "prefers".to_string(),
            object: "vector retrieval".to_string(),
            valid_from: Some("2025-01-01".to_string()),
            valid_to: Some("2025-12-31".to_string()),
            confidence: 70,
            evidence_ids: vec!["old".to_string()],
        });
        store.add_fact(Fact {
            subject: "colmem".to_string(),
            predicate: "prefers".to_string(),
            object: "hybrid retrieval".to_string(),
            valid_from: Some("2026-01-01".to_string()),
            valid_to: None,
            confidence: 80,
            evidence_ids: vec!["new".to_string()],
        });

        let hints = store.rerank_hints_for_query("colmem prefers retrieval");
        let old_hint = hints
            .iter()
            .find(|hint| hint.summary.contains("vector retrieval"))
            .expect("old hint");
        let new_hint = hints
            .iter()
            .find(|hint| hint.summary.contains("hybrid retrieval"))
            .expect("new hint");

        assert!(new_hint.confidence > old_hint.confidence);
        assert!(
            new_hint
                .reason
                .as_deref()
                .unwrap_or_default()
                .contains("latest active")
        );
    }

    #[test]
    fn expired_conflicting_fact_is_marked_as_older() {
        let mut store = InMemoryFactStore::default();
        store.add_fact(Fact {
            subject: "colmem".to_string(),
            predicate: "supports".to_string(),
            object: "sdk runtime".to_string(),
            valid_from: Some("2024-01-01".to_string()),
            valid_to: Some("2025-01-01".to_string()),
            confidence: 88,
            evidence_ids: vec!["sdk".to_string()],
        });
        store.add_fact(Fact {
            subject: "colmem".to_string(),
            predicate: "supports".to_string(),
            object: "cli and mcp".to_string(),
            valid_from: Some("2026-01-01".to_string()),
            valid_to: None,
            confidence: 88,
            evidence_ids: vec!["cli".to_string()],
        });

        let hints = store.rerank_hints_for_query("colmem supports");
        let expired = hints
            .iter()
            .find(|hint| hint.summary.contains("sdk runtime"))
            .expect("expired hint");

        assert!(
            expired
                .reason
                .as_deref()
                .unwrap_or_default()
                .contains("older conflicting fact")
        );
    }

    #[test]
    fn duplicate_facts_are_merged_and_evidence_is_combined() {
        let mut store = InMemoryFactStore::default();
        store.add_fact(Fact {
            subject: "colmem".to_string(),
            predicate: "supports".to_string(),
            object: "mcp".to_string(),
            valid_from: Some("2026-04-09".to_string()),
            valid_to: None,
            confidence: 85,
            evidence_ids: vec!["manual-colmem-supports-mcp".to_string()],
        });
        store.add_fact(Fact {
            subject: "colmem".to_string(),
            predicate: "supports".to_string(),
            object: "mcp".to_string(),
            valid_from: Some("2026-04-09".to_string()),
            valid_to: None,
            confidence: 85,
            evidence_ids: vec!["path:crates/colmem-core/src/mcp.rs".to_string()],
        });

        assert!(store.merge_duplicate_facts());
        assert_eq!(store.all().len(), 1);
        assert_eq!(store.all()[0].evidence_ids.len(), 2);
    }

    #[test]
    fn facts_for_query_can_filter_active_and_history() {
        let mut store = InMemoryFactStore::default();
        store.add_fact(Fact {
            subject: "colmem".to_string(),
            predicate: "prefers".to_string(),
            object: "vector retrieval".to_string(),
            valid_from: Some("2026-04-01".to_string()),
            valid_to: Some("2026-04-10".to_string()),
            confidence: 70,
            evidence_ids: vec!["old".to_string()],
        });
        store.add_fact(Fact {
            subject: "colmem".to_string(),
            predicate: "prefers".to_string(),
            object: "hybrid retrieval".to_string(),
            valid_from: Some("2026-04-10".to_string()),
            valid_to: None,
            confidence: 93,
            evidence_ids: vec!["new".to_string()],
        });

        let active = store.facts_for_query_scoped(
            "colmem prefers retrieval",
            FactQueryScope::Active,
            "2026-04-11",
        );
        let history = store.facts_for_query_scoped(
            "colmem prefers retrieval",
            FactQueryScope::History,
            "2026-04-11",
        );

        assert_eq!(active.len(), 1);
        assert_eq!(active[0].object, "hybrid retrieval");
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].object, "vector retrieval");
    }

    #[test]
    fn replace_and_invalidate_write_audit_events() {
        let mut store = InMemoryFactStore::default();
        store.add_fact(Fact {
            subject: "colmem".to_string(),
            predicate: "supports".to_string(),
            object: "sdk runtime".to_string(),
            valid_from: Some("2026-04-01".to_string()),
            valid_to: None,
            confidence: 70,
            evidence_ids: vec!["old".to_string()],
        });
        store.replace_fact(
            Fact {
                subject: "colmem".to_string(),
                predicate: "supports".to_string(),
                object: "mcp".to_string(),
                valid_from: Some("2026-04-10".to_string()),
                valid_to: None,
                confidence: 85,
                evidence_ids: vec!["new".to_string()],
            },
            "2026-04-10",
        );
        store.invalidate_matching("colmem", "supports", Some("mcp"), "2026-04-11");

        let events = store.audit_events_for_query("colmem supports");
        assert!(events.iter().any(|event| event.action == "created"));
        assert!(events.iter().any(|event| event.action == "superseded"));
        assert!(events.iter().any(|event| event.action == "invalidated"));
    }

    #[test]
    fn ensure_audit_baseline_adds_import_events_for_existing_facts() {
        let mut store = InMemoryFactStore::default();
        store.facts.push(Fact {
            subject: "colmem".to_string(),
            predicate: "prefers".to_string(),
            object: "hybrid retrieval".to_string(),
            valid_from: Some("2026-04-09".to_string()),
            valid_to: None,
            confidence: 93,
            evidence_ids: vec!["path:crates/colmem-core/src/retrieval.rs".to_string()],
        });

        assert!(store.ensure_audit_baseline());
        assert_eq!(store.audit_log().len(), 1);
        assert_eq!(store.audit_log()[0].action, "imported");
        assert!(!store.ensure_audit_baseline());
    }

    #[test]
    fn invalidate_matching_closes_open_fact() {
        let mut store = InMemoryFactStore::default();
        store.add_fact(Fact {
            subject: "colmem".to_string(),
            predicate: "supports".to_string(),
            object: "mcp".to_string(),
            valid_from: Some("2026-04-01".to_string()),
            valid_to: None,
            confidence: 85,
            evidence_ids: vec!["path:crates/colmem-core/src/mcp.rs".to_string()],
        });

        let changed = store.invalidate_matching("colmem", "supports", Some("mcp"), "2026-04-10");

        assert_eq!(changed, 1);
        assert_eq!(store.all()[0].valid_to.as_deref(), Some("2026-04-10"));
        assert!(!store.all()[0].is_active_on("2026-04-11"));
    }

    #[test]
    fn replace_fact_invalidates_previous_relation_and_adds_new_fact() {
        let mut store = InMemoryFactStore::default();
        store.add_fact(Fact {
            subject: "colmem".to_string(),
            predicate: "prefers".to_string(),
            object: "vector retrieval".to_string(),
            valid_from: Some("2026-04-01".to_string()),
            valid_to: None,
            confidence: 70,
            evidence_ids: vec!["old".to_string()],
        });

        let invalidated = store.replace_fact(
            Fact {
                subject: "colmem".to_string(),
                predicate: "prefers".to_string(),
                object: "hybrid retrieval".to_string(),
                valid_from: Some("2026-04-10".to_string()),
                valid_to: None,
                confidence: 93,
                evidence_ids: vec!["new".to_string()],
            },
            "2026-04-10",
        );

        assert_eq!(invalidated, 1);
        assert_eq!(store.all().len(), 2);
        let old_fact = store
            .all()
            .iter()
            .find(|fact| fact.object == "vector retrieval")
            .expect("old fact");
        let new_fact = store
            .all()
            .iter()
            .find(|fact| fact.object == "hybrid retrieval")
            .expect("new fact");
        assert_eq!(old_fact.valid_to.as_deref(), Some("2026-04-10"));
        assert_eq!(new_fact.valid_from.as_deref(), Some("2026-04-10"));
        assert!(new_fact.valid_to.is_none());
    }
}
