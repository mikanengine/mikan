//! The `--actions` input path: the demo's controls as actions and contexts.
//!
//! This module demonstrates Phase 8's action-mapping pipeline against the same
//! controls the classic path reads straight off [`InputState`]: WASD, the
//! arrows, F (despawn), Escape (quit), and the vertical wheel. Gameplay still
//! runs through the Luau script; only the *front door* changes — instead of
//! asking the platform about keys, [`Demo`][`crate::demo::Demo`] asks about
//! actions, so remapping a key is a one-line `bind` change here.
//!
//! Layout:
//!
//! - the `global` context (priority 10) owns Escape: quitting must win even
//!   though `gameplay` also reads Escape? It does not — the demo binds Escape
//!   only here, but the priority shows the mechanism: any future gameplay
//!   binding on the same key loses to `global`.
//! - the `gameplay` context (priority 0) owns movement, the F key, and the
//!   vertical wheel, all bound to one [`MOVE`] per keyset and one [`SCROLL_Y`]
//!   for the wheel.

use mikan::{
    ActionId, Binding, ContextId, InputError, InputSystem, KeyCode, PhysicalKey, WheelAxis,
};

/// quit the demo ([`InputSystem::just_pressed`]-scoped).
pub const QUIT: ActionId = ActionId::new(1);
/// move left: KeyA or ArrowLeft.
pub const MOVE_LEFT: ActionId = ActionId::new(2);
/// move right: KeyD or ArrowRight.
pub const MOVE_RIGHT: ActionId = ActionId::new(3);
/// move up: KeyW or ArrowUp.
pub const MOVE_UP: ActionId = ActionId::new(4);
/// move down: KeyS or ArrowDown.
pub const MOVE_DOWN: ActionId = ActionId::new(5);
/// despawn the moon (hierarchy), KeyF, press-scoped.
pub const DESPAWN: ActionId = ActionId::new(6);
/// vertical wheel scroll, value-carrying.
pub const SCROLL_Y: ActionId = ActionId::new(7);

fn key(code: KeyCode) -> Binding {
    Binding::Key(PhysicalKey::Code(code))
}

/// The demo's action mapping over [`mikan_input::InputSystem`].
#[derive(Debug)]
pub struct ActionMapping {
    system: InputSystem,
    global: ContextId,
    gameplay: ContextId,
}

impl ActionMapping {
    /// Creates the contexts and binds the demo's controls.
    ///
    /// [`Binding::Button`] with the primary button is bound to [`DESPAWN`]
    /// only for demonstration of the mouse-button lane; the demo does not
    /// make it do anything yet.
    ///
    /// # Errors
    ///
    /// Returns [`InputError`] if the native module rejects a binding; with
    /// this fixed set, it cannot.
    pub fn new() -> Result<Self, InputError> {
        let mut system = InputSystem::new();
        let global = system.create_context("global", 10);
        let gameplay = system.create_context("gameplay", 0);
        system.bind(global, key(KeyCode::Escape), QUIT)?;
        system.bind(gameplay, key(KeyCode::KeyA), MOVE_LEFT)?;
        system.bind(gameplay, key(KeyCode::ArrowLeft), MOVE_LEFT)?;
        system.bind(gameplay, key(KeyCode::KeyD), MOVE_RIGHT)?;
        system.bind(gameplay, key(KeyCode::ArrowRight), MOVE_RIGHT)?;
        system.bind(gameplay, key(KeyCode::KeyW), MOVE_UP)?;
        system.bind(gameplay, key(KeyCode::ArrowUp), MOVE_UP)?;
        system.bind(gameplay, key(KeyCode::KeyS), MOVE_DOWN)?;
        system.bind(gameplay, key(KeyCode::ArrowDown), MOVE_DOWN)?;
        system.bind(gameplay, key(KeyCode::KeyF), DESPAWN)?;
        system.bind(gameplay, Binding::Wheel(WheelAxis::Vertical), SCROLL_Y)?;

        Ok(Self {
            system,
            global,
            gameplay,
        })
    }

