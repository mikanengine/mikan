//! Scene semantics through the public API: lifecycle, deferred destruction,
//! hierarchy edits, destroy-and-rerender guarantees, and the documented
//! update order.

use std::time::Duration;

use mikan_ecs::World;
use mikan_scene::{
    DespawnQueue, GlobalTransform2d, Hierarchy, HierarchyError, Scene, Transform2d,
    propagate_transforms, request_despawn,
};

fn step() -> Duration {
    Duration::from_millis(10)
}

#[test]
fn spawn_registers_a_node_and_despawn_removes_everything() {
    let mut scene = Scene::new("test");
    let parent = scene.spawn();
    let child = scene.spawn();
    scene
        .world_mut()
        .insert(parent, Transform2d::from_translation([100.0, 0.0]))
        .unwrap();
    scene
        .world_mut()
        .insert(child, Transform2d::from_translation([10.0, 0.0]))
        .unwrap();
    scene.set_parent(child, parent).unwrap();
    assert!(scene.hierarchy().contains(child));
    assert_eq!(scene.hierarchy().len(), 2);

    scene.update(step());
    let child_global = scene.world().get::<GlobalTransform2d>(child).unwrap();
    assert_eq!(child_global.translation(), [110.0, 0.0]);

    assert!(scene.despawn(parent));
    assert!(!scene.world().is_alive(parent));
    assert!(!scene.hierarchy().contains(parent));
    assert_eq!(
        scene.world().get::<GlobalTransform2d>(parent),
        None,
        "a destroyed entity keeps no derived state a renderer could submit"
    );
    assert!(
        scene.hierarchy().contains(child),
        "the child survives and was re-rooted"
    );
    assert_eq!(scene.parent(child).unwrap(), None);
    scene.propagate();
    assert_eq!(
        scene
            .world()
            .get::<GlobalTransform2d>(child)
            .unwrap()
            .translation(),
        [10.0, 0.0],
        "re-rooted children keep their local transform"
    );
    assert!(!scene.despawn(parent), "the second despawn is a no-op");
}

#[test]
fn deferred_despawns_apply_after_the_schedule() {
    let mut scene = Scene::new("test");
    let doomed = scene.spawn();
    scene
        .world_mut()
        .insert(doomed, Transform2d::from_translation([1.0, 1.0]))
        .unwrap();

    scene.schedule_mut().add("request", move |world, _dt| {
        let mut log = world
            .get_resource::<Vec<bool>>()
            .cloned()
            .unwrap_or_default();
        log.push(world.is_alive(doomed));
        world.insert_resource(log);
        request_despawn(world, doomed);
    });
    scene.schedule_mut().add("observe", move |world, _dt| {
        let mut log = world
            .get_resource::<Vec<bool>>()
            .cloned()
            .unwrap_or_default();
        log.push(world.is_alive(doomed));
        world.insert_resource(log);
    });

    scene.update(step());
    assert_eq!(
        scene.world().get_resource::<Vec<bool>>(),
        Some(&vec![true, true]),
        "the entity lives through the whole schedule"
    );
    assert!(!scene.world().is_alive(doomed), "the flush applies it");
    assert!(!scene.hierarchy().contains(doomed));
    assert!(scene.world().get_resource::<DespawnQueue>().is_none());
}

#[test]
fn despawn_queue_is_idempotent_and_flush_counts_honestly() {
    let mut world = World::new();
    let entity = world.spawn();
    request_despawn(&mut world, entity);
    request_despawn(&mut world, entity);
    request_despawn(&mut world, entity);
    assert_eq!(world.get_resource::<DespawnQueue>().unwrap().len(), 3);
    let destroyed = mikan_scene::flush_despawns(&mut world);
    assert_eq!(destroyed, 1, "the duplicate requests despawn nothing extra");
    request_despawn(&mut world, entity);
    assert_eq!(
        mikan_scene::flush_despawns(&mut world),
        0,
        "a stale request is skipped"
    );
}

#[test]
fn destroying_a_parent_reroots_children_without_rewriting_locals() {
    let mut scene = Scene::new("test");
    let parent = scene.spawn();
    let child = scene.spawn();
    scene
        .world_mut()
        .insert(parent, Transform2d::from_translation([50.0, 0.0]))
        .unwrap();
    scene
        .world_mut()
        .insert(child, Transform2d::from_translation([5.0, 2.0]))
        .unwrap();
    scene.set_parent(child, parent).unwrap();
    scene.update(step());
    assert_eq!(
        scene
            .world()
            .get::<GlobalTransform2d>(child)
            .unwrap()
            .translation(),
        [55.0, 2.0]
    );

    scene.despawn_later(parent);
    assert_eq!(
        scene.world().get::<Transform2d>(child).unwrap().translation,
        [5.0, 2.0],
        "the local transform is untouched before the flush too"
    );
    scene.update(step());
    assert_eq!(
        scene
            .world()
            .get::<GlobalTransform2d>(child)
            .unwrap()
            .translation(),
        [5.0, 2.0],
        "the child is a root now; its local transform is its world transform"
    );
}

#[test]
fn entities_spawned_through_the_world_reconcile_on_propagation() {
    let mut scene = Scene::new("test");
    let direct = scene.world_mut().spawn();
    scene
        .world_mut()
        .insert(direct, Transform2d::from_translation([7.0, 8.0]))
        .unwrap();
    assert!(!scene.hierarchy().contains(direct), "not registered yet");
    scene.update(step());
    assert!(scene.hierarchy().contains(direct), "propagation reconciles");
    assert_eq!(
        scene
            .world()
            .get::<GlobalTransform2d>(direct)
            .unwrap()
            .translation(),
        [7.0, 8.0]
    );

    let no_transform = scene.world_mut().spawn();
    scene.update(step());
    assert!(
        !scene.hierarchy().contains(no_transform),
        "an entity with no transform needs no node"
    );
    assert_eq!(scene.world().get::<GlobalTransform2d>(no_transform), None);
}

