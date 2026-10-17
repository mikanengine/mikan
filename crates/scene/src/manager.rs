//! Owning many scenes: the [`SceneManager`].
//!
//! A manager holds scenes in slots, hands out generational [`SceneId`]s, and
//! runs exactly one scene as the *active* scene per [`SceneManager::update`].
//! Switching scenes is an id operation, not a destruction: a deactivated
//! scene keeps its world, its systems, and its state until it is activated
//! again or removed.

use core::fmt;

use thiserror::Error;

use crate::Scene;

/// A handle to one scene in a [`SceneManager`].
///
/// Generational, like [`Entity`](mikan_ecs::Entity): removing a scene
/// invalidates its id even after the slot is reused by a new scene.
/// Generation `0` is never issued.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SceneId {
    index: u32,
    generation: u32,
}

impl SceneId {
    /// The slot the scene occupies; slots are reused after removal.
    #[must_use]
    pub const fn index(self) -> u32 {
        self.index
    }

    /// The generation of the scene occupying the slot.
    #[must_use]
    pub const fn generation(self) -> u32 {
        self.generation
    }
}

impl fmt::Display for SceneId {
    /// `index`v`generation`, the same spelling entity handles use.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}v{}", self.index, self.generation)
    }
}

/// A scene operation named an id the manager does not know.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
#[error("scene {0} is not in the manager")]
pub struct SceneError(SceneId);

impl SceneError {
    /// The id that was rejected.
    #[must_use]
    pub const fn id(self) -> SceneId {
        self.0
    }
}

struct Slot {
    generation: u32,
    scene: Option<Scene>,
}

/// A collection of scenes with exactly one active scene.
///
/// ```
/// use std::time::Duration;
///
/// use mikan_scene::{Scene, SceneManager};
///
/// let mut manager = SceneManager::new();
/// let menu = manager.insert(Scene::new("menu"));
/// let game = manager.insert(Scene::new("game"));
///
/// manager.activate(game)?;
/// manager.update(Duration::from_millis(16)); // runs "game" only
/// assert_eq!(manager.active().map(|id| id.index()), Some(1));
///
/// let removed = manager.remove(menu).expect("menu is present");
/// assert_eq!(removed.name(), "menu");
/// # Ok::<(), mikan_scene::SceneError>(())
/// ```
#[derive(Default)]
pub struct SceneManager {
    slots: Vec<Slot>,
    free: Vec<u32>,
    active: Option<SceneId>,
    len: usize,
}

impl SceneManager {
    /// Creates a manager with no scenes.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Inserts a fully built scene, returning its id.
    ///
    /// The new scene is not activated: the previously active scene (if any)
    /// keeps running.
    pub fn insert(&mut self, scene: Scene) -> SceneId {
        self.len += 1;
        if let Some(index) = self.free.pop() {
            let slot = &mut self.slots[index as usize];
            slot.generation += 1;
            slot.scene = Some(scene);
            SceneId {
                index,
                generation: slot.generation,
            }
        } else {
            let index =
                u32::try_from(self.slots.len()).expect("a process cannot hold four billion scenes");
            self.slots.push(Slot {
                generation: 1,
                scene: Some(scene),
            });
            SceneId {
                index,
                generation: 1,
            }
        }
    }

    /// Whether `id` names a scene in this manager.
    #[must_use]
    pub fn contains(&self, id: SceneId) -> bool {
        self.slot(id).is_some()
    }

    /// The scene named by `id`.
    #[must_use]
    pub fn get(&self, id: SceneId) -> Option<&Scene> {
        self.slot(id)?.scene.as_ref()
    }

    /// The scene named by `id`, mutably.
    #[must_use]
    pub fn get_mut(&mut self, id: SceneId) -> Option<&mut Scene> {
        self.slot_mut(id)?.scene.as_mut()
    }

    /// The id of the active scene, if one is active.
    #[must_use]
    pub const fn active(&self) -> Option<SceneId> {
        self.active
    }

