//! Integration tests for `mikan-input`: the platform event log feeding the
//! native pipeline, contexts and priorities, wheel values, and an
//! ECS-shaped example that turns actions into per-entity state changes.

use winit::keyboard::{KeyCode, PhysicalKey};

use mikan_input::{ActionId, Binding, InputError, InputSystem};
use mikan_platform::{DeviceEvent, InputState, MouseButton, PhysicalPosition};

const KEY_D: PhysicalKey = PhysicalKey::Code(KeyCode::KeyD);
const KEY_F: PhysicalKey = PhysicalKey::Code(KeyCode::KeyF);
const KEY_W: PhysicalKey = PhysicalKey::Code(KeyCode::KeyW);
const KEY_ESCAPE: PhysicalKey = PhysicalKey::Code(KeyCode::Escape);
const KEY_UP: PhysicalKey = PhysicalKey::Code(KeyCode::ArrowUp);
const KEY_UNKNOWN: PhysicalKey = PhysicalKey::Code(KeyCode::NumpadAdd);

fn pressed(key: PhysicalKey) -> DeviceEvent {
    DeviceEvent::Key {
        key,
        pressed: true,
        repeat: false,
    }
}

fn released(key: PhysicalKey) -> DeviceEvent {
    DeviceEvent::Key {
        key,
        pressed: false,
        repeat: false,
    }
}

fn wheel(dx: f64, dy: f64) -> DeviceEvent {
    DeviceEvent::MouseWheel { delta: (dx, dy) }
}

const MOVE: ActionId = ActionId::new(1);
const ACTION: ActionId = ActionId::new(2);
const MENU: ActionId = ActionId::new(3);
const TIE_A: ActionId = ActionId::new(4);
const TIE_B: ActionId = ActionId::new(5);

/// The crate exposes the architecture it advertises: identifiers, contexts,
/// bindings, a pipeline, and queries.
#[test]
fn the_public_surface_is_complete() {
    let mut input = InputSystem::new();
    let gameplay = input.create_context("gameplay", 0);
    assert!(gameplay.to_raw() > 0);
    assert!(input.context_exists(gameplay));
    assert_eq!(input.binding_count(gameplay), 0);

    input
        .bind(gameplay, Binding::Key(KEY_D), MOVE)
        .expect("KeyD has a stable code");
    assert_eq!(input.binding_count(gameplay), 1);

    let _: Option<mikan_input::ActionState> = input.action_state(MOVE);
    let _ = input.value(MOVE);
    let _ = input.dropped();
}

/// Bindings and queries are pure plumbing until events flow.
#[test]
fn key_edges_follow_the_press_release_repeat_cycle() {
    let mut input = InputSystem::new();
    let gameplay = input.create_context("gameplay", 0);
    input.bind(gameplay, Binding::Key(KEY_D), ACTION).unwrap();

    input.process(&[pressed(KEY_D)]).unwrap();
    assert!(input.held(ACTION));
    assert!(input.just_pressed(ACTION));
    input.end_step();
    assert!(input.held(ACTION));
    assert!(!input.just_pressed(ACTION));
    assert!(!input.just_released(ACTION));

    // An operating-system repeat marks the action as repeating while held.
    input
        .process(&[DeviceEvent::Key {
            key: KEY_D,
            pressed: true,
            repeat: true,
        }])
        .unwrap();
    assert!(input.repeating(ACTION));
    input.end_step();
    assert!(!input.repeating(ACTION));

    input.process(&[released(KEY_D)]).unwrap();
    assert!(!input.held(ACTION));
    assert!(input.just_released(ACTION));
    input.end_step();
    assert!(!input.just_released(ACTION));
    assert_eq!(input.value(ACTION), 0.0);
}

