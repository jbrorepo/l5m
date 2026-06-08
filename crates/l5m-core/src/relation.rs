use serde::{Deserialize, Serialize};

#[derive(Copy, Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum RelationKind {
    Supports,
    Contradicts,
    Supersedes,
    DependsOn,
    DerivedFrom,
    DuplicateOf,
}

impl RelationKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Supports => "Supports",
            Self::Contradicts => "Contradicts",
            Self::Supersedes => "Supersedes",
            Self::DependsOn => "DependsOn",
            Self::DerivedFrom => "DerivedFrom",
            Self::DuplicateOf => "DuplicateOf",
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct RelationEdge {
    pub from: u128,
    pub to: u128,
    pub kind: RelationKind,
    pub weight: i16,
}
