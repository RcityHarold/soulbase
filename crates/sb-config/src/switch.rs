use parking_lot::RwLock;
use std::sync::Arc;

use crate::snapshot::ConfigSnapshot;

pub struct SnapshotSwitch {
    current: RwLock<Arc<ConfigSnapshot>>,
    lkg: Arc<ConfigSnapshot>,
}

impl SnapshotSwitch {
    pub fn new(initial: Arc<ConfigSnapshot>) -> Self {
        Self {
            current: RwLock::new(initial.clone()),
            lkg: initial,
        }
    }

    pub fn get(&self) -> Arc<ConfigSnapshot> {
        self.current.read().clone()
    }

    pub fn swap(&self, next: Arc<ConfigSnapshot>) {
        *self.current.write() = next;
    }

    pub fn rollback(&self) -> Arc<ConfigSnapshot> {
        let lkg = self.lkg.clone();
        *self.current.write() = lkg.clone();
        lkg
    }
}
