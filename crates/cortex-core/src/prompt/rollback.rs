/// Automatic prompt rollback on performance degradation (issue #23).
///
/// When a new prompt version is deployed, a `RollbackMonitor` records baseline metrics
/// and watches each subsequent observation. If correction rates or sentiment scores
/// deviate from baseline beyond configurable σ thresholds, the monitor automatically
/// rolls back to the previous version and creates a full audit trail in the graph.
use std::sync::Arc;

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

use crate::{
    kinds::defaults as kinds,
    relations::defaults as rels,
    storage::{NodeFilter, Storage},
    types::{Edge, EdgeProvenance, Node, NodeId, Source},
    Result,
};

// ── Configuration ─────────────────────────────────────────────────────────────

/// Configuration for automatic prompt rollback on performance degradation.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct RollbackConfig {
    /// Whether automatic rollback is enabled.
    pub enabled: bool,
    /// Number of interactions to monitor after deployment before marking stable.
    pub monitoring_window: u32,
    /// Minimum observations before σ-based triggers activate.
    pub min_samples_before_check: u32,
    /// Correction-rate σ deviation that triggers a warning (informational only).
    pub correction_rate_warning: f32,
    /// Correction-rate σ deviation that triggers auto-rollback.
    pub correction_rate_rollback: f32,
    /// Absolute increase in correction rate (0.0–1.0) that triggers rollback.
    pub absolute_correction_increase: f32,
    /// Sentiment σ decline that triggers a warning.
    pub sentiment_warning: f32,
    /// Sentiment σ decline that triggers auto-rollback.
    pub sentiment_rollback: f32,
    /// Consecutive negative observations (score < 0.4) that trigger rollback.
    pub consecutive_negative_limit: u32,
    /// Base cooldown hours after a rollback. Doubles on each subsequent rollback.
    pub cooldown_base_hours: u32,
    /// Number of rollbacks before a version is quarantined (requires manual override).
    pub max_rollbacks_before_quarantine: u32,
}

impl Default for RollbackConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            monitoring_window: 20,
            min_samples_before_check: 5,
            correction_rate_warning: 2.0,
            correction_rate_rollback: 3.0,
            absolute_correction_increase: 0.25,
            sentiment_warning: 1.5,
            sentiment_rollback: 2.0,
            consecutive_negative_limit: 3,
            cooldown_base_hours: 1,
            max_rollbacks_before_quarantine: 3,
        }
    }
}

// ── Result types ───────────────────────────────────────────────────────────────

/// What triggered a rollback.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RollbackTrigger {
    CorrectionRateSigma {
        sigma: f32,
        post_rate: f32,
        baseline: f32,
    },
    SentimentSigma {
        sigma: f32,
        post_sentiment: f32,
        baseline: f32,
    },
    AbsoluteCorrectionIncrease {
        increase: f32,
    },
    ConsecutiveNegative {
        count: u32,
    },
}

impl RollbackTrigger {
    pub fn kind_str(&self) -> &'static str {
        match self {
            Self::CorrectionRateSigma { .. } => "correction_rate_sigma",
            Self::SentimentSigma { .. } => "sentiment_sigma",
            Self::AbsoluteCorrectionIncrease { .. } => "absolute_correction_increase",
            Self::ConsecutiveNegative { .. } => "consecutive_negative",
        }
    }
}

/// Outcome of a successful rollback.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RollbackResult {
    pub rollback_node_id: NodeId,
    pub from_node_id: NodeId,
    pub from_version: u32,
    pub to_node_id: NodeId,
    pub to_version: u32,
    pub trigger: RollbackTrigger,
    pub cooldown_hours: u32,
    pub cooldown_expires_at: DateTime<Utc>,
    pub is_quarantined: bool,
    pub rollback_count: u32,
}

/// Summary of a past rollback (for status reporting).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RollbackSummary {
    pub rollback_node_id: NodeId,
    pub from_version: u32,
    pub to_version: u32,
    pub trigger: String,
    pub rolled_back_at: DateTime<Utc>,
    pub cooldown_hours: u32,
}

/// Information about the active monitoring window for a deployed version.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActiveDeploymentInfo {
    pub deployment_node_id: NodeId,
    pub prompt_node_id: NodeId,
    pub version: u32,
    pub agent_name: String,
    pub deployed_at: DateTime<Utc>,
    pub n_observed: u32,
    pub monitoring_window: u32,
    pub baseline_correction_rate: f32,
    pub baseline_sentiment: f32,
    pub mean_correction: f32,
    pub mean_sentiment: f32,
    pub consecutive_negative: u32,
}

/// Current rollback status for a prompt slug+branch.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RollbackStatus {
    pub slug: String,
    pub branch: String,
    pub head_node_id: NodeId,
    pub current_version: u32,
    pub is_quarantined: bool,
    pub rollback_count: u32,
    pub cooldown_expires_at: Option<DateTime<Utc>>,
    pub active_deployment: Option<ActiveDeploymentInfo>,
    pub recent_rollbacks: Vec<RollbackSummary>,
}

// ── Monitor ────────────────────────────────────────────────────────────────────

/// Monitors deployed prompt versions for performance degradation and auto-rolls back.
pub struct RollbackMonitor<S: Storage> {
    storage: Arc<S>,
    config: RollbackConfig,
}

impl<S: Storage> RollbackMonitor<S> {
    pub fn new(storage: Arc<S>, config: RollbackConfig) -> Self {
        Self { storage, config }
    }

