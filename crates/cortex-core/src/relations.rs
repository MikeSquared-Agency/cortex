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
        ]
    }
}
