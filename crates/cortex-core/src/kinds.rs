use crate::NodeKind;

/// The 8 default node kinds shipped with Cortex.
/// Users may define additional kinds in cortex.toml.
pub mod defaults {
    use super::*;

    pub fn agent() -> NodeKind {
        NodeKind::new("agent").unwrap()
    }
    pub fn decision() -> NodeKind {
        NodeKind::new("decision").unwrap()
    }
    pub fn fact() -> NodeKind {
        NodeKind::new("fact").unwrap()
    }
    pub fn event() -> NodeKind {
        NodeKind::new("event").unwrap()
    }
    pub fn goal() -> NodeKind {
        NodeKind::new("goal").unwrap()
    }
    pub fn preference() -> NodeKind {
        NodeKind::new("preference").unwrap()
    }
    pub fn pattern() -> NodeKind {
        NodeKind::new("pattern").unwrap()
    }
    pub fn observation() -> NodeKind {
        NodeKind::new("observation").unwrap()
    }
    pub fn prompt() -> NodeKind {
        NodeKind::new("prompt").unwrap()
    }

    pub fn all() -> Vec<NodeKind> {
        vec![
            agent(),
            decision(),
            fact(),
            event(),
            goal(),
            preference(),
            pattern(),
            observation(),
            prompt(),
        ]
    }
}
