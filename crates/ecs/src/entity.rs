//! Entity handles and the allocator behind them.
//!
//! An [`Entity`] is a handle, not a thing: an index into the allocator's slot
//! array plus the generation of the entity that occupied that slot when the
//! handle was issued. Despawning increments the slot's generation, so handles
//! to dead entities are never silently reused — operations on a stale handle
//! fail (or return `None`) instead of touching a different entity.

use core::fmt;

/// A handle to one entity in a [`World`](crate::World).
///
/// Handles are cheap (`Copy`) and stable: they stay valid until the entity is
/// despawned. After a despawn, every operation rejects the handle, even after
/// the slot is reused by a new entity, because the generation no longer
/// matches.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Entity {
    index: u32,
    generation: u32,
}

impl Entity {
    /// The slot index the entity occupies.
    ///
    /// Slot indices are reused after a despawn; comparing them alone is not a
    /// valid aliveness test.
    #[must_use]
    pub const fn index(self) -> u32 {
        self.index
    }

    /// The generation of the entity that occupied the slot.
    ///
    /// Generation `0` is never issued: the first entity in a slot has
    /// generation 1, and every reuse of the slot increments it.
    #[must_use]
    pub const fn generation(self) -> u32 {
        self.generation
    }

    /// Packs the handle into one `u64`: the slot index occupies the high 32
    /// bits, the generation the low 32.
    ///
    /// This is the form non-ECS storage uses to key entities — the scene
    /// graph stores its nodes this way — because the full handle, generation
    /// included, is what keeps a stale reference from matching a reused slot.
    #[must_use]
    pub const fn to_bits(self) -> u64 {
        ((self.index as u64) << 32) | self.generation as u64
    }

    /// Unpacks a handle from [`Entity::to_bits`].
    ///
    /// Returns `None` when the generation is zero, which no issued handle has:
    /// such bits can only be hand-built, and refusing them preserves "every
    /// `Entity` is a handle an allocator could have issued". The result is a
    /// handle, not a liveness promise — an entity can be despawned after its
    /// bits are taken.
    #[must_use]
    pub const fn from_bits(bits: u64) -> Option<Self> {
        let generation = bits as u32;
        if generation == 0 {
            return None;
        }
        Some(Self {
            index: (bits >> 32) as u32,
            generation,
        })
    }
}

impl fmt::Display for Entity {
    /// `index`v`generation`, the conventional spelling in logs and errors.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}v{}", self.index, self.generation)
    }
}

/// The index/generation allocator behind [`World`](crate::World).
///
/// Slot reuse is LIFO: the most recently freed slot is handed out first, which
/// keeps the slot array dense and makes reuse order easy to reason about.
#[derive(Debug, Default)]
pub(crate) struct Entities {
    /// Generation of each slot; 0 means "never spawned". Indexed by slot.
    generations: Vec<u32>,
    /// Whether each slot currently holds a live entity. Indexed by slot.
    alive: Vec<bool>,
    /// Slots whose entity was despawned, ready for reuse, most recent last.
    free: Vec<u32>,
    /// Number of live entities.
    count: usize,
}

impl Entities {
    /// Allocates a slot and returns a handle to the new entity.
    pub(crate) fn spawn(&mut self) -> Entity {
        match self.free.pop() {
            Some(index) => {
                let generation = self.generations[index as usize].wrapping_add(1);
                self.generations[index as usize] = generation;
                self.alive[index as usize] = true;
                self.count += 1;
                Entity { index, generation }
            }
            None => {
                // The slot array itself grows: index fits in u32 because a
                // process cannot hold four billion live entities.
                assert!(
                    self.generations.len() < u32::MAX as usize,
                    "entity index space exhausted"
                );
                let index = self.generations.len() as u32;
                self.generations.push(1);
                self.alive.push(true);
                self.count += 1;
                Entity {
                    index,
                    generation: 1,
                }
            }
        }
    }

    /// Despawns the entity, returning whether it was alive.
    ///
    /// A `false` return means the handle was stale or the entity was already
    /// despawned; the world is left untouched.
    pub(crate) fn despawn(&mut self, entity: Entity) -> bool {
        if !self.is_alive(entity) {
            return false;
        }
        self.alive[entity.index as usize] = false;
        self.free.push(entity.index);
        self.count -= 1;
        true
    }