    /// The active scene, if one is active.
    #[must_use]
    pub fn active_scene(&self) -> Option<&Scene> {
        self.active.and_then(|id| self.get(id))
    }

    /// The active scene, if one is active, mutably.
    #[must_use]
    pub fn active_scene_mut(&mut self) -> Option<&mut Scene> {
        self.active.and_then(|id| self.get_mut(id))
    }

    /// Makes `id` the active scene; activation is exclusive.
    ///
    /// Activating the already-active scene is a no-op that succeeds. On
    /// failure nothing changes, in particular the previously active scene
    /// stays active.
    ///
    /// # Errors
    ///
    /// [`SceneError`] if `id` does not name a scene in this manager.
    pub fn activate(&mut self, id: SceneId) -> Result<(), SceneError> {
        if !self.contains(id) {
            return Err(SceneError(id));
        }
        self.active = Some(id);
        Ok(())
    }

    /// Stops running the active scene; every scene stays inserted.
    pub fn deactivate(&mut self) {
        self.active = None;
    }

    /// Advances the active scene by one fixed step; returns whether a scene
    /// ran.
    ///
    /// Inactive scenes do not advance: no systems run, no despawns flush, no
    /// transforms propagate.
    pub fn update(&mut self, dt: core::time::Duration) -> bool {
        match self.active_scene_mut() {
            Some(scene) => {
                scene.update(dt);
                true
            }
            None => false,
        }
    }

    /// Removes and returns the scene named by `id`, deactivating it first if
    /// it was active.
    ///
    /// The id becomes stale; reusing its slot later issues a new generation.
    pub fn remove(&mut self, id: SceneId) -> Option<Scene> {
        if self.active == Some(id) {
            self.active = None;
        }
        let scene = self.slot_mut(id)?.scene.take()?;
        self.free.push(id.index);
        self.len -= 1;
        Some(scene)
    }

    /// Replaces the scene named by `id` in place, returning the old scene.
    ///
    /// The id survives the replacement and stays active if it was active:
    /// this is a cheap way to restart a scene with a freshly built one.
    ///
    /// # Errors
    ///
    /// [`SceneError`] if `id` does not name a scene in this manager.
    pub fn replace(&mut self, id: SceneId, scene: Scene) -> Result<Scene, SceneError> {
        let slot = self.slot_mut(id).ok_or(SceneError(id))?;
        let previous = slot
            .scene
            .replace(scene)
            .expect("a valid slot always holds a scene");
        Ok(previous)
    }

    /// Number of inserted scenes.
    #[must_use]
    pub fn len(&self) -> usize {
        self.len
    }

    /// Whether no scene is inserted.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Every inserted scene and its id, in slot order.
    pub fn iter(&self) -> impl Iterator<Item = (SceneId, &Scene)> {
        self.slots.iter().enumerate().filter_map(|(index, slot)| {
            slot.scene.as_ref().map(|scene| {
                (
                    SceneId {
                        index: index as u32,
                        generation: slot.generation,
                    },
                    scene,
                )
            })
        })
    }

    fn slot(&self, id: SceneId) -> Option<&Slot> {
        let slot = self.slots.get(id.index as usize)?;
        (slot.generation == id.generation && slot.scene.is_some()).then_some(slot)
    }

    fn slot_mut(&mut self, id: SceneId) -> Option<&mut Slot> {
        let slot = self.slots.get_mut(id.index as usize)?;
        (slot.generation == id.generation && slot.scene.is_some()).then_some(slot)
    }
}

