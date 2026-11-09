//! Impact graph storage (petgraph) and its serializable export.

use petgraph::stable_graph::{NodeIndex, StableDiGraph};
use petgraph::visit::{EdgeRef, IntoEdgeReferences};
use ripplefix_core::{Confidence, Evidence, Span};
use serde::Serialize;
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeKind {
    Change,
    Model,
    Route,
    TypescriptType,
    TypescriptUsage,
    Test,
}

#[derive(Debug, Clone, Serialize)]
pub struct GraphNode {
    pub id: String,
    pub kind: NodeKind,
    pub label: String,
    pub file: Option<String>,
    pub span: Option<Span>,
}

#[derive(Debug, Clone, Serialize)]
pub struct GraphEdge {
    pub from: String,
    pub to: String,
    pub kind: String,
    pub confidence: Confidence,
    pub evidence: Vec<Evidence>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct GraphExport {
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
}

struct EdgeData {
    kind: String,
    confidence: Confidence,
    evidence: Vec<Evidence>,
}

#[derive(Default)]
pub(crate) struct GraphBuilder {
    graph: StableDiGraph<GraphNode, EdgeData>,
    ids: HashMap<String, NodeIndex>,
}

impl GraphBuilder {
    /// Insert a node, or return the existing one with the same id.
    pub fn node(
        &mut self,
        id: String,
        kind: NodeKind,
        label: impl Into<String>,
        file: Option<&str>,
        span: Option<Span>,
    ) -> NodeIndex {
        if let Some(&ix) = self.ids.get(&id) {
            return ix;
        }
        let ix = self.graph.add_node(GraphNode {
            id: id.clone(),
            kind,
            label: label.into(),
            file: file.map(str::to_owned),
            span,
        });
        self.ids.insert(id, ix);
        ix
    }

    /// Add an edge unless an identical (from, to, kind) edge exists.
    pub fn edge(
        &mut self,
        from: NodeIndex,
        to: NodeIndex,
        kind: &str,
        confidence: Confidence,
        evidence: Vec<Evidence>,
    ) {
        let exists = self
            .graph
            .edges_connecting(from, to)
            .any(|e| e.weight().kind == kind);
        if !exists {
            self.graph.add_edge(
                from,
                to,
                EdgeData {
                    kind: kind.to_owned(),
                    confidence,
                    evidence,
                },
            );
        }
    }

    pub fn export(&self) -> GraphExport {
        GraphExport {
            nodes: self.graph.node_weights().cloned().collect(),
            edges: (&self.graph)
                .edge_references()
                .map(|e| GraphEdge {
                    from: self.graph[e.source()].id.clone(),
                    to: self.graph[e.target()].id.clone(),
                    kind: e.weight().kind.clone(),
                    confidence: e.weight().confidence,
                    evidence: e.weight().evidence.clone(),
                })
                .collect(),
        }
    }
}