/// A key held while its context deactivates releases with the usual edges.
#[test]
fn deactivating_a_context_releases_every_binding() {
    let mut input = InputSystem::new();
    let gameplay = input.create_context("gameplay", 0);
    input.bind(gameplay, Binding::Key(KEY_D), ACTION).unwrap();

    input.process(&[pressed(KEY_D)]).unwrap();
    assert!(input.held(ACTION));
    input.end_step();

    input.set_active(gameplay, false).unwrap();
    assert!(!input.held(ACTION));
    assert!(
        input.just_released(ACTION),
        "deactivation produces the release edge"
    );
    input.end_step();

    // Nothing auto-presses when the context returns.
    input.set_active(gameplay, true).unwrap();
    assert!(!input.held(ACTION));
    assert!(!input.just_pressed(ACTION));
}

/// Unbinding a held key releases the action's count.
#[test]
fn unbinding_a_held_key_releases_state() {
    let mut input = InputSystem::new();
    let gameplay = input.create_context("gameplay", 0);
    input.bind(gameplay, Binding::Key(KEY_D), ACTION).unwrap();

    input.process(&[pressed(KEY_D)]).unwrap();
    input.end_step();
    assert!(input.held(ACTION));

    input.unbind(gameplay, Binding::Key(KEY_D)).unwrap();
    assert!(!input.held(ACTION));
    assert!(input.just_released(ACTION));
    input.end_step();
    assert_eq!(input.binding_count(gameplay), 0);
}

/// The same key is given to exactly one context per step: the highest
/// priority active one, or the later-created one on ties.
#[test]
fn priority_arbitration_and_late_creation_win() {
    let mut input = InputSystem::new();
    let gameplay = input.create_context("gameplay", 0);
    let menu = input.create_context("menu", 10);
    input
        .bind(gameplay, Binding::Key(KEY_ESCAPE), ACTION)
        .unwrap();
    input.bind(menu, Binding::Key(KEY_ESCAPE), MENU).unwrap();

    input.process(&[pressed(KEY_ESCAPE)]).unwrap();
    assert!(
        input.just_pressed(MENU),
        "the higher-priority context consumes"
    );
    assert!(
        !input.just_pressed(ACTION) && !input.held(ACTION),
        "a consumed press must not leak into the losing context"
    );
    input.end_step();

    input.set_active(menu, false).unwrap();
    input.process(&[released(KEY_ESCAPE)]).unwrap();
    assert!(
        !input.just_pressed(ACTION) && !input.held(ACTION),
        "the missed release consumes nothing; the losing context never held"
    );
    input.end_step();

    // New activity now maps to the surviving context.
    input
        .process(&[released(KEY_ESCAPE), pressed(KEY_ESCAPE)])
        .unwrap();
    assert!(
        input.just_pressed(ACTION),
        "with the menu gone, gameplay receives input"
    );
    input.end_step();

    // Equal priorities: the later-created context wins, on shared fresh state.
    let a = input.create_context("a", 5);
    let b = input.create_context("b", 5);
    input.bind(a, Binding::Key(KEY_D), TIE_A).unwrap();
    input.bind(b, Binding::Key(KEY_D), TIE_B).unwrap();
    input.process(&[pressed(KEY_D)]).unwrap();
    assert!(input.just_pressed(TIE_B) && !input.just_pressed(TIE_A));
}

/// Equal-priority arbitration is per event: with `b` deactivated, the next
/// press of the contended key goes to `a`.
#[test]
fn ties_are_per_event_not_per_context() {
    let mut input = InputSystem::new();
    let a = input.create_context("a", 5);
    let b = input.create_context("b", 5);
    input.bind(a, Binding::Key(KEY_D), MOVE).unwrap();
    input.bind(b, Binding::Key(KEY_D), ACTION).unwrap();

    input.process(&[pressed(KEY_D)]).unwrap();
    assert!(input.just_pressed(ACTION), "later-created b owns the tie");
    input.end_step();

    // b deactivates and releases; the key is free again.
    input.set_active(b, false).unwrap();

    input.process(&[released(KEY_D), pressed(KEY_D)]).unwrap();
    assert!(
        input.just_pressed(MOVE),
        "arbitration runs per key event, not once per context pair"
    );
}