    /// Record a new deployment and snapshot baseline metrics.
    ///
    /// `baseline_obs`: recent `(correction_rate, sentiment_score)` pairs sampled from
    /// observations *before* this deployment. Used to establish the baseline mean/stddev.
    ///
    /// Returns the `NodeId` of the deployment event node.
    pub fn record_deployment(
        &self,
        slug: &str,
        branch: &str,
        version: u32,
        prompt_node_id: NodeId,
        agent_name: &str,
        baseline_obs: Vec<(f32, f32)>,
    ) -> Result<NodeId> {
        let (baseline_correction, baseline_stddev_correction, baseline_sentiment, baseline_stddev_sentiment) =
            compute_baseline_stats(&baseline_obs);

        let body = serde_json::json!({
            "event_type": "deployment",
            "slug": slug,
            "branch": branch,
            "version": version,
            "prompt_node_id": prompt_node_id.to_string(),
            "agent_name": agent_name,
            "baseline_correction_rate": baseline_correction,
            "baseline_sentiment": baseline_sentiment,
            "baseline_stddev_correction": baseline_stddev_correction,
            "baseline_stddev_sentiment": baseline_stddev_sentiment,
            "baseline_sample_size": baseline_obs.len(),
            "monitoring_window": self.config.monitoring_window,
            "n_observed": 0u32,
            "m2_correction": 0.0f32,
            "mean_correction": baseline_correction,
            "m2_sentiment": 0.0f32,
            "mean_sentiment": baseline_sentiment,
            "consecutive_negative": 0u32,
            "status": "monitoring",
        });

        let deployment_node = Node::new(
            kinds::event(),
            format!("deployment:{}/{}/v{}", slug, branch, version),
            body.to_string(),
            Source { agent: agent_name.to_string(), session: None, channel: None },
            1.0,
        );
        self.storage.put_node(&deployment_node)?;

        // Link: deployment_event --deployed--> prompt_version
        self.storage.put_edge(&Edge::new(
            deployment_node.id,
            prompt_node_id,
            rels::deployed(),
            1.0,
            EdgeProvenance::Manual { created_by: agent_name.to_string() },
        ))?;

        Ok(deployment_node.id)
    }

    /// Process an observation for a specific prompt version.
    ///
    /// If the version is under a monitoring window, updates Welford running stats and
    /// checks degradation triggers. Returns `Some(RollbackResult)` if rollback fired.
    pub fn process_observation(
        &self,
        obs_node_id: NodeId,
        prompt_node_id: NodeId,
        correction_rate: f32,
        sentiment: f32,
        obs_score: f32,
    ) -> Result<Option<RollbackResult>> {
        if !self.config.enabled {
            return Ok(None);
        }

        // Guard: skip if this version is already in a cooldown window. This prevents
        // a burst of observations from firing multiple rollbacks before the cooldown
        // state is visible to subsequent calls.
        if self.is_in_cooldown(prompt_node_id)? {
            return Ok(None);
        }

        // Find the active (status=monitoring) deployment event for this prompt version.
        let deployment_rel = rels::deployed();
        let mut deployment_nodes: Vec<Node> = self
            .storage
            .edges_to(prompt_node_id)?
            .into_iter()
            .filter(|e| e.relation == deployment_rel)
            .filter_map(|e| self.storage.get_node(e.from).ok().flatten())
            .filter(|n| n.kind == kinds::event())
            .filter(|n| is_active_deployment(n))
            .collect();

        if deployment_nodes.is_empty() {
            return Ok(None);
        }

        // Most-recent active deployment wins.
        deployment_nodes.sort_by_key(|n| n.created_at);
        let mut deployment_node = deployment_nodes.pop().unwrap();

        let body: serde_json::Value = serde_json::from_str(&deployment_node.data.body)
            .map_err(|e| crate::CortexError::Validation(format!("bad deployment body: {}", e)))?;

        let monitoring_window =
            body["monitoring_window"].as_u64().unwrap_or(self.config.monitoring_window as u64) as u32;
        let n_prev = body["n_observed"].as_u64().unwrap_or(0) as u32;

        let baseline_correction =
            body["baseline_correction_rate"].as_f64().unwrap_or(0.15) as f32;
        let baseline_stddev_correction =
            body["baseline_stddev_correction"].as_f64().unwrap_or(0.05) as f32;
        let baseline_sentiment = body["baseline_sentiment"].as_f64().unwrap_or(0.5) as f32;
        let baseline_stddev_sentiment =
            body["baseline_stddev_sentiment"].as_f64().unwrap_or(0.1) as f32;

        let prev_mean_correction =
            body["mean_correction"].as_f64().unwrap_or(baseline_correction as f64) as f32;
        let prev_m2_correction = body["m2_correction"].as_f64().unwrap_or(0.0) as f32;
        let prev_mean_sentiment =
            body["mean_sentiment"].as_f64().unwrap_or(baseline_sentiment as f64) as f32;
        let prev_m2_sentiment = body["m2_sentiment"].as_f64().unwrap_or(0.0) as f32;
        let prev_consecutive_negative =
            body["consecutive_negative"].as_u64().unwrap_or(0) as u32;

        // Welford online update for incremental mean + M2
        let n = n_prev + 1;
        let delta_c = correction_rate - prev_mean_correction;
        let mean_correction = prev_mean_correction + delta_c / n as f32;
        let m2_correction = prev_m2_correction + delta_c * (correction_rate - mean_correction);

        let delta_s = sentiment - prev_mean_sentiment;
        let mean_sentiment = prev_mean_sentiment + delta_s / n as f32;
        let m2_sentiment = prev_m2_sentiment + delta_s * (sentiment - mean_sentiment);

        let consecutive_negative =
            if obs_score < 0.4 { prev_consecutive_negative + 1 } else { 0 };

        let new_status = if n >= monitoring_window { "stable" } else { "monitoring" };

        // Link observation to deployment event for audit trail.
        self.storage.put_edge(&Edge::new(
            obs_node_id,
            deployment_node.id,
            rels::observed_with(),
            1.0,
            EdgeProvenance::AutoStructural { rule: "rollback_monitor".into() },
        ))?;

        // Persist updated stats into deployment node body.
        let new_body = serde_json::json!({
            "event_type": "deployment",
            "slug": body["slug"],
            "branch": body["branch"],
            "version": body["version"],
            "prompt_node_id": body["prompt_node_id"],
            "agent_name": body["agent_name"],
            "baseline_correction_rate": baseline_correction,
            "baseline_sentiment": baseline_sentiment,
            "baseline_stddev_correction": baseline_stddev_correction,
            "baseline_stddev_sentiment": baseline_stddev_sentiment,
            "baseline_sample_size": body["baseline_sample_size"],
            "monitoring_window": monitoring_window,
            "n_observed": n,
            "m2_correction": m2_correction,
            "mean_correction": mean_correction,
            "m2_sentiment": m2_sentiment,
            "mean_sentiment": mean_sentiment,
            "consecutive_negative": consecutive_negative,
            "status": new_status,
        });
        deployment_node.data.body = new_body.to_string();
        deployment_node.updated_at = Utc::now();
        self.storage.put_node(&deployment_node)?;

        // After monitoring_window observations with no trigger → stable, we're done.
        if n >= monitoring_window {
            return Ok(None);
        }

        // Wait for minimum samples before σ checks.
        if n < self.config.min_samples_before_check {
            return Ok(None);
        }

        // ── Trigger checks ────────────────────────────────────────────────────
        let correction_sigma = if baseline_stddev_correction > 1e-6 {
            (mean_correction - baseline_correction) / baseline_stddev_correction
        } else {
            0.0
        };
        let sentiment_sigma = if baseline_stddev_sentiment > 1e-6 {
            (baseline_sentiment - mean_sentiment) / baseline_stddev_sentiment
        } else {
            0.0
        };
        let correction_increase = mean_correction - baseline_correction;

        if consecutive_negative >= self.config.consecutive_negative_limit {
            let trigger = RollbackTrigger::ConsecutiveNegative { count: consecutive_negative };
            return self
                .execute_rollback(deployment_node, prompt_node_id, trigger, &body)
                .map(Some);
        }

        if correction_sigma > self.config.correction_rate_rollback {
            let trigger = RollbackTrigger::CorrectionRateSigma {
                sigma: correction_sigma,
                post_rate: mean_correction,
                baseline: baseline_correction,
            };
            return self
                .execute_rollback(deployment_node, prompt_node_id, trigger, &body)
                .map(Some);
        }

        if sentiment_sigma > self.config.sentiment_rollback {
            let trigger = RollbackTrigger::SentimentSigma {
                sigma: sentiment_sigma,
                post_sentiment: mean_sentiment,
                baseline: baseline_sentiment,
            };
            return self
                .execute_rollback(deployment_node, prompt_node_id, trigger, &body)
                .map(Some);
        }

        if correction_increase > self.config.absolute_correction_increase {
            let trigger =
                RollbackTrigger::AbsoluteCorrectionIncrease { increase: correction_increase };
            return self
                .execute_rollback(deployment_node, prompt_node_id, trigger, &body)
                .map(Some);
        }

        Ok(None)
    }

