use std::collections::{BTreeMap, BTreeSet, VecDeque};

use serde::{Deserialize, Serialize};

use crate::agent::AgentHabitat;
use crate::project::ProjectScope;
use crate::utils::{json_array, json_object, quote};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SpaceNode {
    pub id: String,
    pub label: String,
    pub parent_id: Option<String>,
    pub tags: BTreeSet<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum SpaceLinkKind {
    SimilarTo,
    DependsOn,
    References,
    SharedEntity,
    Follows,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SpaceLink {
    pub from: String,
    pub to: String,
    pub kind: SpaceLinkKind,
    pub weight: u8,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct SpaceGraph {
    pub nodes: BTreeMap<String, SpaceNode>,
    pub links: Vec<SpaceLink>,
}

impl SpaceGraph {
    pub fn add_node(&mut self, node: SpaceNode) {
        self.nodes.insert(node.id.clone(), node);
    }

    pub fn add_link(&mut self, link: SpaceLink) {
        self.links.push(link);
    }

    pub fn candidate_spaces(
        &self,
        habitat: &AgentHabitat,
        project: &ProjectScope,
        max_hops: usize,
    ) -> BTreeSet<String> {
        let mut seed = habitat.accessible_space_ids();
        seed.extend(project.focus_spaces.iter().cloned());
        self.expand_from(&seed, max_hops)
    }

    pub fn expand_from(&self, seed: &BTreeSet<String>, max_hops: usize) -> BTreeSet<String> {
        let mut visited = seed.clone();
        let mut frontier = VecDeque::new();

        for space_id in seed {
            frontier.push_back((space_id.clone(), 0usize));
        }

        while let Some((space_id, depth)) = frontier.pop_front() {
            if depth >= max_hops {
                continue;
            }

            for link in self.links.iter().filter(|link| link.weight >= 50) {
                let next = if link.from == space_id {
                    Some(link.to.clone())
                } else if link.to == space_id {
                    Some(link.from.clone())
                } else {
                    None
                };

                if let Some(next_id) = next
                    && visited.insert(next_id.clone())
                {
                    frontier.push_back((next_id, depth + 1));
                }
            }
        }

        visited
    }

    pub fn path_labels(&self, space_id: &str) -> Vec<String> {
        let mut labels = Vec::new();
        let mut seen = BTreeSet::new();
        let mut current = Some(space_id.to_string());

        while let Some(id) = current {
            if !seen.insert(id.clone()) {
                break;
            }

            match self.nodes.get(&id) {
                Some(node) => {
                    labels.push(node.label.clone());
                    current = node.parent_id.clone();
                }
                None => {
                    labels.push(id);
                    break;
                }
            }
        }

        labels.reverse();
        labels
    }

    pub fn path_index(&self) -> BTreeMap<String, Vec<String>> {
        self.nodes
            .keys()
            .map(|space_id| (space_id.clone(), self.path_labels(space_id)))
            .collect()
    }

    pub fn to_memory_map_json(&self) -> String {
        self.memory_map_json_for_nodes(self.nodes.values())
    }

    pub fn to_memory_map_json_for_space(&self, space_id: &str) -> Option<String> {
        let node = self.nodes.get(space_id)?;
        Some(self.memory_map_json_for_nodes(std::iter::once(node)))
    }

    fn memory_map_json_for_nodes<'a>(
        &'a self,
        nodes: impl IntoIterator<Item = &'a SpaceNode>,
    ) -> String {
        let selected_nodes = nodes.into_iter().collect::<Vec<_>>();
        let selected_ids = selected_nodes
            .iter()
            .map(|node| node.id.clone())
            .collect::<BTreeSet<_>>();
        let nodes = selected_nodes.into_iter().map(|node| {
            let path = self.path_labels(&node.id);
            json_object([
                ("id".to_string(), quote(&node.id)),
                ("label".to_string(), quote(&node.label)),
                (
                    "parent_id".to_string(),
                    node.parent_id
                        .as_ref()
                        .map(|parent| quote(parent))
                        .unwrap_or_else(|| "null".to_string()),
                ),
                (
                    "path".to_string(),
                    json_array(path.iter().map(|segment| quote(segment))),
                ),
                ("memory_path".to_string(), quote(&path.join(" > "))),
                (
                    "tags".to_string(),
                    json_array(node.tags.iter().map(|tag| quote(tag))),
                ),
            ])
        });
        let links = self
            .links
            .iter()
            .filter(|link| {
                selected_ids.is_empty()
                    || selected_ids.contains(&link.from)
                    || selected_ids.contains(&link.to)
            })
            .map(|link| {
                json_object([
                    ("from".to_string(), quote(&link.from)),
                    ("to".to_string(), quote(&link.to)),
                    ("kind".to_string(), quote(link.kind.as_str())),
                    ("weight".to_string(), link.weight.to_string()),
                ])
            });

        json_object([
            ("nodes".to_string(), json_array(nodes)),
            ("links".to_string(), json_array(links)),
        ])
    }
}

impl SpaceLinkKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::SimilarTo => "similar_to",
            Self::DependsOn => "depends_on",
            Self::References => "references",
            Self::SharedEntity => "shared_entity",
            Self::Follows => "follows",
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::{SpaceGraph, SpaceNode};

    #[test]
    fn path_labels_returns_root_to_leaf_memory_path() {
        let mut graph = SpaceGraph::default();
        graph.add_node(SpaceNode {
            id: "root".to_string(),
            label: "Root".to_string(),
            parent_id: None,
            tags: BTreeSet::new(),
        });
        graph.add_node(SpaceNode {
            id: "retrieval".to_string(),
            label: "Retrieval".to_string(),
            parent_id: Some("root".to_string()),
            tags: BTreeSet::new(),
        });

        assert_eq!(
            graph.path_labels("retrieval"),
            vec!["Root".to_string(), "Retrieval".to_string()]
        );
        assert_eq!(
            graph.path_index().get("retrieval").expect("retrieval path"),
            &vec!["Root".to_string(), "Retrieval".to_string()]
        );
        assert!(
            graph
                .to_memory_map_json()
                .contains("\"memory_path\": \"Root > Retrieval\"")
        );
        assert!(
            graph
                .to_memory_map_json_for_space("retrieval")
                .expect("filtered memory map")
                .contains("\"id\": \"retrieval\"")
        );
    }
}
