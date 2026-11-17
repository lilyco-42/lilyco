//! Conway's Game of Life stress harness for the NSH native backend.
//!
//! Independent benchmark binary: it deliberately does NOT touch the 13-variant
//! ElementSpec contract used by lilyco-native-ui::App. Both backends run the
//! same rule, the same seed and the same canvas drawing workload.

use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use nsh_core::prelude::*;
#[cfg(target_os = "linux")]
use nsh_platform_linux as platform;

const COLS: usize = 120;
const ROWS: usize = 80;
const CELL: f32 = 6.0;
const WIDTH: f32 = COLS as f32 * CELL;
const HEIGHT: f32 = ROWS as f32 * CELL;
const SEED: u32 = 0x4C49_4C59;
const DENSITY: u32 = 28;
const RESTART: usize = 240;
const DIGEST_STEPS: usize = 300;
const TICK_US: u64 = 16_667;

#[derive(Clone)]
struct Life {
    cells: Vec<u8>,
    next: Vec<u8>,
    rng: u32,
    generation: usize,
}

impl Life {
    fn new() -> Self {
        let mut life = Self {
            cells: vec![0; COLS * ROWS],
            next: vec![0; COLS * ROWS],
            rng: SEED,
            generation: 0,
        };
        life.seed();
        life
    }

    /// xorshift32; bit pattern must match the JS implementation exactly.
    fn rand(&mut self) -> u32 {
        let mut x = self.rng;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.rng = x;
        x
    }

    fn seed(&mut self) {
        self.rng = SEED;
        for i in 0..self.cells.len() {
            let v = self.rand();
            self.cells[i] = u8::from(v % 100 < DENSITY);
        }
        self.generation = 0;
    }

    /// Bounded 8-neighbour B3/S23.
    fn step(&mut self) {
        for y in 0..ROWS {
            for x in 0..COLS {
                let mut neighbors = 0u8;
                for dy in -1i32..=1 {
                    for dx in -1i32..=1 {
                        if dx == 0 && dy == 0 {
                            continue;
                        }
                        let nx = x as i32 + dx;
                        let ny = y as i32 + dy;
                        if nx >= 0 && nx < COLS as i32 && ny >= 0 && ny < ROWS as i32 {
                            neighbors += self.cells[ny as usize * COLS + nx as usize];
                        }
                    }
                }
                let alive = self.cells[y * COLS + x];
                self.next[y * COLS + x] =
                    u8::from(neighbors == 3 || (alive == 1 && neighbors == 2));
            }
        }
        std::mem::swap(&mut self.cells, &mut self.next);
        self.generation += 1;
        if self.generation % RESTART == 0 {
            self.seed();
        }
    }

    /// FNV-1a over the grid, matching Math.imul in JS.
    fn hash(&self, mut h: u32) -> u32 {
        for cell in &self.cells {
            h = (h ^ *cell as u32).wrapping_mul(0x0100_0193);
        }
        h
    }
}

fn config() -> String {
    format!(
        "cols={COLS},rows={ROWS},cell={CELL},seed={SEED},density={DENSITY},restart={RESTART},digest_steps={DIGEST_STEPS},tick_us={TICK_US}"
    )
}

fn digest() -> &'static str {
    static DIGEST: OnceLock<String> = OnceLock::new();
    DIGEST
        .get_or_init(|| {
            let mut life = Life::new();
            let mut h: u32 = 0x811c_9dc5;
            h = life.hash(h);
            for _ in 0..DIGEST_STEPS {
                life.step();
                h = life.hash(h);
            }
            format!("{h:08x}")
        })
        .as_str()
}

struct Metrics {
    started: Instant,
    last_tick: Option<Instant>,
    tick_gaps_us: Vec<u32>,
    step_us: Vec<u32>,
    draw_us: Vec<u32>,
    ticks: u64,
    draws: u64,
}

impl Metrics {
    fn new() -> Self {
        Self {
            started: Instant::now(),
            last_tick: None,
            tick_gaps_us: Vec::new(),
            step_us: Vec::new(),
            draw_us: Vec::new(),
            ticks: 0,
            draws: 0,
        }
    }