    /// Return the current rollback status for a prompt `slug`+`branch`.
    pub fn get_status(&self, slug: &str, branch: &str) -> Result<Option<RollbackStatus>> {
        use crate::prompt::PromptResolver;
        let resolver = PromptResolver::new(self.storage.clone());
        let head_node = match resolver.find_head(slug, branch)? {
            Some(n) => n,
            None => return Ok(None),
        };

        let current_version: u32 = serde_json::from_str::<serde_json::Value>(&head_node.data.body)
            .ok()
            .and_then(|b| b["version"].as_u64())
            .unwrap_or(1) as u32;

        let is_quarantined = head_node.data.tags.contains(&"quarantined".to_string());
        let rollback_events = self.list_rollback_events(slug, branch)?;

        // Single pass: extract both the max cooldown expiry and all summaries.
        let mut cooldown_expires_at: Option<DateTime<Utc>> = None;
        let mut recent_rollbacks: Vec<RollbackSummary> = Vec::with_capacity(rollback_events.len());

        for n in &rollback_events {
            if let Ok(body) = serde_json::from_str::<serde_json::Value>(&n.data.body) {
                // Track the latest cooldown window.
                if let Some(exp) = body["cooldown_expires_at"]
                    .as_str()
                    .and_then(|s| s.parse::<DateTime<Utc>>().ok())
                {
                    cooldown_expires_at = Some(match cooldown_expires_at {
                        Some(prev) => prev.max(exp),
                        None => exp,
                    });
                }

                recent_rollbacks.push(RollbackSummary {
                    rollback_node_id: n.id,
                    from_version: body["from_version"].as_u64().unwrap_or(0) as u32,
                    to_version: body["to_version"].as_u64().unwrap_or(0) as u32,
                    trigger: body["trigger"]["kind"]
                        .as_str()
                        .unwrap_or("unknown")
                        .to_string(),
                    rolled_back_at: n.created_at,
                    cooldown_hours: body["cooldown_hours"].as_u64().unwrap_or(1) as u32,
                });
            }
        }

        let rollback_count = recent_rollbacks.len() as u32;

        // Find active monitoring deployment for the head version.
        let deployment_rel = rels::deployed();
        let active_deployment = self
            .storage
            .edges_to(head_node.id)?
            .into_iter()
            .filter(|e| e.relation == deployment_rel)
            .filter_map(|e| self.storage.get_node(e.from).ok().flatten())
            .filter(|n| n.kind == kinds::event() && is_active_deployment(n))
            .max_by_key(|n| n.created_at)
            .and_then(|n| parse_active_deployment_info(&n));

        Ok(Some(RollbackStatus {
            slug: slug.to_string(),
            branch: branch.to_string(),
            head_node_id: head_node.id,
            current_version,
            is_quarantined,
            rollback_count,
            cooldown_expires_at,
            active_deployment,
            recent_rollbacks,
        }))
    }

    /// Manually remove the `quarantined` tag from a prompt version node.
    pub fn unquarantine(&self, prompt_node_id: NodeId) -> Result<()> {
        if let Ok(Some(mut node)) = self.storage.get_node(prompt_node_id) {
            node.data.tags.retain(|t| t != "quarantined");
            node.updated_at = Utc::now();
            self.storage.put_node(&node)?;
        }
        Ok(())
    }

    // ── Private helpers ────────────────────────────────────────────────────────

    /// True if `prompt_node_id` has an active rollback cooldown window.
    ///
    /// Uses edge traversal: finds rollback event nodes that point to this prompt
    /// via the `rolled_back` relation, then checks their `cooldown_expires_at`.
    fn is_in_cooldown(&self, prompt_node_id: NodeId) -> Result<bool> {
        let rolled_back_rel = rels::rolled_back();
        let now = Utc::now();

        let in_cooldown = self
            .storage
            .edges_to(prompt_node_id)?
            .into_iter()
            .filter(|e| e.relation == rolled_back_rel)
            .filter_map(|e| self.storage.get_node(e.from).ok().flatten())
            .any(|n| {
                serde_json::from_str::<serde_json::Value>(&n.data.body)
                    .ok()
                    .and_then(|b| {
                        b["cooldown_expires_at"]
                            .as_str()
                            .and_then(|s| s.parse::<DateTime<Utc>>().ok())
                    })
                    .map(|exp| exp > now)
                    .unwrap_or(false)
            });

        Ok(in_cooldown)
    }