impl fmt::Debug for SceneManager {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SceneManager")
            .field("active", &self.active)
            .field(
                "scenes",
                &self
                    .iter()
                    .map(|(id, scene)| (id, scene.name()))
                    .collect::<Vec<_>>(),
            )
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_remove_and_stale_ids() {
        let mut manager = SceneManager::new();
        let first = manager.insert(Scene::new("first"));
        let second = manager.insert(Scene::new("second"));
        assert_eq!(manager.len(), 2);
        assert!(manager.contains(first) && manager.contains(second));
        assert_eq!(manager.get(second).map(Scene::name), Some("second"));

        let removed = manager.remove(first).expect("first is present");
        assert_eq!(removed.name(), "first");
        assert!(!manager.contains(first), "the id is stale");
        assert!(manager.get(first).is_none());
        assert!(manager.remove(first).is_none());
        assert_eq!(manager.len(), 1);

        let recycled = manager.insert(Scene::new("recycled"));
        assert_eq!(recycled.index(), first.index(), "the slot is reused");
        assert_ne!(recycled.generation(), first.generation());
        assert!(!manager.contains(first), "the old id stays stale");
    }

    #[test]
    fn activation_is_exclusive_and_failure_changes_nothing() {
        let mut manager = SceneManager::new();
        let first = manager.insert(Scene::new("first"));
        let second = manager.insert(Scene::new("second"));
        manager.activate(first).unwrap();
        assert_eq!(manager.active(), Some(first));

        let removed = manager.remove(second);
        assert!(removed.is_some());
        let error = manager.activate(second).unwrap_err();
        assert_eq!(error.id(), second);
        assert_eq!(manager.active(), Some(first), "unchanged on failure");
        manager.deactivate();
        assert_eq!(manager.active(), None);
    }

    #[test]
    fn update_runs_only_the_active_scene() {
        use core::time::Duration;
        let mut manager = SceneManager::new();
        let idle = manager.insert(Scene::new("idle"));
        let running = manager.insert(Scene::new("running"));
        manager
            .get_mut(running)
            .unwrap()
            .schedule_mut()
            .add("count", |world, _dt| {
                let count = world.get_resource::<u32>().copied().unwrap_or(0);
                world.insert_resource(count + 1);
            });
        assert!(!manager.update(Duration::ZERO), "nothing is active");
        manager.activate(running).unwrap();
        assert!(manager.update(Duration::ZERO));
        assert_eq!(
            manager.get(running).unwrap().world().get_resource::<u32>(),
            Some(&1)
        );
        assert_eq!(
            manager.get(idle).unwrap().world().get_resource::<u32>(),
            None,
            "idle never ran"
        );
        assert!(manager.remove(idle).is_some());
        assert!(manager.update(Duration::ZERO), "running is still active");
        assert_eq!(
            manager.get(running).unwrap().world().get_resource::<u32>(),
            Some(&2)
        );
    }

    #[test]
    fn replace_keeps_the_id_and_activation() {
        let mut manager = SceneManager::new();
        let id = manager.insert(Scene::new("old"));
        manager.activate(id).unwrap();
        let previous = manager.replace(id, Scene::new("new")).unwrap();
        assert_eq!(previous.name(), "old");
        assert_eq!(manager.get(id).map(Scene::name), Some("new"));
        assert_eq!(manager.active(), Some(id));
        let ghost = manager.insert(Scene::new("ghost"));
        manager.remove(ghost).expect("just inserted");
        assert_eq!(
            manager.replace(ghost, Scene::new("x")).unwrap_err().id(),
            ghost
        );
    }

    #[test]
    fn removing_the_active_scene_deactivates_it() {
        let mut manager = SceneManager::new();
        let id = manager.insert(Scene::new("only"));
        manager.activate(id).unwrap();
        manager.remove(id);
        assert_eq!(manager.active(), None);
        assert!(manager.active_scene().is_none());
        assert!(!manager.update(core::time::Duration::ZERO));
    }

    #[test]
    fn iter_reports_slot_order() {
        let mut manager = SceneManager::new();
        let a = manager.insert(Scene::new("a"));
        let b = manager.insert(Scene::new("b"));
        let ids: Vec<SceneId> = manager.iter().map(|(id, _)| id).collect();
        let names: Vec<&str> = manager.iter().map(|(_, scene)| scene.name()).collect();
        assert_eq!(ids, [a, b]);
        assert_eq!(names, ["a", "b"]);
    }
}
