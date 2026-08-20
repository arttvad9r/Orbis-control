//! Diagnostic policy/control-flow trace.
//!
//! The graph is descriptive evidence for advanced diagnostics. It is not an
//! executable workflow and cannot bypass capability or reconciliation policy.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::FeatureId;

/// Kind of one diagnostic control-flow node.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ControlFlowNodeKind {
    /// Runtime trigger such as AC/battery/process/GameMode.
    Trigger,
    /// Selected preset/policy.
    Preset,
    /// Desired state owned by Orbis.
    Desired { feature: FeatureId },
    /// Capability/readiness gate.
    Capability { feature: FeatureId },
    /// Reconciliation decision.
    Reconciliation { feature: FeatureId },
    /// Typed mutation attempt.
    Mutation { feature: FeatureId },
    /// Authoritative observation/read-back.
    Observation { feature: FeatureId },
}

/// One node in a diagnostic flow graph.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ControlFlowNode {
    /// Stable graph-local id.
    pub id: String,
    /// Node semantic kind.
    pub kind: ControlFlowNodeKind,
    /// Short technical label.
    pub label: String,
}

/// Directed edge between graph nodes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ControlFlowEdge {
    /// Source node id.
    pub from: String,
    /// Destination node id.
    pub to: String,
    /// Optional technical edge label.
    pub label: Option<String>,
}

/// Read-only graph for diagnostics/UI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ControlFlowGraph {
    /// Nodes.
    pub nodes: Vec<ControlFlowNode>,
    /// Directed edges.
    pub edges: Vec<ControlFlowEdge>,
}

/// Structural graph validation error.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ControlFlowError {
    /// Node ids must be unique and non-empty.
    #[error("duplicate or empty control-flow node id: {0}")]
    InvalidNodeId(String),
    /// Edge references a node that does not exist.
    #[error("control-flow edge references missing node: {0}")]
    MissingNode(String),
}

impl ControlFlowGraph {
    /// Validate graph structure.
    ///
    /// Cycles are allowed because runtime policy may intentionally express
    /// feedback observation -> reconciliation. This validator checks only
    /// identity/reference integrity.
    pub fn validate(&self) -> Result<(), ControlFlowError> {
        let mut ids = BTreeSet::new();
        for node in &self.nodes {
            if node.id.is_empty() || !ids.insert(node.id.as_str()) {
                return Err(ControlFlowError::InvalidNodeId(node.id.clone()));
            }
        }
        for edge in &self.edges {
            if !ids.contains(edge.from.as_str()) {
                return Err(ControlFlowError::MissingNode(edge.from.clone()));
            }
            if !ids.contains(edge.to.as_str()) {
                return Err(ControlFlowError::MissingNode(edge.to.clone()));
            }
        }
        Ok(())
    }

    /// Add a node. Call [`Self::validate`] before publication.
    pub fn push_node(&mut self, node: ControlFlowNode) {
        self.nodes.push(node);
    }

    /// Add an edge. Call [`Self::validate`] before publication.
    pub fn push_edge(&mut self, edge: ControlFlowEdge) {
        self.edges.push(edge);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_flow_can_show_trigger_to_observation() {
        let graph = ControlFlowGraph {
            nodes: vec![
                ControlFlowNode {
                    id: "trigger".into(),
                    kind: ControlFlowNodeKind::Trigger,
                    label: "AC".into(),
                },
                ControlFlowNode {
                    id: "desired".into(),
                    kind: ControlFlowNodeKind::Desired {
                        feature: FeatureId::Performance,
                    },
                    label: "Turbo".into(),
                },
                ControlFlowNode {
                    id: "observed".into(),
                    kind: ControlFlowNodeKind::Observation {
                        feature: FeatureId::Performance,
                    },
                    label: "Balanced".into(),
                },
            ],
            edges: vec![
                ControlFlowEdge {
                    from: "trigger".into(),
                    to: "desired".into(),
                    label: None,
                },
                ControlFlowEdge {
                    from: "desired".into(),
                    to: "observed".into(),
                    label: Some("read-back".into()),
                },
            ],
        };
        assert!(graph.validate().is_ok());
    }

    #[test]
    fn duplicate_node_is_rejected() {
        let graph = ControlFlowGraph {
            nodes: vec![
                ControlFlowNode {
                    id: "x".into(),
                    kind: ControlFlowNodeKind::Trigger,
                    label: String::new(),
                },
                ControlFlowNode {
                    id: "x".into(),
                    kind: ControlFlowNodeKind::Preset,
                    label: String::new(),
                },
            ],
            edges: Vec::new(),
        };
        assert_eq!(
            graph.validate(),
            Err(ControlFlowError::InvalidNodeId("x".into()))
        );
    }

    #[test]
    fn missing_edge_endpoint_is_rejected() {
        let graph = ControlFlowGraph {
            nodes: vec![ControlFlowNode {
                id: "a".into(),
                kind: ControlFlowNodeKind::Trigger,
                label: String::new(),
            }],
            edges: vec![ControlFlowEdge {
                from: "a".into(),
                to: "missing".into(),
                label: None,
            }],
        };
        assert_eq!(
            graph.validate(),
            Err(ControlFlowError::MissingNode("missing".into()))
        );
    }
}