    fn execute_rollback(
        &self,
        deployment_node: Node,
        prompt_node_id: NodeId,
        trigger: RollbackTrigger,
        deployment_body: &serde_json::Value,
    ) -> Result<RollbackResult> {
        let slug = deployment_body["slug"].as_str().unwrap_or("unknown");
        let branch = deployment_body["branch"].as_str().unwrap_or("main");
        let from_version = deployment_body["version"].as_u64().unwrap_or(0) as u32;
        let agent_name = deployment_body["agent_name"].as_str().unwrap_or("system");

        // Find the previous version via the `supersedes` edge.
        // Layout: new_version --supersedes--> prev_version
        let supersedes_rel = rels::supersedes();
        let prev_id = self
            .storage
            .edges_from(prompt_node_id)?
            .into_iter()
            .find(|e| e.relation == supersedes_rel)
            .map(|e| e.to);

        let prev_node = prev_id.and_then(|id| self.storage.get_node(id).ok().flatten());

        let (to_node_id, to_version) = match prev_node {
            Some(ref n) => {
                let ver: u32 = serde_json::from_str::<serde_json::Value>(&n.data.body)
                    .ok()
                    .and_then(|b| b["version"].as_u64())
                    .unwrap_or_else(|| from_version.saturating_sub(1) as u64)
                    as u32;
                (n.id, ver)
            }
            None => {
                return Err(crate::CortexError::Validation(format!(
                    "Cannot rollback {}/{} v{}: no previous version found",
                    slug, branch, from_version
                )));
            }
        };

        let existing_rollbacks = self.count_rollbacks(slug, branch)?;
        let rollback_count = existing_rollbacks + 1;

        // Cooldown doubles on each rollback, capped at 168 h (1 week).
        let cooldown_hours = (self.config.cooldown_base_hours as u64
            * (1u64 << (rollback_count - 1).min(7)))
        .min(168) as u32;
        let cooldown_expires_at = Utc::now() + Duration::hours(cooldown_hours as i64);

        let is_quarantined =
            rollback_count >= self.config.max_rollbacks_before_quarantine;

        log::warn!(
            "prompt rollback: {}/{} v{} → v{} (trigger: {}, rollback #{}, cooldown: {}h, quarantined: {})",
            slug, branch, from_version, to_version,
            trigger.kind_str(), rollback_count, cooldown_hours, is_quarantined
        );

        // Create rollback event node.
        let rollback_body = serde_json::json!({
            "event_type": "rollback",
            "slug": slug,
            "branch": branch,
            "from_version": from_version,
            "to_version": to_version,
            "from_node_id": prompt_node_id.to_string(),
            "to_node_id": to_node_id.to_string(),
            "trigger": trigger,
            "rollback_count": rollback_count,
            "cooldown_hours": cooldown_hours,
            "cooldown_expires_at": cooldown_expires_at.to_rfc3339(),
            "is_quarantined": is_quarantined,
        });

        let mut rollback_node = Node::new(
            kinds::event(),
            format!("rollback:{}/{}/v{}->v{}", slug, branch, from_version, to_version),
            rollback_body.to_string(),
            Source { agent: "rollback_monitor".to_string(), session: None, channel: None },
            1.0,
        );
        rollback_node.data.tags.push("rollback".to_string());
        self.storage.put_node(&rollback_node)?;

        // rollback --rolled_back--> from_version
        self.storage.put_edge(&Edge::new(
            rollback_node.id,
            prompt_node_id,
            rels::rolled_back(),
            1.0,
            EdgeProvenance::AutoStructural { rule: "rollback_monitor".into() },
        ))?;
        // rollback --rolled_back_to--> to_version
        self.storage.put_edge(&Edge::new(
            rollback_node.id,
            to_node_id,
            rels::rolled_back_to(),
            1.0,
            EdgeProvenance::AutoStructural { rule: "rollback_monitor".into() },
        ))?;

        // Tag the rolled-back version.
        if let Ok(Some(mut prompt_node)) = self.storage.get_node(prompt_node_id) {
            if !prompt_node.data.tags.contains(&"auto-rolled-back".to_string()) {
                prompt_node.data.tags.push("auto-rolled-back".to_string());
            }
            if is_quarantined && !prompt_node.data.tags.contains(&"quarantined".to_string()) {
                prompt_node.data.tags.push("quarantined".to_string());
            }
            prompt_node.updated_at = Utc::now();
            self.storage.put_node(&prompt_node)?;
        }

        // Update deployment event status (take by value — no clone needed).
        let mut updated_dep = deployment_node;
        if let Ok(mut dep_body) =
            serde_json::from_str::<serde_json::Value>(&updated_dep.data.body)
        {
            dep_body["status"] = serde_json::json!(
                if is_quarantined { "quarantined" } else { "rolled_back" }
            );
            updated_dep.data.body = dep_body.to_string();
        }
        updated_dep.updated_at = Utc::now();
        self.storage.put_node(&updated_dep)?;

        // Depress ALL `uses` edges from agent → rolled-back prompt version to 0.1.
        if let Some(agent_node) = self.find_agent_for_prompt(agent_name, prompt_node_id)? {
            let uses_rel = rels::uses();
            if let Ok(edges) = self.storage.edges_between(agent_node.id, prompt_node_id) {
                for mut edge in edges {
                    if edge.relation == uses_rel {
                        edge.weight = 0.1;
                        edge.updated_at = Utc::now();
                        let _ = self.storage.put_edge(&edge);
                    }
                }
            }
        }

        Ok(RollbackResult {
            rollback_node_id: rollback_node.id,
            from_node_id: prompt_node_id,
            from_version,
            to_node_id,
            to_version,
            trigger,
            cooldown_hours,
            cooldown_expires_at,
            is_quarantined,
            rollback_count,
        })
    }

    fn count_rollbacks(&self, slug: &str, branch: &str) -> Result<u32> {
        Ok(self.list_rollback_events(slug, branch)?.len() as u32)
    }