    /// Whether `entity` identifies a live entity right now.
    pub(crate) fn is_alive(&self, entity: Entity) -> bool {
        let index = entity.index as usize;
        self.alive.get(index).copied().unwrap_or(false)
            && self.generations[index] == entity.generation
    }

    /// Number of live entities.
    pub(crate) fn len(&self) -> usize {
        self.count
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spawn_issues_distinct_handles_and_counts_alive() {
        let mut entities = Entities::default();
        let first = entities.spawn();
        let second = entities.spawn();
        assert_ne!(first, second);
        assert_eq!(entities.len(), 2);
        assert!(entities.is_alive(first) && entities.is_alive(second));
        assert_eq!(first.generation(), 1, "first use of a slot is generation 1");
    }

    #[test]
    fn despawn_invalidates_the_handle_and_counts_down() {
        let mut entities = Entities::default();
        let entity = entities.spawn();
        assert!(entities.despawn(entity));
        assert!(!entities.is_alive(entity));
        assert_eq!(entities.len(), 0);
    }

    #[test]
    fn despawn_reports_whether_the_entity_was_alive() {
        let mut entities = Entities::default();
        let entity = entities.spawn();
        assert!(entities.despawn(entity));
        assert!(
            !entities.despawn(entity),
            "a second despawn must not count down twice"
        );
        assert_eq!(entities.len(), 0);
    }

    #[test]
    fn respawn_reuses_the_slot_with_a_new_generation() {
        let mut entities = Entities::default();
        let first = entities.spawn();
        entities.despawn(first);
        let second = entities.spawn();
        assert_eq!(second.index(), first.index(), "the slot is reused");
        assert_eq!(
            second.generation(),
            first.generation() + 1,
            "reuse bumps the generation"
        );
        assert!(!entities.is_alive(first), "the old handle stays dead");
        assert!(entities.is_alive(second));
    }

    #[test]
    fn generations_keep_advancing_across_repeated_reuse() {
        let mut entities = Entities::default();
        let mut seen = Vec::new();
        for _ in 0..8 {
            let entity = entities.spawn();
            seen.push(entity.generation());
            entities.despawn(entity);
        }
        let mut sorted = seen.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), seen.len(), "no generation is ever reissued");
    }

    #[test]
    fn free_slots_are_handed_out_most_recently_freed_first() {
        let mut entities = Entities::default();
        let first = entities.spawn();
        let second = entities.spawn();
        entities.despawn(first);
        entities.despawn(second);
        // `second` was freed last, so its slot is reused first.
        assert_eq!(entities.spawn().index(), second.index());
        assert_eq!(entities.spawn().index(), first.index());
    }

    #[test]
    fn an_out_of_range_handle_is_never_alive() {
        let entities = Entities::default();
        // Generation 0 is never issued; index 0 has no slot in an empty
        // allocator, so this exercises the bounds check as well.
        assert!(!entities.is_alive(Entity {
            index: 0,
            generation: 0,
        }));
        assert!(!entities.is_alive(Entity {
            index: 99,
            generation: 1,
        }));
    }

    #[test]
    fn bits_round_trip_and_keep_generations_distinct() {
        let mut entities = Entities::default();
        let first = entities.spawn();
        entities.despawn(first);
        let second = entities.spawn();
        assert_eq!(Entity::from_bits(first.to_bits()), Some(first));
        assert_eq!(Entity::from_bits(second.to_bits()), Some(second));
        assert_ne!(
            first.to_bits(),
            second.to_bits(),
            "a reused slot packs a different generation"
        );
    }

    #[test]
    fn bits_refuse_generation_zero() {
        assert_eq!(Entity::from_bits(0), None);
        assert_eq!(Entity::from_bits(1_u64 << 32), None, "index only, gen 0");
        assert_eq!(
            Entity::from_bits((7_u64 << 32) | 1).map(|e| (e.index(), e.generation())),
            Some((7, 1))
        );
    }
}
