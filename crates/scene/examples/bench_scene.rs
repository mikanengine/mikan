//! Times the native scene graph against a Rust reference fold on the same
//! hierarchy.
//!
//! This is the measurement behind `docs/STATUS.md`'s scene numbers and the
//! honest baseline ADR 0012 promises: the same forest, the same local
//! transforms, warm-up plus repeated runs, release profile. The native side
//! is the real `mikan_scene_graph_propagate` call (C++ graph, C compose
//! kernel); the reference side folds `mikan_scene::reference::compose` over
//! the nodes in insertion order, which is a valid dependency order for this
//! forest. Run it through `scripts/bench_scene.sh`, or directly:
//!
//!     cargo run -p mikan-scene --release --example bench_scene [NODE_COUNT]

use std::hint::black_box;
use std::time::{Duration, Instant};

use mikan_scene::Transform2d;
use mikan_scene::reference::{bits, compose as reference_compose};
use mikan_scene_sys::{Graph, Transform2dRepr};

struct Lcg(u64);

impl Lcg {
    fn next_u32(&mut self) -> u32 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (self.0 >> 33) as u32
    }

    fn next_f32(&mut self) -> f32 {
        (self.next_u32() >> 8) as f32 / (1u32 << 24) as f32
    }
}

fn time(runs: u32, mut body: impl FnMut()) -> Duration {
    for _ in 0..3 {
        body();
    }
    let start = Instant::now();
    for _ in 0..runs {
        body();
    }
    start.elapsed() / runs
}

fn main() {
    let node_count: usize = std::env::args()
        .nth(1)
        .and_then(|argument| argument.parse().ok())
        .unwrap_or(100_000);
    let runs: u32 = 15;

    // A binary heap shape: node i's parent is i >> 1, so a node's parent
    // always has a smaller index and insertion order is a valid dependency
    // order. Depth grows logarithmically, so composed transforms stay finite.
    let mut rng = Lcg(0x0f1e_2d3c_4b5a_6978);
    let locals: Vec<Transform2dRepr> = (0..node_count)
        .map(|_| {
            Transform2d::from_parts(
                [rng.next_f32() * 4.0 - 2.0, rng.next_f32() * 4.0 - 2.0],
                rng.next_f32() * 0.02 - 0.01,
                [0.99, 1.01],
            )
            .to_repr()
        })
        .collect();
    let parents: Vec<usize> = (0..node_count).map(|index| index >> 1).collect();

    // The native hierarchy: node ids are indices + 1 (id 0 is the sentinel).
    let mut graph = Graph::new();
    for index in 0..node_count {
        graph.add(index as u64 + 1).unwrap();
    }
    for (index, &parent) in parents.iter().enumerate().skip(1) {
        graph
            .set_parent(index as u64 + 1, parent as u64 + 1)
            .unwrap();
    }

    let mut native = vec![Transform2dRepr::identity(); node_count];
    let mut rust = native.clone();

    graph.propagate(&locals, &mut native).unwrap();
    rust[0] = locals[0];
    for index in 1..node_count {
        rust[index] = reference_compose(&rust[parents[index]], &locals[index]);
    }
    assert_eq!(
        bits_of(&native),
        bits_of(&rust),
        "the graph and the reference must agree before any timing is trusted"
    );

    let native_time = time(runs, || {
        graph.propagate(black_box(&locals), &mut native).unwrap();
    });
    let rust_time = time(runs, || {
        rust[0] = locals[0];
        for index in 1..node_count {
            rust[index] = reference_compose(black_box(&rust[parents[index]]), &locals[index]);
        }
    });

    let per_node = |duration: Duration| duration.as_nanos() as f64 / node_count as f64;
    println!("nodes: {node_count} (binary heap shape), {runs} runs each");
    println!(
        "native graph:    {:.3} ms/run ({:.2} ns/node)",
        native_time.as_secs_f64() * 1e3,
        per_node(native_time)
    );
    println!(
        "rust reference:  {:.3} ms/run ({:.2} ns/node)",
        rust_time.as_secs_f64() * 1e3,
        per_node(rust_time)
    );
    println!(
        "ratio native/reference: {:.2} (below 1.0 means the native graph is faster)",
        native_time.as_secs_f64() / rust_time.as_secs_f64()
    );
}

fn bits_of(values: &[Transform2dRepr]) -> Vec<[u32; 7]> {
    values.iter().map(bits).collect()
}