    /// List rollback event nodes for `slug`+`branch`, sorted newest-first.
    ///
    /// Uses a combined kind+tag filter so only the small set of `event` nodes
    /// tagged `"rollback"` are deserialised, rather than the entire event table.
    fn list_rollback_events(&self, slug: &str, branch: &str) -> Result<Vec<Node>> {
        let mut events: Vec<Node> = self
            .storage
            .list_nodes(
                NodeFilter::new()
                    .with_kinds(vec![kinds::event()])
                    .with_tags(vec!["rollback".to_string()]),
            )?
            .into_iter()
            .filter(|n| {
                serde_json::from_str::<serde_json::Value>(&n.data.body)
                    .map(|b| {
                        b["event_type"].as_str() == Some("rollback")
                            && b["slug"].as_str() == Some(slug)
                            && b["branch"].as_str() == Some(branch)
                    })
                    .unwrap_or(false)
            })
            .collect();
        events.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        Ok(events)
    }

    /// Find an agent node that has a `uses` edge to `prompt_node_id` and whose
    /// title matches `agent_name`. This avoids a full agent-table scan by traversing
    /// the `uses` relation backwards from the prompt node.
    fn find_agent_for_prompt(
        &self,
        agent_name: &str,
        prompt_node_id: NodeId,
    ) -> Result<Option<Node>> {
        let uses_rel = rels::uses();
        let agent_kind = kinds::agent();
        let found = self
            .storage
            .edges_to(prompt_node_id)?
            .into_iter()
            .filter(|e| e.relation == uses_rel)
            .filter_map(|e| self.storage.get_node(e.from).ok().flatten())
            .find(|n| n.kind == agent_kind && n.data.title == agent_name);
        Ok(found)
    }
}

// ── Pure helpers ───────────────────────────────────────────────────────────────

/// Return true if node body has `"event_type":"deployment"` and `"status":"monitoring"`.
fn is_active_deployment(n: &Node) -> bool {
    serde_json::from_str::<serde_json::Value>(&n.data.body)
        .map(|b| {
            b["event_type"].as_str() == Some("deployment")
                && b["status"].as_str() == Some("monitoring")
        })
        .unwrap_or(false)
}

fn parse_active_deployment_info(n: &Node) -> Option<ActiveDeploymentInfo> {
    let body = serde_json::from_str::<serde_json::Value>(&n.data.body).ok()?;
    let prompt_node_id = body["prompt_node_id"]
        .as_str()?
        .parse::<uuid::Uuid>()
        .ok()?;
    Some(ActiveDeploymentInfo {
        deployment_node_id: n.id,
        prompt_node_id,
        version: body["version"].as_u64().unwrap_or(0) as u32,
        agent_name: body["agent_name"].as_str().unwrap_or("unknown").to_string(),
        deployed_at: n.created_at,
        n_observed: body["n_observed"].as_u64().unwrap_or(0) as u32,
        monitoring_window: body["monitoring_window"].as_u64().unwrap_or(20) as u32,
        baseline_correction_rate: body["baseline_correction_rate"].as_f64().unwrap_or(0.0) as f32,
        baseline_sentiment: body["baseline_sentiment"].as_f64().unwrap_or(0.5) as f32,
        mean_correction: body["mean_correction"].as_f64().unwrap_or(0.0) as f32,
        mean_sentiment: body["mean_sentiment"].as_f64().unwrap_or(0.5) as f32,
        consecutive_negative: body["consecutive_negative"].as_u64().unwrap_or(0) as u32,
    })
}

/// Compute (mean_correction, stddev_correction, mean_sentiment, stddev_sentiment)
/// from a slice of (correction_rate, sentiment) baseline observations.
pub fn compute_baseline_stats(obs: &[(f32, f32)]) -> (f32, f32, f32, f32) {
    if obs.is_empty() {
        return (0.15, 0.05, 0.7, 0.1);
    }
    let n = obs.len() as f32;
    let mean_c = obs.iter().map(|(c, _)| c).sum::<f32>() / n;
    let mean_s = obs.iter().map(|(_, s)| s).sum::<f32>() / n;
    let var_c = obs.iter().map(|(c, _)| (c - mean_c).powi(2)).sum::<f32>() / n;
    let var_s = obs.iter().map(|(_, s)| (s - mean_s).powi(2)).sum::<f32>() / n;
    // Floor stddev at 0.01 to avoid division-by-zero in σ calculations.
    (mean_c, var_c.sqrt().max(0.01), mean_s, var_s.sqrt().max(0.01))
}