    fn snapshot(&self) -> String {
        let mut s = String::new();
        s.push_str("config=");
        s.push_str(&config());
        s.push('\n');
        s.push_str("digest=");
        s.push_str(digest());
        s.push('\n');
        s.push_str("ticks=");
        s.push_str(&self.ticks.to_string());
        s.push('\n');
        s.push_str("draws=");
        s.push_str(&self.draws.to_string());
        s.push('\n');
        s.push_str("elapsed_ms=");
        s.push_str(&self.started.elapsed().as_millis().to_string());
        s.push('\n');
        s.push_str("tick_gaps_us=");
        s.push_str(&join(&self.tick_gaps_us));
        s.push('\n');
        s.push_str("step_us=");
        s.push_str(&join(&self.step_us));
        s.push('\n');
        s.push_str("draw_us=");
        s.push_str(&join(&self.draw_us));
        s.push('\n');
        s
    }
}

fn join(values: &[u32]) -> String {
    let mut out = String::new();
    for (i, value) in values.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str(&value.to_string());
    }
    out
}

static METRICS: OnceLock<Mutex<Metrics>> = OnceLock::new();

fn metrics() -> &'static Mutex<Metrics> {
    METRICS.get_or_init(|| Mutex::new(Metrics::new()))
}

fn flush(snapshot: &str) {
    if let Ok(path) = std::env::var("LILYCO_LIFE_METRICS") {
        let tmp = format!("{path}.tmp");
        if std::fs::write(&tmp, snapshot).is_ok() {
            let _ = std::fs::rename(&tmp, &path);
        }
    }
}

fn record_tick(step_us: u32) {
    let snapshot = {
        let mut m = metrics().lock().unwrap();
        let now = Instant::now();
        if let Some(last) = m.last_tick {
            let gap = now.duration_since(last).as_micros();
            m.tick_gaps_us.push(gap.min(u128::from(u32::MAX)) as u32);
        }
        m.last_tick = Some(now);
        m.step_us.push(step_us);
        m.ticks += 1;
        if m.ticks % 60 == 0 {
            Some(m.snapshot())
        } else {
            None
        }
    };
    if let Some(s) = snapshot {
        flush(&s);
    }
}

fn record_draw(draw_us: u32) {
    let mut m = metrics().lock().unwrap();
    m.draw_us.push(draw_us);
    m.draws += 1;
}

fn draw(life: &Life) -> Painter {
    let mut painter = Painter::new()
        .fill_color(rgb(0x101820))
        .fill_rect(0.0, 0.0, WIDTH, HEIGHT)
        .fill_color(rgb(0x4ade80));
    for y in 0..ROWS {
        for x in 0..COLS {
            if life.cells[y * COLS + x] == 1 {
                painter =
                    painter.fill_rect(x as f32 * CELL, y as f32 * CELL, CELL - 1.0, CELL - 1.0);
            }
        }
    }
    painter
}

#[component]
fn Game() -> impl IntoElement {
    let life: Signal<Life> = signal_with(Life::new(), SignalEquals::Never);

    // set_interval returns a #[must_use] Spawned task: keep it inside cleanup,
    // otherwise it is cancelled the moment this function returns.
    effect(move || {
        let task = set_interval(Duration::from_micros(TICK_US), move || {
            let started = Instant::now();
            life.update(|state| state.step());
            record_tick(started.elapsed().as_micros() as u32);
        });
        cleanup(move || task.cancel());
    });

    canvas(move || {
        let started = Instant::now();
        let painter = life.with(|state| draw(state));
        record_draw(started.elapsed().as_micros() as u32);
        painter
    })
    .w(px(WIDTH))
    .h(px(HEIGHT))
}

fn main() -> impl std::process::Termination {
    let bench = std::env::var_os("LILYCO_BENCH").is_some();
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or(if bench {
        "nsh::perf=info,nsh::fps=info"
    } else {
        "warn"
    }))
    .format_timestamp_millis()
    .init();

    eprintln!("LILYCO_GAME_DIGEST {}", digest());
    eprintln!("LILYCO_GAME_CONFIG {}", config());
    if bench {
        eprintln!("LILYCO_BENCH_PID {}", std::process::id());
    }

    platform::set_ui_sets(vec![nsh_base::registry()]);
    platform::run_app(
        platform::AppSource::Rust(platform::Root::of::<Game>()),
        platform::AppOptions {
            title: "Lilyco Life - NSH native".to_owned(),
            app_id: "com.lilyco.life.native".to_owned(),
            size: (760, 560),
            trace: bench,
            fps: bench,
            ..Default::default()
        },
    )
}
