//! Scalable Conway Life stress harness for the NSH native backend.
//!
//! Independent benchmark binary. Grid size, cell size and tick period come from
//! the environment so one binary can sweep load levels without recompiling.
//! It deliberately does NOT touch the 13-variant ElementSpec contract.

use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use nsh_core::prelude::*;
#[cfg(target_os = "linux")]
use nsh_platform_linux as platform;

const SEED: u32 = 0x4C49_4C59;
const DENSITY: u32 = 28;
const RESTART: usize = 240;
const DIGEST_STEPS: usize = 60;

struct Config {
    cols: usize,
    rows: usize,
    cell: f32,
    tick_us: u64,
}

fn env_or<T: std::str::FromStr>(key: &str, default: T) -> T {
    std::env::var(key)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

fn cfg() -> &'static Config {
    static C: OnceLock<Config> = OnceLock::new();
    C.get_or_init(|| Config {
        cols: env_or("LIFE_COLS", 120usize),
        rows: env_or("LIFE_ROWS", 80usize),
        cell: env_or("LIFE_CELL", 6.0f32),
        tick_us: env_or("LIFE_TICK_US", 16_667u64),
    })
}

fn config_string() -> String {
    let c = cfg();
    format!(
        "cols={},rows={},cell={},seed={},density={},restart={},digest_steps={},tick_us={}",
        c.cols, c.rows, c.cell, SEED, DENSITY, RESTART, DIGEST_STEPS, c.tick_us
    )
}

#[derive(Clone)]
struct Life {
    cols: usize,
    rows: usize,
    cell: f32,
    cells: Vec<u8>,
    next: Vec<u8>,
    rng: u32,
    generation: usize,
}

impl Life {
    fn new() -> Self {
        let c = cfg();
        let mut life = Self {
            cols: c.cols,
            rows: c.rows,
            cell: c.cell,
            cells: vec![0; c.cols * c.rows],
            next: vec![0; c.cols * c.rows],
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
        let (cols, rows) = (self.cols, self.rows);
        for y in 0..rows {
            for x in 0..cols {
                let mut neighbors = 0u8;
                for dy in -1i32..=1 {
                    for dx in -1i32..=1 {
                        if dx == 0 && dy == 0 {
                            continue;
                        }
                        let nx = x as i32 + dx;
                        let ny = y as i32 + dy;
                        if nx >= 0 && nx < cols as i32 && ny >= 0 && ny < rows as i32 {
                            neighbors += self.cells[ny as usize * cols + nx as usize];
                        }
                    }
                }
                let alive = self.cells[y * cols + x];
                self.next[y * cols + x] =
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
        s.push_str(&config_string());
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
    let c = life.cell;
    let w = life.cols as f32 * c;
    let h = life.rows as f32 * c;
    let mut painter = Painter::new()
        .fill_color(rgb(0x101820))
        .fill_rect(0.0, 0.0, w, h)
        .fill_color(rgb(0x4ade80));
    for y in 0..life.rows {
        for x in 0..life.cols {
            if life.cells[y * life.cols + x] == 1 {
                painter = painter.fill_rect(
                    x as f32 * c,
                    y as f32 * c,
                    c - 1.0,
                    c - 1.0,
                );
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
        let task = set_interval(Duration::from_micros(cfg().tick_us), move || {
            let started = Instant::now();
            life.update(|state| state.step());
            record_tick(started.elapsed().as_micros() as u32);
        });
        cleanup(move || task.cancel());
    });

    let (w, h) = {
        let c = cfg();
        (c.cols as f32 * c.cell, c.rows as f32 * c.cell)
    };
    canvas(move || {
        let started = Instant::now();
        let painter = life.with(|state| draw(state));
        record_draw(started.elapsed().as_micros() as u32);
        painter
    })
    .w(px(w))
    .h(px(h))
}

fn main() -> impl std::process::Termination {
    let bench = std::env::var_os("LILYCO_BENCH").is_some();
    env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or(if bench {
            "nsh::perf=info,nsh::fps=info"
        } else {
            "warn"
        }),
    )
    .format_timestamp_millis()
    .init();

    eprintln!("LILYCO_GAME_DIGEST {}", digest());
    eprintln!("LILYCO_GAME_CONFIG {}", config_string());
    if bench {
        eprintln!("LILYCO_BENCH_PID {}", std::process::id());
    }

    platform::set_ui_sets(vec![nsh_base::registry()]);
    platform::run_app(
        platform::AppSource::Rust(platform::Root::of::<Game>()),
        platform::AppOptions {
            title: "Lilyco Life Scale - NSH native".to_owned(),
            app_id: "com.lilyco.life.scale".to_owned(),
            size: (760, 560),
            trace: bench,
            fps: bench,
            ..Default::default()
        },
    )
}
