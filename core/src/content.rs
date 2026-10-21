//! Presentation structure, not a solver AST. Incorrect/incomplete notation is data.
use crate::{Error, Id, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MathNode {
    pub id: Id,
    pub form: MathForm,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "value", deny_unknown_fields)]
pub enum MathForm {
    Token(String),
    Text(String),
    Row(Vec<MathNode>),
    Fraction {
        numerator: Box<MathNode>,
        denominator: Box<MathNode>,
    },
    Root {
        radicand: Box<MathNode>,
        index: Option<Box<MathNode>>,
    },
    Scripts {
        base: Box<MathNode>,
        sub: Option<Box<MathNode>>,
        sup: Option<Box<MathNode>>,
    },
    Fence {
        open: String,
        body: Box<MathNode>,
        close: Option<String>,
    },
    Matrix(Vec<Vec<MathNode>>),
    AlignedLines(Vec<MathNode>),
    Hole,
    Unknown(String),
    Unparsed(String),
    Alternatives(Vec<MathNode>),
}
impl MathNode {
    pub fn token(value: impl Into<String>) -> Self {
        Self {
            id: Id::new(),
            form: MathForm::Token(value.into()),
        }
    }
    fn children(&self) -> Vec<&MathNode> {
        match &self.form {
            MathForm::Row(v) | MathForm::AlignedLines(v) | MathForm::Alternatives(v) => {
                v.iter().collect()
            }
            MathForm::Fraction {
                numerator,
                denominator,
            } => vec![numerator, denominator],
            MathForm::Root { radicand, index } => std::iter::once(radicand.as_ref())
                .chain(index.as_deref())
                .collect(),
            MathForm::Scripts { base, sub, sup } => std::iter::once(base.as_ref())
                .chain(sub.as_deref())
                .chain(sup.as_deref())
                .collect(),
            MathForm::Fence { body, .. } => vec![body],
            MathForm::Matrix(rows) => rows.iter().flatten().collect(),
            _ => vec![],
        }
    }
    pub fn validate(&self) -> Result<()> {
        let mut seen = BTreeSet::new();
        let mut stack = vec![(self, 0)];
        while let Some((n, depth)) = stack.pop() {
            if depth > 64 || seen.len() > 10_000 {
                return Err(Error::ResourceLimit);
            }
            if !seen.insert(n.id) {
                return Err(Error::Invalid("duplicate maths node"));
            }
            match &n.form {
                MathForm::Token(t)
                | MathForm::Text(t)
                | MathForm::Unknown(t)
                | MathForm::Unparsed(t)
                    if t.len() > 1_000_000 =>
                {
                    return Err(Error::ResourceLimit)
                }
                _ => {}
            }
            stack.extend(n.children().into_iter().map(|c| (c, depth + 1)));
        }
        Ok(())
    }
    /// A literal search projection. Does not claim mathematical equivalence.
    pub fn literal(&self) -> String {
        match &self.form {
            MathForm::Token(s)
            | MathForm::Text(s)
            | MathForm::Unknown(s)
            | MathForm::Unparsed(s) => s.clone(),
            MathForm::Hole => String::new(),
            _ => self
                .children()
                .iter()
                .map(|n| n.literal())
                .collect::<Vec<_>>()
                .join(" "),
        }
    }
    /// Replace only the chosen presentation subtree; never simplify its neighbours.
    pub fn replace(&mut self, target: Id, replacement: MathNode) -> Result<()> {
        fn walk(node: &mut MathNode, id: Id, new: &MathNode) -> bool {
            if node.id == id {
                *node = new.clone();
                return true;
            }
            match &mut node.form {
                MathForm::Row(v) | MathForm::AlignedLines(v) | MathForm::Alternatives(v) => {
                    v.iter_mut().any(|n| walk(n, id, new))
                }
                MathForm::Fraction {
                    numerator,
                    denominator,
                } => walk(numerator, id, new) || walk(denominator, id, new),
                MathForm::Root { radicand, index } => {
                    walk(radicand, id, new)
                        || index.as_deref_mut().is_some_and(|n| walk(n, id, new))
                }
                MathForm::Scripts { base, sub, sup } => {
                    walk(base, id, new)
                        || sub.as_deref_mut().is_some_and(|n| walk(n, id, new))
                        || sup.as_deref_mut().is_some_and(|n| walk(n, id, new))
                }
                MathForm::Fence { body, .. } => walk(body, id, new),
                MathForm::Matrix(rows) => rows.iter_mut().flatten().any(|n| walk(n, id, new)),
                _ => false,
            }
        }
        let mut candidate = self.clone();
        if !walk(&mut candidate, target, &replacement) {
            return Err(Error::Unavailable);
        }
        candidate.validate()?;
        *self = candidate;
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "value", deny_unknown_fields)]
pub enum Content {
    Ink,
    Text(String),
    Numeric(String),
    Code {
        text: String,
        language: Option<String>,
    },
    Maths(MathNode),
}
impl Content {
    pub fn validate(&self) -> Result<()> {
        match self {
            Self::Maths(n) => n.validate(),
            Self::Code { text, .. } | Self::Text(text) | Self::Numeric(text)
                if text.len() > 4_000_000 =>
            {
                Err(Error::ResourceLimit)
            }
            _ => Ok(()),
        }
    }
    pub fn literal(&self) -> String {
        match self {
            Self::Ink => String::new(),
            Self::Text(t) | Self::Numeric(t) | Self::Code { text: t, .. } => t.clone(),
            Self::Maths(n) => n.literal(),
        }
    }
}
