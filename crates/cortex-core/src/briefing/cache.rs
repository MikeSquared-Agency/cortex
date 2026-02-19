use super::Briefing;
use std::collections::HashMap;
use std::time::{Duration, Instant};

pub struct CachedBriefing {
    pub briefing: Briefing,
    pub generated_at: Instant,
    pub graph_version: u64,
}

pub struct BriefingCache {
    entries: HashMap<String, CachedBriefing>,
    ttl: Duration,
}

impl BriefingCache {
    pub fn new(ttl: Duration) -> Self {
        Self {
            entries: HashMap::new(),
            ttl,
        }
    }

    /// Return cached briefing if version matches and TTL not expired.
    pub fn get(&self, agent_id: &str, current_version: u64) -> Option<&Briefing> {
        self.entries.get(agent_id).and_then(|e| {
            if e.graph_version == current_version && e.generated_at.elapsed() < self.ttl {
                Some(&e.briefing)
            } else {
                None
            }
        })
    }

    pub fn put(&mut self, agent_id: &str, briefing: Briefing, version: u64) {
        self.entries.insert(
            agent_id.to_string(),
            CachedBriefing {
                briefing,
                generated_at: Instant::now(),
                graph_version: version,
            },
        );
    }

    pub fn invalidate(&mut self, agent_id: &str) {
        self.entries.remove(agent_id);
    }
}