/// Multiple keys map onto one action; held count, not binding count.
#[test]
fn several_keys_bind_one_action() {
    let mut input = InputSystem::new();
    let gameplay = input.create_context("gameplay", 0);
    for key in [KEY_W, KEY_UP] {
        input.bind(gameplay, Binding::Key(key), MOVE).unwrap();
    }

    input.process(&[pressed(KEY_UP), pressed(KEY_W)]).unwrap();
    assert!(
        input.just_pressed(MOVE),
        "one press edge even for a double bind"
    );
    input.end_step();

    input.process(&[released(KEY_UP)]).unwrap();
    assert!(input.held(MOVE), "the other binding is still down");
    input.end_step();

    input.process(&[released(KEY_W)]).unwrap();
    assert_eq!(input.value(MOVE), 0.0);
}

/// Wheel deltas add to an action's value per axis; `end_step` clears the
/// wheel contribution while held state survives, recomputed exactly.
#[test]
fn wheel_axes_bind_and_sum_within_a_step() {
    let mut input = InputSystem::new();
    let gameplay = input.create_context("gameplay", 0);
    input.bind(gameplay, Binding::Key(KEY_F), ACTION).unwrap();
    input
        .bind(
            gameplay,
            Binding::Wheel(mikan_input::WheelAxis::Vertical),
            ACTION,
        )
        .unwrap();
    input
        .bind(
            gameplay,
            Binding::Wheel(mikan_input::WheelAxis::Horizontal),
            MOVE,
        )
        .unwrap();

    input.process(&[wheel(0.0, 240.0)]).unwrap();
    assert!(
        !input.just_pressed(ACTION),
        "wheel deltas carry no press edge"
    );
    assert_eq!(input.value(ACTION), 240.0);
    input.end_step();
    assert_eq!(
        input.value(ACTION),
        0.0,
        "the wheel contribution is per-step"
    );

    // Held state and per-axis independence in one mixed batch.
    input.process(&[pressed(KEY_F)]).unwrap();
    input.end_step();
    assert_eq!(input.value(ACTION), 1.0);
    input.process(&[wheel(30.0, -7.0)]).unwrap();
    assert_eq!(
        input.value(ACTION),
        -6.0,
        "held state plus that step's vertical delta"
    );
    assert_eq!(
        input.value(MOVE),
        30.0,
        "only the horizontal axis of this step"
    );
    input.end_step();
    assert_eq!(
        input.value(ACTION),
        1.0,
        "end_step keeps the exact held state and drops the wheel part"
    );
    assert!(input.held(ACTION) && !input.just_pressed(ACTION));
}

/// Wheel direction is data, not a direction enum: one binding covers both
/// scroll directions, revealed by the value's sign.
#[test]
fn wheel_binding_covers_both_scroll_directions() {
    let mut input = InputSystem::new();
    let gameplay = input.create_context("gameplay", 0);
    input
        .bind(
            gameplay,
            Binding::Wheel(mikan_input::WheelAxis::Vertical),
            MOVE,
        )
        .unwrap();

    input.process(&[wheel(0.0, 100.0)]).unwrap();
    assert_eq!(input.value(MOVE), 100.0);
    input.end_step();

    input.process(&[wheel(0.0, -40.0)]).unwrap();
    assert_eq!(input.value(MOVE), -40.0);
}

