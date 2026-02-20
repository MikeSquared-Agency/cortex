use crate::Relation;

pub mod defaults {
    use super::*;

    pub fn informed_by() -> Relation {
        Relation::new("informed_by").unwrap()
    }
    pub fn led_to() -> Relation {
        Relation::new("led_to").unwrap()
    }
    pub fn applies_to() -> Relation {
        Relation::new("applies_to").unwrap()
    }
    pub fn contradicts() -> Relation {
        Relation::new("contradicts").unwrap()
    }
    pub fn supersedes() -> Relation {
        Relation::new("supersedes").unwrap()
    }
    pub fn depends_on() -> Relation {
        Relation::new("depends_on").unwrap()
    }
    pub fn related_to() -> Relation {
        Relation::new("related_to").unwrap()
    }
    pub fn instance_of() -> Relation {
        Relation::new("instance_of").unwrap()
    }
    pub fn uses() -> Relation {
        Relation::new("uses").unwrap()
    }
    pub fn branched_from() -> Relation {
        Relation::new("branched_from").unwrap()
    }
    pub fn inherits_from() -> Relation {
        Relation::new("inherits_from").unwrap()
    }
    pub fn used_by() -> Relation {
        Relation::new("used_by").unwrap()
    }
    pub fn performed() -> Relation {
        Relation::new("performed").unwrap()
    }

    pub fn all() -> Vec<Relation> {
        vec![
            informed_by(),
            led_to(),
            applies_to(),
            contradicts(),
            supersedes(),
            depends_on(),
            related_to(),
            instance_of(),
            uses(),
            branched_from(),
            inherits_from(),
            used_by(),
            performed(),
        ]
    }
}