// ── Tests ──────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        prompt::{PromptContent, PromptResolver},
        storage::RedbStorage,
        types::Source,
    };
    use std::sync::Arc;
    use tempfile::TempDir;

    // ── Pure unit tests ────────────────────────────────────────────────────────

    #[test]
    fn baseline_stats_empty_returns_defaults() {
        let (mc, sc, ms, ss) = compute_baseline_stats(&[]);
        assert!((mc - 0.15).abs() < 1e-5);
        assert!((sc - 0.05).abs() < 1e-5);
        assert!((ms - 0.7).abs() < 1e-5);
        assert!((ss - 0.1).abs() < 1e-5);
    }

    #[test]
    fn baseline_stats_single_observation() {
        let (mc, sc, ms, ss) = compute_baseline_stats(&[(0.2, 0.8)]);
        assert!((mc - 0.2).abs() < 1e-5);
        assert!(sc >= 0.01, "stddev should be floored at 0.01");
        assert!((ms - 0.8).abs() < 1e-5);
        assert!(ss >= 0.01);
    }

    #[test]
    fn baseline_stats_multiple() {
        let obs: Vec<(f32, f32)> = vec![(0.1, 0.9), (0.2, 0.8), (0.3, 0.7)];
        let (mc, sc, ms, ss) = compute_baseline_stats(&obs);
        assert!((mc - 0.2).abs() < 1e-4);
        assert!(sc > 0.01);
        assert!((ms - 0.8).abs() < 1e-4);
        assert!(ss > 0.01);
    }

    #[test]
    fn rollback_config_default_thresholds() {
        let cfg = RollbackConfig::default();
        assert_eq!(cfg.monitoring_window, 20);
        assert_eq!(cfg.correction_rate_rollback, 3.0);
        assert_eq!(cfg.consecutive_negative_limit, 3);
        assert_eq!(cfg.max_rollbacks_before_quarantine, 3);
        assert_eq!(cfg.cooldown_base_hours, 1);
    }

    #[test]
    fn rollback_trigger_kind_str() {
        let t = RollbackTrigger::CorrectionRateSigma {
            sigma: 3.5,
            post_rate: 0.4,
            baseline: 0.15,
        };
        assert_eq!(t.kind_str(), "correction_rate_sigma");

        let t2 = RollbackTrigger::ConsecutiveNegative { count: 3 };
        assert_eq!(t2.kind_str(), "consecutive_negative");
    }

    #[test]
    fn cooldown_doubles_with_cap() {
        // Verify cooldown formula: base * 2^(count-1), capped at 168h.
        // With base=1 the shift is capped at 2^7=128 (never reaches 168).
        let base: u64 = 1;
        let compute = |count: u32| -> u32 {
            (base * (1u64 << (count as u64 - 1).min(7))).min(168) as u32
        };
        assert_eq!(compute(1), 1);
        assert_eq!(compute(2), 2);
        assert_eq!(compute(3), 4);
        assert_eq!(compute(8), 128);
        assert_eq!(compute(9), 128); // shift capped at 7, stays 128
        assert_eq!(compute(20), 128);

        // With base=2 the cap kicks in at count>=8 (2*128=256 > 168).
        let base2: u64 = 2;
        let compute2 = |count: u32| -> u32 {
            (base2 * (1u64 << (count as u64 - 1).min(7))).min(168) as u32
        };
        assert_eq!(compute2(1), 2);
        assert_eq!(compute2(2), 4);
        assert_eq!(compute2(7), 128);
        assert_eq!(compute2(8), 168); // 2*128=256, capped to 168
        assert_eq!(compute2(20), 168);
    }

    // ── Integration helpers ────────────────────────────────────────────────────

    fn make_storage() -> (Arc<RedbStorage>, TempDir) {
        let tmp = TempDir::new().unwrap();
        let db_path = tmp.path().join("test.redb");
        let storage = Arc::new(RedbStorage::open(&db_path).unwrap());
        (storage, tmp)
    }

    fn make_monitor(storage: Arc<RedbStorage>, cfg: RollbackConfig) -> RollbackMonitor<RedbStorage> {
        RollbackMonitor::new(storage, cfg)
    }

    /// Create a two-version prompt chain (v1 --supersedes-- v2, where v2 is HEAD).
    /// Returns (v1_node_id, v2_node_id).
    fn create_prompt_chain(storage: &Arc<RedbStorage>, slug: &str) -> (NodeId, NodeId) {
        use std::collections::HashMap;

        let resolver = PromptResolver::new(storage.clone());

        let v1_content = PromptContent {
            slug: slug.to_string(),
            prompt_type: "skill".to_string(),
            branch: "main".to_string(),
            version: 1,
            sections: HashMap::from([(
                "system".to_string(),
                serde_json::json!("You are a helpful assistant."),
            )]),
            metadata: Default::default(),
            override_sections: Default::default(),
        };
        let v1_id = resolver.create_prompt(v1_content, "main", "test").unwrap();

        let v2_content = PromptContent {
            slug: slug.to_string(),
            prompt_type: "skill".to_string(),
            branch: "main".to_string(),
            version: 2,
            sections: HashMap::from([(
                "system".to_string(),
                serde_json::json!("You are an even more helpful assistant."),
            )]),
            metadata: Default::default(),
            override_sections: Default::default(),
        };
        let v2_id = resolver.create_version(slug, "main", v2_content, "test").unwrap();

        (v1_id, v2_id)
    }

    /// Create a dummy observation node in storage and return its ID.
    fn make_obs_node(storage: &Arc<RedbStorage>) -> NodeId {
        use crate::kinds::defaults as kinds;
        let obs = Node::new(
            kinds::observation(),
            "test observation".to_string(),
            r#"{"observation_type":"performance"}"#.to_string(),
            Source { agent: "test".to_string(), session: None, channel: None },
            1.0,
        );
        storage.put_node(&obs).unwrap();
        obs.id
    }

    // ── Integration tests ──────────────────────────────────────────────────────

    #[test]
    fn record_deployment_creates_node_and_edge() {
        let (storage, _tmp) = make_storage();
        let cfg = RollbackConfig::default();
        let monitor = make_monitor(storage.clone(), cfg);

        let (_v1_id, v2_id) = create_prompt_chain(&storage, "greet");

        let dep_id = monitor
            .record_deployment("greet", "main", 2, v2_id, "kai", vec![(0.1, 0.8)])
            .unwrap();

        // Deployment node must exist.
        let dep_node = storage.get_node(dep_id).unwrap().unwrap();
        let body: serde_json::Value =
            serde_json::from_str(&dep_node.data.body).unwrap();
        assert_eq!(body["event_type"].as_str(), Some("deployment"));
        assert_eq!(body["slug"].as_str(), Some("greet"));
        assert_eq!(body["status"].as_str(), Some("monitoring"));
        assert_eq!(body["n_observed"].as_u64(), Some(0));

        // deployed edge must exist.
        let edges = storage.edges_from(dep_id).unwrap();
        assert!(
            edges.iter().any(|e| e.to == v2_id && e.relation == rels::deployed()),
            "expected deployed edge"
        );
    }

    #[test]
    fn stable_observations_do_not_trigger_rollback() {
        let (storage, _tmp) = make_storage();
        let cfg = RollbackConfig {
            monitoring_window: 10,
            min_samples_before_check: 3,
            correction_rate_rollback: 3.0,
            absolute_correction_increase: 0.25,
            sentiment_rollback: 2.0,
            consecutive_negative_limit: 5,
            ..Default::default()
        };
        let monitor = make_monitor(storage.clone(), cfg);

        let (_v1_id, v2_id) = create_prompt_chain(&storage, "stable-prompt");
        monitor
            .record_deployment("stable-prompt", "main", 2, v2_id, "kai", vec![(0.1, 0.8)])
            .unwrap();

        // Feed good observations — low correction, high sentiment.
        for _ in 0..8 {
            let obs_id = make_obs_node(&storage);
            let result =
                monitor.process_observation(obs_id, v2_id, 0.1, 0.85, 0.9).unwrap();
            assert!(result.is_none(), "good observations should not trigger rollback");
        }
    }

    #[test]
    fn consecutive_negative_triggers_rollback() {
        let (storage, _tmp) = make_storage();
        let cfg = RollbackConfig {
            monitoring_window: 20,
            min_samples_before_check: 1,
            consecutive_negative_limit: 3,
            correction_rate_rollback: 99.0, // disable sigma check
            absolute_correction_increase: 99.0,
            sentiment_rollback: 99.0,
            ..Default::default()
        };
        let monitor = make_monitor(storage.clone(), cfg);

        let (_v1_id, v2_id) = create_prompt_chain(&storage, "neg-prompt");
        monitor
            .record_deployment("neg-prompt", "main", 2, v2_id, "kai", vec![(0.1, 0.8)])
            .unwrap();

        // Two sub-threshold observations — no rollback yet.
        for _ in 0..2 {
            let obs_id = make_obs_node(&storage);
            let result =
                monitor.process_observation(obs_id, v2_id, 0.9, 0.2, 0.1).unwrap();
            assert!(result.is_none());
        }

        // Third consecutive negative — rollback fires.
        let obs_id = make_obs_node(&storage);
        let result = monitor.process_observation(obs_id, v2_id, 0.9, 0.2, 0.1).unwrap();
        assert!(result.is_some(), "third consecutive negative must trigger rollback");

        let rb = result.unwrap();
        assert!(matches!(rb.trigger, RollbackTrigger::ConsecutiveNegative { .. }));
        assert_eq!(rb.rollback_count, 1);
        assert_eq!(rb.from_node_id, v2_id);
    }

    #[test]
    fn correction_sigma_triggers_rollback() {
        let (storage, _tmp) = make_storage();
        let cfg = RollbackConfig {
            monitoring_window: 20,
            min_samples_before_check: 3,
            correction_rate_rollback: 2.0, // lower threshold for test
            absolute_correction_increase: 99.0,
            sentiment_rollback: 99.0,
            consecutive_negative_limit: 99,
            ..Default::default()
        };
        let monitor = make_monitor(storage.clone(), cfg);

        // Baseline: low correction (mean≈0.05, stddev≈0.01)
        let baseline: Vec<(f32, f32)> = (0..10).map(|_| (0.05f32, 0.8f32)).collect();
        let (_v1_id, v2_id) = create_prompt_chain(&storage, "sigma-prompt");
        monitor
            .record_deployment("sigma-prompt", "main", 2, v2_id, "kai", baseline)
            .unwrap();

        // Feed high-correction observations pushing sigma above 2.0.
        // correction=0.5, baseline≈0.05, stddev≈0.01 → sigma ≈ 45 >> 2.0
        let mut rollback_result = None;
        for _ in 0..5 {
            let obs_id = make_obs_node(&storage);
            let res = monitor
                .process_observation(obs_id, v2_id, 0.5, 0.8, 0.9)
                .unwrap();
            if res.is_some() {
                rollback_result = res;
                break;
            }
        }

        assert!(rollback_result.is_some(), "high correction sigma should trigger rollback");
        let rb = rollback_result.unwrap();
        assert!(matches!(rb.trigger, RollbackTrigger::CorrectionRateSigma { .. }));
    }

    #[test]
    fn cooldown_prevents_re_rollback() {
        let (storage, _tmp) = make_storage();
        let cfg = RollbackConfig {
            monitoring_window: 20,
            min_samples_before_check: 1,
            consecutive_negative_limit: 3,
            correction_rate_rollback: 99.0,
            absolute_correction_increase: 99.0,
            sentiment_rollback: 99.0,
            cooldown_base_hours: 24, // long cooldown
            ..Default::default()
        };
        let monitor = make_monitor(storage.clone(), cfg);

        let (_v1_id, v2_id) = create_prompt_chain(&storage, "cooldown-prompt");
        monitor
            .record_deployment("cooldown-prompt", "main", 2, v2_id, "kai", vec![(0.1, 0.8)])
            .unwrap();

        // Trigger first rollback via consecutive negatives.
        for _ in 0..3 {
            let obs_id = make_obs_node(&storage);
            monitor.process_observation(obs_id, v2_id, 0.9, 0.2, 0.1).unwrap();
        }

        // Now cooldown should be active — subsequent observations must return None.
        let obs_id = make_obs_node(&storage);
        let result = monitor.process_observation(obs_id, v2_id, 0.9, 0.2, 0.1).unwrap();
        assert!(
            result.is_none(),
            "second rollback must be suppressed by cooldown"
        );
    }

    #[test]
    fn quarantine_after_max_rollbacks() {
        let (storage, _tmp) = make_storage();
        let cfg = RollbackConfig {
            monitoring_window: 100,
            min_samples_before_check: 1,
            consecutive_negative_limit: 3,
            correction_rate_rollback: 99.0,
            absolute_correction_increase: 99.0,
            sentiment_rollback: 99.0,
            max_rollbacks_before_quarantine: 1, // quarantine on first rollback
            cooldown_base_hours: 0,             // no cooldown for this test
            ..Default::default()
        };
        let monitor = make_monitor(storage.clone(), cfg);

        let (_v1_id, v2_id) = create_prompt_chain(&storage, "quarantine-prompt");
        monitor
            .record_deployment("quarantine-prompt", "main", 2, v2_id, "kai", vec![(0.1, 0.8)])
            .unwrap();

        // Trigger rollback.
        let mut rb_result = None;
        for _ in 0..3 {
            let obs_id = make_obs_node(&storage);
            let res = monitor.process_observation(obs_id, v2_id, 0.9, 0.2, 0.1).unwrap();
            if res.is_some() {
                rb_result = res;
                break;
            }
        }

        assert!(rb_result.is_some());
        let rb = rb_result.unwrap();
        assert!(rb.is_quarantined, "rollback_count >= max should quarantine");

        // Prompt node must have 'quarantined' tag.
        let prompt_node = storage.get_node(v2_id).unwrap().unwrap();
        assert!(
            prompt_node.data.tags.contains(&"quarantined".to_string()),
            "prompt node must be tagged 'quarantined'"
        );
    }

    #[test]
    fn unquarantine_removes_tag() {
        let (storage, _tmp) = make_storage();
        let cfg = RollbackConfig {
            max_rollbacks_before_quarantine: 1,
            consecutive_negative_limit: 3,
            min_samples_before_check: 1,
            monitoring_window: 100,
            cooldown_base_hours: 0,
            correction_rate_rollback: 99.0,
            absolute_correction_increase: 99.0,
            sentiment_rollback: 99.0,
            ..Default::default()
        };
        let monitor = make_monitor(storage.clone(), cfg);
        let (_v1_id, v2_id) = create_prompt_chain(&storage, "unquarantine-prompt");
        monitor
            .record_deployment("unquarantine-prompt", "main", 2, v2_id, "kai", vec![(0.1, 0.8)])
            .unwrap();

        // Trigger quarantine.
        for _ in 0..3 {
            let obs_id = make_obs_node(&storage);
            monitor.process_observation(obs_id, v2_id, 0.9, 0.2, 0.1).unwrap();
        }

        let prompt_node = storage.get_node(v2_id).unwrap().unwrap();
        assert!(prompt_node.data.tags.contains(&"quarantined".to_string()));

        monitor.unquarantine(v2_id).unwrap();

        let prompt_node = storage.get_node(v2_id).unwrap().unwrap();
        assert!(
            !prompt_node.data.tags.contains(&"quarantined".to_string()),
            "quarantined tag must be removed after unquarantine"
        );
    }

    #[test]
    fn get_status_reflects_rollback_count_and_cooldown() {
        let (storage, _tmp) = make_storage();
        let cfg = RollbackConfig {
            monitoring_window: 20,
            min_samples_before_check: 1,
            consecutive_negative_limit: 3,
            correction_rate_rollback: 99.0,
            absolute_correction_increase: 99.0,
            sentiment_rollback: 99.0,
            cooldown_base_hours: 1,
            max_rollbacks_before_quarantine: 5,
            ..Default::default()
        };
        let monitor = make_monitor(storage.clone(), cfg);
        let (_v1_id, v2_id) = create_prompt_chain(&storage, "status-prompt");
        monitor
            .record_deployment("status-prompt", "main", 2, v2_id, "kai", vec![(0.1, 0.8)])
            .unwrap();

        // Status before any rollback.
        let status = monitor.get_status("status-prompt", "main").unwrap().unwrap();
        assert_eq!(status.rollback_count, 0);
        assert!(status.cooldown_expires_at.is_none());
        assert!(!status.is_quarantined);

        // Trigger a rollback.
        for _ in 0..3 {
            let obs_id = make_obs_node(&storage);
            monitor.process_observation(obs_id, v2_id, 0.9, 0.2, 0.1).unwrap();
        }

        // Status after rollback — must show count=1 and a cooldown window.
        let status = monitor.get_status("status-prompt", "main").unwrap().unwrap();
        assert_eq!(status.rollback_count, 1);
        assert!(
            status.cooldown_expires_at.map(|t| t > Utc::now()).unwrap_or(false),
            "cooldown_expires_at must be in the future"
        );
        assert_eq!(status.recent_rollbacks.len(), 1);
        assert_eq!(status.recent_rollbacks[0].from_version, 2);
        assert_eq!(status.recent_rollbacks[0].to_version, 1);
    }

    #[test]
    fn monitoring_window_exhausted_marks_stable() {
        let (storage, _tmp) = make_storage();
        let cfg = RollbackConfig {
            monitoring_window: 5,
            min_samples_before_check: 1,
            correction_rate_rollback: 99.0,
            absolute_correction_increase: 99.0,
            sentiment_rollback: 99.0,
            consecutive_negative_limit: 99,
            ..Default::default()
        };
        let monitor = make_monitor(storage.clone(), cfg);
        let (_v1_id, v2_id) = create_prompt_chain(&storage, "window-prompt");
        let dep_id = monitor
            .record_deployment("window-prompt", "main", 2, v2_id, "kai", vec![(0.1, 0.8)])
            .unwrap();

        // Feed 5 good observations to exhaust the window.
        for _ in 0..5 {
            let obs_id = make_obs_node(&storage);
            monitor.process_observation(obs_id, v2_id, 0.1, 0.9, 0.95).unwrap();
        }

        // Deployment node should be marked stable.
        let dep_node = storage.get_node(dep_id).unwrap().unwrap();
        let body: serde_json::Value =
            serde_json::from_str(&dep_node.data.body).unwrap();
        assert_eq!(
            body["status"].as_str(),
            Some("stable"),
            "deployment must be marked stable after monitoring window"
        );
    }

    #[test]
    fn list_rollback_events_uses_tag_filter() {
        // Verify that non-rollback events are not included in the result.
        let (storage, _tmp) = make_storage();
        let cfg = RollbackConfig::default();
        let monitor = make_monitor(storage.clone(), cfg);

        // Insert a decoy event node (no "rollback" tag).
        let decoy = Node::new(
            kinds::event(),
            "some-other-event".to_string(),
            r#"{"event_type":"deployment","slug":"x","branch":"main"}"#.to_string(),
            Source { agent: "sys".to_string(), session: None, channel: None },
            1.0,
        );
        storage.put_node(&decoy).unwrap();

        // Insert a rollback event node with the tag.
        let mut rb_event = Node::new(
            kinds::event(),
            "rollback:x/main/v2->v1".to_string(),
            r#"{"event_type":"rollback","slug":"x","branch":"main","from_version":2,"to_version":1,"trigger":{"kind":"consecutive_negative","count":3},"rollback_count":1,"cooldown_hours":1,"cooldown_expires_at":"2099-01-01T00:00:00Z","is_quarantined":false}"#.to_string(),
            Source { agent: "rollback_monitor".to_string(), session: None, channel: None },
            1.0,
        );
        rb_event.data.tags.push("rollback".to_string());
        storage.put_node(&rb_event).unwrap();

        let events = monitor.list_rollback_events("x", "main").unwrap();
        assert_eq!(events.len(), 1, "only the tagged rollback event should be returned");
        assert_eq!(events[0].id, rb_event.id);
    }
}
