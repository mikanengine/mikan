# Architecture decision records

Short, numbered records of decisions that shape the repository. Each one states
the context, the decision, and the consequences that follow from it — including
the ones that are inconvenient.

| ADR | Decision | Status |
| --- | --- | --- |
| [0001](0001-rust-as-primary-language.md) | Rust is the primary implementation language | Accepted |
| [0002](0002-wgpu-for-graphics.md) | `wgpu` for rendering, `winit` for windowing | Accepted |
| [0003](0003-windowing-rendering-boundary.md) | Windowing and rendering are separate layers joined by the application | Accepted |
| [0004](0004-deferring-native-modules.md) | C and C++ modules are deferred until justified by a measured need | Accepted |
| [0005](0005-cross-platform-validation.md) | Cross-platform validation distinguishes compile-verified from runtime-verified | Accepted |
| [0006](0006-textured-sprites-and-2d-camera.md) | Textured sprites, run batching, and a minimal 2D camera; no native module in this milestone | Accepted |
| [0007](0007-engine-facade.md) | An engine facade crate for application composition | Accepted |
| [0008](0008-2d-sprite-rendering-system.md) | PNG assets, sprite-sheet metadata, and a pixel-perfect 2D view; no native module in this milestone | Accepted |
| [0009](0009-ecs-lite.md) | ECS-lite: generational entities, sparse-set components, and scheduled systems; no native module in this milestone | Accepted |
| [0010](0010-2d-collision-and-a-c-kernel.md) | 2D collision detection and resolution, with the narrow-phase kernel in C | Accepted |
| [0011](0011-luau-gameplay-scripting.md) | Gameplay scripting with embedded Luau (vendored C++ VM) | Accepted |
| [0012](0012-scene-and-entity-system.md) | Scene and entity transform hierarchy, with a C compose kernel and a hand-written C++ scene graph | Accepted |

A decision is superseded by adding a new ADR that states what it replaces; the
old record is not rewritten.
