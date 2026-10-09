//! Comments: pinned notes with reply threads, stored in the file.

use serde::{Deserialize, Serialize};

use crate::{Document, NodeId};

/// One reply in a comment thread.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Reply {
    pub author: String,
    pub text: String,
    /// Unix seconds.
    #[serde(default)]
    pub created: u64,
}

/// A comment pinned to a point on a page, optionally anchored to a node so it follows it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Comment {
    pub id: u64,
    pub page: NodeId,
    /// Page coordinates, or the offset in the anchor's local space when `anchor` is set.
    pub x: f64,
    pub y: f64,
    #[serde(default)]
    pub anchor: Option<NodeId>,
    pub author: String,
    pub text: String,
    #[serde(default)]
    pub created: u64,
    #[serde(default)]
    pub resolved: bool,
    #[serde(default)]
    pub replies: Vec<Reply>,
}

impl Comment {
    /// Where the pin sits on its page (anchored comments follow their node).
    pub fn position(&self, doc: &Document) -> (f64, f64) {
        if let Some(a) = self.anchor
            && doc.node(a).is_some()
        {
            let p = doc.world_transform(a) * crate::geom::Point::new(self.x, self.y);
            return (p.x, p.y);
        }
        (self.x, self.y)
    }

    /// Everyone in the thread, in order of first appearance.
    pub fn participants(&self) -> Vec<&str> {
        let mut out: Vec<&str> = vec![self.author.as_str()];
        for r in &self.replies {
            if !out.contains(&r.author.as_str()) {
                out.push(r.author.as_str());
            }
        }
        out
    }
}

impl Document {
    pub fn comment(&self, id: u64) -> Option<&Comment> {
        self.comments.iter().find(|c| c.id == id)
    }

    pub fn comment_mut(&mut self, id: u64) -> Option<&mut Comment> {
        self.comments.iter_mut().find(|c| c.id == id)
    }

    /// The next free comment id.
    pub fn next_comment_id(&self) -> u64 {
        self.comments.iter().map(|c| c.id).max().unwrap_or(0) + 1
    }
}
