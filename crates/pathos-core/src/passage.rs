use serde::{Deserialize, Serialize};
use crate::config::PassageId;
use crate::content::ContentNode;

/// A single passage (narrative node) in the story graph.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PassageNode {
    pub id: PassageId,
    pub tags: Vec<String>,
    /// Fully-parsed content AST (P3 output).
    pub body: Vec<ContentNode>,
    /// Inline scripts (extracted from fenced code blocks within the passage).
    pub scripts: Vec<PassageScript>,
    /// Hooks declared in this passage via `@hook:` directives.
    pub hooks: Vec<HookDeclaration>,
}

/// A script block extracted from a passage, with its language tag.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PassageScript {
    pub lang: String,
    pub code: String,
}

/// A hook declared in a passage body.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookDeclaration {
    pub event: String,
    pub script: PassageScript,
}

/// An edge in the passage graph.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PassageEdge {
    pub from: PassageId,
    pub to: PassageId,
    pub kind: EdgeKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EdgeKind {
    Link,
    Display,
    Script,
}

/// The directed graph of all passages.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PassageGraph {
    pub nodes: Vec<PassageNode>,
    pub edges: Vec<PassageEdge>,
}

impl PassageGraph {
    /// Find a passage node by ID.
    pub fn get(&self, id: &str) -> Option<&PassageNode> {
        self.nodes.iter().find(|n| n.id == id)
    }

    /// Collect outbound links from this passage.
    pub fn outbound(&self, from: &str) -> Vec<&PassageEdge> {
        self.edges.iter().filter(|e| e.from == from).collect()
    }

    /// Build edges from links found in the passage content.
    /// Call after all passages are parsed.
    pub fn rebuild_edges(&mut self) {
        self.edges.clear();
        for node in &self.nodes {
            for cn in &node.body {
                match cn {
                    ContentNode::Link { target, .. } => {
                        self.edges.push(PassageEdge {
                            from: node.id.clone(),
                            to: target.clone(),
                            kind: EdgeKind::Link,
                        });
                    }
                    ContentNode::Display { passage } => {
                        self.edges.push(PassageEdge {
                            from: node.id.clone(),
                            to: passage.clone(),
                            kind: EdgeKind::Display,
                        });
                    }
                    // Script-triggered navigation (game.goto) can't be statically resolved.
                    _ => {}
                }
            }
        }
    }
}
