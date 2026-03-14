//! Mutation hooks -- callbacks invoked after node/edge writes.
//!
//! Register hooks on the `Cortex` struct (library mode) or via the server.
//! Hooks run synchronously in the write path -- keep them fast.

use crate::{Edge, Node};

/// What happened to a node or edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MutationAction {
    Created,
    Updated,
    Deleted,
}

/// A callback invoked after node/edge mutations.
///
/// Default implementations are no-ops, so hooks only need to implement
/// the methods they care about.
pub trait MutationHook: Send + Sync {
    /// Called after a node is created, updated, or deleted.
    fn on_node_mutation(&self, _node: &Node, _action: MutationAction) {}

    /// Called after an edge is created, updated, or deleted.
    fn on_edge_mutation(&self, _edge: &Edge, _action: MutationAction) {}
}

/// A registry that holds multiple hooks and dispatches mutations to all of them.
pub struct HookRegistry {
    hooks: Vec<std::sync::Arc<dyn MutationHook>>,
}

impl HookRegistry {
    pub fn new() -> Self {
        Self { hooks: Vec::new() }
    }

    /// Register a new mutation hook.
    pub fn add(&mut self, hook: std::sync::Arc<dyn MutationHook>) {
        self.hooks.push(hook);
    }

    /// Notify all hooks of a node mutation.
    pub fn notify_node(&self, node: &Node, action: MutationAction) {
        for hook in &self.hooks {
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                hook.on_node_mutation(node, action);
            }));
        }
    }

    /// Notify all hooks of an edge mutation.
    pub fn notify_edge(&self, edge: &Edge, action: MutationAction) {
        for hook in &self.hooks {
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                hook.on_edge_mutation(edge, action);
            }));
        }
    }

    /// Returns the number of registered hooks.
    pub fn len(&self) -> usize {
        self.hooks.len()
    }

    /// Returns true if no hooks are registered.
    pub fn is_empty(&self) -> bool {
        self.hooks.is_empty()
    }
}