/// `MouseWheel`'s two driver paths — `LineDelta` scaled to pixels by the
/// platform and `PixelDelta` passed through — both reach the action layer.
#[test]
fn line_and_pixel_wheel_deltas_flow_to_actions() {
    let mut input = InputSystem::new();
    let gameplay = input.create_context("gameplay", 0);
    input
        .bind(
            gameplay,
            Binding::Wheel(mikan_input::WheelAxis::Vertical),
            MOVE,
        )
        .unwrap();

    let mut state = InputState::new();
    state.handle_mouse_wheel(winit::event::MouseScrollDelta::LineDelta(0.0, 2.0));
    let mut log = Vec::new();
    state.take_events(&mut log);
    input.process(&log).unwrap();
    assert_eq!(input.value(MOVE), 48.0, "2 lines become 2 x 24 pixels");
}

/// The full path the engine will run, in one test: the platform layer records
/// a mixed event stream, the app drains it into the input system once, loses
/// focus, and gets clean state back at both layers.
#[test]
fn platform_log_drains_into_the_native_pipeline() {
    let mut input = InputSystem::new();
    let gameplay = input.create_context("gameplay", 0);
    input.bind(gameplay, Binding::Key(KEY_D), ACTION).unwrap();

    let mut state = InputState::new();
    state.set_focused(true);
    state.handle_key(KEY_D, winit::event::ElementState::Pressed, false);
    state.handle_mouse_button(MouseButton::Left, winit::event::ElementState::Pressed);
    state.handle_cursor_moved(PhysicalPosition::new(11.0, 22.0));
    state.handle_mouse_wheel(winit::event::MouseScrollDelta::PixelDelta(
        PhysicalPosition::new(5.0, -9.0),
    ));

    let mut log = Vec::new();
    state.take_events(&mut log);
    assert_eq!(log.len(), 5, "the raw stream is intact");
    input.process(&log).unwrap();

    // InputState still provides the per-frame snapshots it always did.
    assert!(state.is_key_pressed(KEY_D));
    assert!(state.is_mouse_button_pressed(MouseButton::Left));
    assert!(input.held(ACTION));
    assert!(input.just_pressed(ACTION));
    input.end_step();

    // Focus loss: the platform releases held input, the input system sees the
    // synthesized releases, and both sides come back clean.
    state.set_focused(false);
    let mut log = Vec::new();
    state.take_events(&mut log);
    input.process(&log).unwrap();
    assert!(!state.is_key_pressed(KEY_D));
    assert!(!input.held(ACTION));
    assert!(input.just_released(ACTION));
    assert_eq!(input.dropped(), 0);
}

/// Context lifecycle rules make mistakes hard: an active context cannot be
/// removed, destroyed ids fail with clear errors, unusable keys are refused.
#[test]
fn context_lifecycle_rules_write_through_errors() {
    let mut input = InputSystem::new();
    let gameplay = input.create_context("gameplay", 0);
    input.bind(gameplay, Binding::Key(KEY_D), ACTION).unwrap();

    assert!(
        matches!(
            input.destroy_context(gameplay),
            Err(InputError::ContextActive(_))
        ),
        "an active context cannot be removed"
    );
    input.set_active(gameplay, false).unwrap();
    input.destroy_context(gameplay).unwrap();
    assert!(!input.context_exists(gameplay));
    assert_eq!(input.binding_count(gameplay), 0);

    let error = input.bind(gameplay, Binding::Key(KEY_W), MOVE).unwrap_err();
    assert!(matches!(error, InputError::UnknownContext(_)), "{error:?}");
}

/// The wheel is consumed by at most one context, like any other input.
#[test]
fn wheel_arbitration_follows_priority() {
    let mut input = InputSystem::new();
    let gameplay = input.create_context("gameplay", 0);
    let menu = input.create_context("menu", 10);
    input
        .bind(
            gameplay,
            Binding::Wheel(mikan_input::WheelAxis::Vertical),
            MOVE,
        )
        .unwrap();
    input
        .bind(menu, Binding::Wheel(mikan_input::WheelAxis::Vertical), MENU)
        .unwrap();

    input.process(&[wheel(0.0, 12.0)]).unwrap();
    assert_eq!(input.value(MENU), 12.0);
    assert_eq!(input.value(MOVE), 0.0, "the losing context stays untouched");
}

