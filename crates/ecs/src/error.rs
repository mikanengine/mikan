//! The errors entity operations can report.

use thiserror::Error;

use crate::Entity;

/// A component operation targeted an entity that is not alive.
///
/// The entity was despawned, or the handle was never issued by this world
/// (for example, a handle from an older entity that used the same slot). The
/// operation changed nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
#[error("entity {entity} is not alive")]
pub struct DeadEntity {
    /// The handle that was rejected.
    pub entity: Entity,
}