impl Default for HookRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Edge, EdgeProvenance, Node, NodeKind, Relation, Source};
    use std::sync::{
        atomic::{AtomicU32, Ordering},
        Arc,
    };

    struct CountingHook {
        node_count: AtomicU32,
        edge_count: AtomicU32,
    }

    impl CountingHook {
        fn new() -> Self {
            Self {
                node_count: AtomicU32::new(0),
                edge_count: AtomicU32::new(0),
            }
        }
    }

    impl MutationHook for CountingHook {
        fn on_node_mutation(&self, _node: &Node, _action: MutationAction) {
            self.node_count.fetch_add(1, Ordering::Relaxed);
        }

        fn on_edge_mutation(&self, _edge: &Edge, _action: MutationAction) {
            self.edge_count.fetch_add(1, Ordering::Relaxed);
        }
    }

    fn make_test_node() -> Node {
        Node::new(
            NodeKind::new("fact").unwrap(),
            "Test node title here".to_string(),
            "Test node body content here for testing".to_string(),
            Source {
                agent: "test".to_string(),
                session: None,
                channel: None,
            },
            0.5,
        )
    }

    fn make_test_edge() -> Edge {
        Edge::new(
            uuid::Uuid::now_v7(),
            uuid::Uuid::now_v7(),
            Relation::new("related_to").unwrap(),
            0.8,
            EdgeProvenance::Manual {
                created_by: "test".to_string(),
            },
        )
    }

    #[test]
    fn test_hook_registration() {
        let mut registry = HookRegistry::new();
        assert!(registry.is_empty());
        assert_eq!(registry.len(), 0);

        let hook = Arc::new(CountingHook::new());
        registry.add(hook.clone());
        assert!(!registry.is_empty());
        assert_eq!(registry.len(), 1);
    }

    #[test]
    fn test_hook_called_on_node_mutation() {
        let mut registry = HookRegistry::new();
        let hook = Arc::new(CountingHook::new());
        registry.add(hook.clone());

        let node = make_test_node();
        registry.notify_node(&node, MutationAction::Created);
        assert_eq!(hook.node_count.load(Ordering::Relaxed), 1);

        registry.notify_node(&node, MutationAction::Updated);
        assert_eq!(hook.node_count.load(Ordering::Relaxed), 2);

        registry.notify_node(&node, MutationAction::Deleted);
        assert_eq!(hook.node_count.load(Ordering::Relaxed), 3);
    }

    #[test]
    fn test_hook_called_on_edge_mutation() {
        let mut registry = HookRegistry::new();
        let hook = Arc::new(CountingHook::new());
        registry.add(hook.clone());

        let edge = make_test_edge();
        registry.notify_edge(&edge, MutationAction::Created);
        assert_eq!(hook.edge_count.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn test_multiple_hooks_called_in_order() {
        let mut registry = HookRegistry::new();
        let hook1 = Arc::new(CountingHook::new());
        let hook2 = Arc::new(CountingHook::new());
        registry.add(hook1.clone());
        registry.add(hook2.clone());

        let node = make_test_node();
        registry.notify_node(&node, MutationAction::Created);

        assert_eq!(hook1.node_count.load(Ordering::Relaxed), 1);
        assert_eq!(hook2.node_count.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn test_action_type_passed_correctly() {
        use std::sync::Mutex;

        struct ActionCapture {
            actions: Mutex<Vec<MutationAction>>,
        }

        impl MutationHook for ActionCapture {
            fn on_node_mutation(&self, _node: &Node, action: MutationAction) {
                self.actions.lock().unwrap().push(action);
            }
        }

        let mut registry = HookRegistry::new();
        let hook = Arc::new(ActionCapture {
            actions: Mutex::new(Vec::new()),
        });
        registry.add(hook.clone());

        let node = make_test_node();
        registry.notify_node(&node, MutationAction::Created);
        registry.notify_node(&node, MutationAction::Updated);
        registry.notify_node(&node, MutationAction::Deleted);

        let actions = hook.actions.lock().unwrap();
        assert_eq!(actions.len(), 3);
        assert_eq!(actions[0], MutationAction::Created);
        assert_eq!(actions[1], MutationAction::Updated);
        assert_eq!(actions[2], MutationAction::Deleted);
    }

    #[test]
    fn test_cortex_store_fires_hook() {
        use crate::{Cortex, LibraryConfig};

        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("test.redb");
        let mut cortex = Cortex::open(&db_path, LibraryConfig::default()).unwrap();

        let hook = Arc::new(CountingHook::new());
        cortex.add_hook(hook.clone());

        let node = make_test_node();
        cortex.store(node).unwrap();

        assert_eq!(hook.node_count.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn test_cortex_create_edge_fires_hook() {
        use crate::{Cortex, EdgeProvenance, LibraryConfig, Relation};

        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("test.redb");
        let mut cortex = Cortex::open(&db_path, LibraryConfig::default()).unwrap();

        let hook = Arc::new(CountingHook::new());
        cortex.add_hook(hook.clone());

        let node_a = make_test_node();
        let node_b = make_test_node();
        let id_a = cortex.store(node_a).unwrap();
        let id_b = cortex.store(node_b).unwrap();

        let edge = Edge::new(
            id_a,
            id_b,
            Relation::new("related_to").unwrap(),
            0.8,
            EdgeProvenance::Manual {
                created_by: "test".to_string(),
            },
        );
        cortex.create_edge(edge).unwrap();

        // Two node stores + one edge creation
        assert_eq!(hook.node_count.load(Ordering::Relaxed), 2);
        assert_eq!(hook.edge_count.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn test_panicking_hook_isolated() {
        struct PanickingHook;

        impl MutationHook for PanickingHook {
            fn on_node_mutation(&self, _node: &Node, _action: MutationAction) {
                panic!("hook panic on purpose");
            }

            fn on_edge_mutation(&self, _edge: &Edge, _action: MutationAction) {
                panic!("hook panic on purpose");
            }
        }

        let mut registry = HookRegistry::new();
        // PanickingHook first, CountingHook second
        registry.add(Arc::new(PanickingHook));
        let counter = Arc::new(CountingHook::new());
        registry.add(counter.clone());

        let node = make_test_node();
        registry.notify_node(&node, MutationAction::Created);

        // catch_unwind isolates the panic, so CountingHook still fires
        assert_eq!(counter.node_count.load(Ordering::Relaxed), 1);

        let edge = make_test_edge();
        registry.notify_edge(&edge, MutationAction::Created);

        assert_eq!(counter.edge_count.load(Ordering::Relaxed), 1);
    }
}