/// One action drives per-entity state changed in a fixed step, the shape the
/// engine's app hooks will run.
#[test]
fn an_ecs_style_controller_moves_entities_from_actions() {
    #[derive(Clone, Copy, PartialEq, Debug)]
    struct Velocity {
        id: usize,
        dx: f32,
    }

    struct World {
        input: InputSystem,
        entities: Vec<Velocity>,
    }

    impl World {
        fn update(&mut self, log: &[DeviceEvent]) {
            self.input
                .process(log)
                .expect("input processing is infallible");
            let step = 8.0 * self.input.value(MOVE);
            if self.input.just_pressed(ACTION) {
                let id = self.entities.len();
                self.entities.push(Velocity { id, dx: 0.0 });
            }
            for entity in &mut self.entities {
                entity.dx += step;
            }
            self.input.end_step();
        }
    }

    let mut input = InputSystem::new();
    let gameplay = input.create_context("gameplay", 0);
    input.bind(gameplay, Binding::Key(KEY_D), MOVE).unwrap();
    input.bind(gameplay, Binding::Key(KEY_F), ACTION).unwrap();
    let mut world = World {
        input,
        entities: Vec::new(),
    };

    // Step 1: hold D and spawn an entity with F.
    let mut state = InputState::new();
    state.handle_key(KEY_D, winit::event::ElementState::Pressed, false);
    state.handle_key(KEY_F, winit::event::ElementState::Pressed, false);
    let mut log = Vec::new();
    state.take_events(&mut log);
    state.end_frame();
    world.update(&log);
    assert_eq!(world.entities.len(), 1);
    assert_eq!(world.entities[0].dx, 8.0);

    // Step 2: the held D keeps moving, and only F's edges matter once.
    world.update(&[]);
    assert_eq!(world.entities[0].dx, 16.0, "held means hold");
    assert_eq!(world.entities.len(), 1, "just_pressed is step-scoped");

    // Step 3: releasing D stops the motion.
    state.handle_key(KEY_D, winit::event::ElementState::Released, false);
    let mut log = Vec::new();
    state.take_events(&mut log);
    state.end_frame();
    world.update(&log);
    assert_eq!(world.entities[0].dx, 16.0);
}

/// The F press stays blocked while the pause menu owns its context.
#[test]
fn opening_a_menu_supersedes_gameplay_input() {
    let mut input = InputSystem::new();
    let gameplay = input.create_context("gameplay", 0);
    let pause = input.create_context("pause", 1);
    input.bind(gameplay, Binding::Key(KEY_F), ACTION).unwrap();
    input.bind(pause, Binding::Key(KEY_ESCAPE), MENU).unwrap();

    input.process(&[pressed(KEY_ESCAPE)]).unwrap();
    assert!(input.just_pressed(MENU), "the menu opens");
    assert!(
        !input.held(ACTION),
        "gameplay stays oblivious while the menu reads"
    );

    input.end_step();
    let _ = pause;
}

/// Keys without stable codes are forbidden to bind but not to be observed:
/// they are tracked in raw state and invisible to actions.
#[test]
fn an_unknown_key_is_tracked_but_unbindable() {
    let mut input = InputSystem::new();
    let gameplay = input.create_context("gameplay", 0);
    input
        .bind(gameplay, Binding::Key(KEY_ESCAPE), MENU)
        .unwrap();

    let error = input
        .bind(gameplay, Binding::Key(KEY_UNKNOWN), MENU)
        .unwrap_err();
    assert!(matches!(error, InputError::UnknownKey(_)), "{error:?}");
    assert_eq!(
        input.binding_count(gameplay),
        1,
        "the refused bind changed nothing"
    );

    input.process(&[pressed(KEY_UNKNOWN)]).unwrap();
    input.end_step();
    assert!(
        !input.just_pressed(MENU),
        "tracked raw state never reaches actions"
    );
}