#[test]
fn removing_a_local_transform_degrades_to_the_parent_frame() {
    let mut scene = Scene::new("test");
    let parent = scene.spawn();
    let child = scene.spawn();
    scene
        .world_mut()
        .insert(parent, Transform2d::from_translation([9.0, 0.0]))
        .unwrap();
    scene
        .world_mut()
        .insert(child, Transform2d::from_translation([1.0, 0.0]))
        .unwrap();
    scene.set_parent(child, parent).unwrap();
    scene.update(step());
    assert_eq!(
        scene
            .world()
            .get::<GlobalTransform2d>(child)
            .unwrap()
            .translation(),
        [10.0, 0.0]
    );
    scene.world_mut().remove::<Transform2d>(child);
    scene.propagate();
    assert_eq!(
        scene
            .world()
            .get::<GlobalTransform2d>(child)
            .unwrap()
            .translation(),
        [9.0, 0.0],
        "identity local => the parent's frame"
    );
}

#[test]
fn systems_see_the_previous_steps_world_transforms() {
    let mut scene = Scene::new("test");
    let root = scene.spawn();
    let child = scene.spawn();
    scene
        .world_mut()
        .insert(root, Transform2d::from_translation([0.0, 0.0]))
        .unwrap();
    scene
        .world_mut()
        .insert(child, Transform2d::from_translation([10.0, 0.0]))
        .unwrap();
    scene.set_parent(child, root).unwrap();

    scene
        .schedule_mut()
        .add("move-and-record", move |world, _dt| {
            let observed = world
                .get::<GlobalTransform2d>(child)
                .copied()
                .unwrap_or_default()
                .translation()[0];
            let mut history = world
                .get_resource::<Vec<f32>>()
                .cloned()
                .unwrap_or_default();
            history.push(observed);
            world.insert_resource(history);
            if let Some(transform) = world.get_mut::<Transform2d>(root) {
                transform.translation[0] += 5.0;
            }
        });

    scene.update(step());
    scene.update(step());
    scene.update(step());

    let history = scene.world().get_resource::<Vec<f32>>().unwrap();
    assert_eq!(
        history.as_slice(),
        [0.0, 15.0, 20.0],
        "systems read the previous step's propagation, exactly as documented"
    );
    assert_eq!(
        scene
            .world()
            .get::<GlobalTransform2d>(child)
            .unwrap()
            .translation(),
        [25.0, 0.0],
        "outside the schedule, readers see the current step"
    );
}

#[test]
fn hierarchy_errors_name_the_culprit_and_change_nothing() {
    let mut scene = Scene::new("test");
    let a = scene.spawn();
    let b = scene.spawn();
    scene.set_parent(b, a).unwrap();
    assert_eq!(
        scene.set_parent(a, b),
        Err(HierarchyError::Cycle {
            child: a,
            parent: b
        })
    );
    assert_eq!(scene.parent(a).unwrap(), None);
    assert_eq!(scene.parent(b).unwrap(), Some(a));

    let stale = scene.spawn();
    scene.despawn(stale);
    assert_eq!(
        scene.set_parent(stale, a),
        Err(HierarchyError::UnknownEntity(stale))
    );
    assert_eq!(
        scene.set_parent(a, stale),
        Err(HierarchyError::UnknownEntity(stale))
    );
    assert_eq!(
        scene.parent(stale),
        Err(HierarchyError::UnknownEntity(stale))
    );
}

#[test]
fn propagate_without_a_hierarchy_leaves_a_bare_world_alone() {
    let mut world = World::new();
    let entity = world.spawn();
    world
        .insert(entity, Transform2d::from_translation([1.0, 2.0]))
        .unwrap();
    propagate_transforms(&mut world);
    assert!(world.get_resource::<Hierarchy>().is_none());
    assert_eq!(world.get::<GlobalTransform2d>(entity), None);

    world.insert_resource(Hierarchy::new());
    propagate_transforms(&mut world);
    assert!(world.get_resource::<Hierarchy>().unwrap().contains(entity));
    assert_eq!(
        world
            .get::<GlobalTransform2d>(entity)
            .unwrap()
            .translation(),
        [1.0, 2.0]
    );
}

#[test]
fn scene_debug_names_entities_and_systems() {
    let mut scene = Scene::new("debug-me");
    scene.schedule_mut().add("noop", |_world, _dt| {});
    let debug = format!("{scene:?}");
    assert!(debug.contains("debug-me"), "{debug}");
    assert!(debug.contains("noop"), "{debug}");
}

#[test]
fn hierarchy_is_a_plain_resource_over_any_world() {
    let mut world = World::new();
    let a = world.spawn();
    let b = world.spawn();
    let mut hierarchy = Hierarchy::new();
    hierarchy.add(a).unwrap();
    hierarchy.add(b).unwrap();
    hierarchy.set_parent(b, a).unwrap();
    assert_eq!(hierarchy.entities(), vec![a, b]);
    let debug = format!("{hierarchy:?}");
    assert!(debug.contains(&format!("{a:?}")), "{debug}");
    assert!(debug.contains(&format!("{b:?}")), "{debug}");
    world.insert_resource(hierarchy);
    propagate_transforms(&mut world);
    // Neither entity has a Transform2d, so each world transform is identity.
    for entity in [a, b] {
        assert_eq!(
            world
                .get::<GlobalTransform2d>(entity)
                .unwrap()
                .translation(),
            [0.0, 0.0]
        );
    }
}