    /// Processes the platform's recorded device events for this step.
    ///
    /// # Errors
    ///
    /// See [`InputSystem::process`].
    pub fn process(&mut self, events: &[mikan::DeviceEvent]) -> Result<(), InputError> {
        self.system.process(events)
    }

    /// Whether the quit action fired this step.
    #[must_use]
    pub fn quit_pressed(&self) -> bool {
        self.system.just_pressed(QUIT)
    }

    /// Whether the despawn action fired this step.
    #[must_use]
    pub fn despawn_pressed(&self) -> bool {
        self.system.just_pressed(DESPAWN)
    }

    /// The four movement directions as `(left, right, up, down)` booleans,
    /// shaped exactly like the raw-key readout the classic path produces.
    #[must_use]
    pub fn movement(&self) -> (bool, bool, bool, bool) {
        let system = &self.system;
        (
            system.held(MOVE_LEFT),
            system.held(MOVE_RIGHT),
            system.held(MOVE_UP),
            system.held(MOVE_DOWN),
        )
    }

    /// The vertical wheel's value for this step, in pixels.
    #[must_use]
    pub fn scroll_y(&self) -> f32 {
        self.system.value(SCROLL_Y)
    }

    /// Clears the step-scoped action edges.
    pub fn end_step(&mut self) {
        self.system.end_step();
    }

    /// Context ids in creation order, for logs.
    #[must_use]
    pub fn contexts(&self) -> [ContextId; 2] {
        [self.global, self.gameplay]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mikan::DeviceEvent;

    fn press(code: KeyCode) -> DeviceEvent {
        DeviceEvent::Key {
            key: PhysicalKey::Code(code),
            pressed: true,
            repeat: false,
        }
    }

    fn release(code: KeyCode) -> DeviceEvent {
        DeviceEvent::Key {
            key: PhysicalKey::Code(code),
            pressed: false,
            repeat: false,
        }
    }

    #[test]
    fn the_controls_map_onto_actions() {
        let mut mapping = ActionMapping::new().expect("the fixed set binds");
        let [global, gameplay] = mapping.contexts();
        assert_ne!(global.to_raw(), gameplay.to_raw());
        assert!(
            global.to_raw() < gameplay.to_raw(),
            "contexts get increasing ids in creation order; the priority is what wins"
        );

        mapping
            .process(&[press(KeyCode::KeyD), press(KeyCode::ArrowLeft)])
            .expect("a small batch");
        assert_eq!(
            mapping.movement(),
            (true, true, false, false),
            "both bindings are held at once"
        );

        mapping.end_step();
        mapping
            .process(&[release(KeyCode::KeyD)])
            .expect("a small batch");
        assert_eq!(mapping.movement(), (true, false, false, false));
        mapping.end_step();
        mapping
            .process(&[release(KeyCode::ArrowLeft)])
            .expect("a small batch");
        assert_eq!(mapping.movement(), (false, false, false, false));
    }

    #[test]
    fn quit_and_despawn_use_press_edges() {
        let mut mapping = ActionMapping::new().expect("the fixed set binds");
        assert!(!mapping.quit_pressed());
        assert!(!mapping.despawn_pressed());

        mapping
            .process(&[press(KeyCode::KeyF), press(KeyCode::Escape)])
            .expect("a small batch");
        assert!(mapping.quit_pressed());
        assert!(mapping.despawn_pressed());

        mapping.end_step();
        assert!(!mapping.quit_pressed(), "press edges are step-scoped");
        assert!(!mapping.despawn_pressed());
    }

    #[test]
    fn the_wheel_arrives_as_a_value() {
        let mut mapping = ActionMapping::new().expect("the fixed set binds");
        mapping
            .process(&[DeviceEvent::MouseWheel { delta: (0.0, 24.0) }])
            .expect("a small batch");
        assert_eq!(mapping.scroll_y(), 24.0);
        mapping.end_step();
        assert_eq!(mapping.scroll_y(), 0.0, "the wheel value is per-step");
    }
}
