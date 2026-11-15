//! How far the opening of a model has got, for the app to show while it waits.
//!
//! The core gives no progress of its own: opening is three calls (read and recognise the
//! model, build the session, draw the sheet), and each runs to its end. So the app shows
//! which of these is running, what the model holds once it has been read, and a bar that
//! estimates the stage's share done from how long that stage took per unit on this phone
//! before (learnt, kept in the library folder).
//!
//! The engine thread writes the state; the app reads it from another thread while the open
//! call waits, so it lives behind its own lock and never behind the engine thread.

use std::path::Path;
use std::sync::Mutex;
use std::time::Instant;

use draftwright::haecceity::geom::Surface;
use serde::{Deserialize, Serialize};

/// The stages of an open, in order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    Reading,
    Building,
    Drawing,
}

impl Stage {
    fn number(self) -> u32 {
        match self {
            Stage::Reading => 1,
            Stage::Building => 2,
            Stage::Drawing => 3,
        }
    }
    fn label(self) -> &'static str {
        match self {
            Stage::Reading => "Reading the model",
            Stage::Building => "Projecting the views and laying out the sheet",
            Stage::Drawing => "Drawing the sheet",
        }
    }
}

/// What the faces of a model are, by surface.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Faces {
    pub total: usize,
    pub planar: usize,
    pub cylindrical: usize,
    pub other: usize,
}

impl Faces {
    pub fn of(part: &draftwright::haecceity::Part) -> Self {
        let mut f = Faces { total: part.faces.len(), ..Faces::default() };
        for face in &part.faces {
            match face.surface {
                Surface::Plane { .. } => f.planar += 1,
                Surface::Cylinder { .. } => f.cylindrical += 1,
                _ => f.other += 1,
            }
        }
        f
    }
}

/// The open in progress, as the app shows it.
#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct Progress {
    /// 1, 2 or 3.
    pub stage: u32,
    pub stages: u32,
    pub label: String,
    /// What is known of the model ("267 faces: 140 planar, …"), or empty before it is read.
    pub detail: String,
    /// The estimated share of the stage done, below 1; `None` when there is no estimate.
    pub fraction: Option<f32>,
    /// Since the open began.
    pub elapsed_ms: u64,
}

struct Running {
    stage: Stage,
    began: Instant,
    stage_began: Instant,
    /// The stage's expected duration, when there is an estimate.
    expected_ms: Option<f64>,
    detail: String,
}

static NOW: Mutex<Option<Running>> = Mutex::new(None);

fn lock() -> std::sync::MutexGuard<'static, Option<Running>> {
    NOW.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// The open in progress, or `None` when nothing is being opened.
pub fn now() -> Option<Progress> {
    let now = lock();
    let r = now.as_ref()?;
    let stage_ms = r.stage_began.elapsed().as_secs_f64() * 1000.0;
    Some(Progress {
        stage: r.stage.number(),
        stages: 3,
        label: r.stage.label().to_string(),
        detail: r.detail.clone(),
        // The bar slows towards its end rather than stop: a stage that runs over its estimate
        // still moves, and never reaches the end before the stage does.
        fraction: r.expected_ms.and_then(|e| estimate(stage_ms, e)),
        elapsed_ms: r.began.elapsed().as_millis() as u64,
    })
}

/// The share of a stage expected to take *expected_ms* done after *ms*: in step with the
/// clock up to 80 %, then slowing so that it never reaches the end. The time a stage takes
/// varies by model several times over (curved faces cost more than flat ones), so the slow
/// part is long: at twice the estimate the bar is near 86 %, at five times near 97 %.
fn estimate(ms: f64, expected_ms: f64) -> Option<f32> {
    if !(expected_ms > 0.0) {
        return None;
    }
    let x = ms / expected_ms;
    let f = if x < 0.8 { x } else { 1.0 - 0.2 * (-(x - 0.8)).exp() };
    Some(f.clamp(0.0, 0.98) as f32)
}

/// Start an open; the state ends when the returned guard is dropped.
pub fn begin(expected_ms: Option<f64>) -> Guard {
    let at = Instant::now();
    *lock() = Some(Running { stage: Stage::Reading, began: at, stage_began: at, expected_ms, detail: String::new() });
    Guard(())
}

/// Move on to *stage*, with what is now known of the model.
pub fn stage(stage: Stage, expected_ms: Option<f64>, detail: Option<String>) {
    if let Some(r) = lock().as_mut() {
        r.stage = stage;
        r.stage_began = Instant::now();
        r.expected_ms = expected_ms;
        if let Some(d) = detail {
            r.detail = d;
        }
    }
}

/// Ends the open's progress when dropped, whichever way the open ends.
pub struct Guard(());

impl Drop for Guard {
    fn drop(&mut self) {
        *lock() = None;
    }
}

/// "267 faces: 140 planar, 95 cylindrical, 32 other · 18 features".
pub fn describe(faces: Faces, features: usize) -> String {
    let mut parts = Vec::new();
    for (n, what) in [(faces.planar, "planar"), (faces.cylindrical, "cylindrical"), (faces.other, "other")] {
        if n > 0 {
            parts.push(format!("{n} {what}"));
        }
    }
    let kinds = if parts.is_empty() { String::new() } else { format!(": {}", parts.join(", ")) };
    let feats = match features {
        1 => " · 1 feature".to_string(),
        n => format!(" · {n} features"),
    };
    let noun = if faces.total == 1 { "face" } else { "faces" };
    format!("{} {noun}{kinds}{feats}", faces.total)
}

/// How long the stages took per unit on this phone: reading per MB of STEP, building per
/// face. A first open on a new phone starts from these.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
pub struct Rates {
    pub read_ms_per_mb: f64,
    pub build_ms_per_face: f64,
    /// Building with the analysis and some projections from the cache.
    pub warm_build_ms_per_face: f64,
}

impl Default for Rates {
    fn default() -> Self {
        Rates { read_ms_per_mb: 2000.0, build_ms_per_face: 100.0, warm_build_ms_per_face: 60.0 }
    }
}

const RATES_FILE: &str = "timing.json";

pub fn rates(root: Option<&Path>) -> Rates {
    root.and_then(|r| std::fs::read(r.join(RATES_FILE)).ok())
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

/// Learn from an open: each rate moves halfway to what was seen, so one odd open does not
/// throw the next estimate far off. Opens too small to time well teach nothing.
pub fn learn(root: Option<&Path>, bytes: usize, read_ms: Option<u128>, faces: usize, build_ms: u128, warm: bool) {
    let Some(root) = root else { return };
    let mut r = rates(Some(root));
    let blend = |old: f64, seen: f64| if seen.is_finite() && seen > 0.0 { (old + seen) / 2.0 } else { old };
    if let Some(ms) = read_ms.filter(|ms| *ms >= 200 && bytes >= 64 * 1024) {
        r.read_ms_per_mb = blend(r.read_ms_per_mb, ms as f64 / (bytes as f64 / 1_048_576.0));
    }
    if build_ms >= 300 && faces >= 20 {
        let seen = build_ms as f64 / faces as f64;
        if warm {
            r.warm_build_ms_per_face = blend(r.warm_build_ms_per_face, seen);
        } else {
            r.build_ms_per_face = blend(r.build_ms_per_face, seen);
        }
    }
    if let Ok(json) = serde_json::to_vec(&r) {
        let _ = std::fs::write(root.join(RATES_FILE), json);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn describes_faces_and_features() {
        let f = Faces { total: 267, planar: 140, cylindrical: 95, other: 32 };
        assert_eq!(describe(f, 18), "267 faces: 140 planar, 95 cylindrical, 32 other · 18 features");
        let f = Faces { total: 6, planar: 6, cylindrical: 0, other: 0 };
        assert_eq!(describe(f, 1), "6 faces: 6 planar · 1 feature");
    }

    #[test]
    fn faces_are_counted_by_surface() {
        let bytes = include_bytes!("../tests/fixtures/GRM-02_shaft.step");
        let (part, analysis) =
            draftwright::model::analyse_native_bytes_part("GRM-02_shaft.step", bytes, None).expect("analyse");
        let f = Faces::of(&part);
        assert!(f.total > 0 && f.planar > 0 && f.cylindrical > 0);
        assert_eq!(f.total, f.planar + f.cylindrical + f.other);
        assert!(describe(f, analysis.features.len()).starts_with(&format!("{} faces: ", f.total)));
    }

    #[test]
    fn the_bar_never_reaches_the_end() {
        assert_eq!(estimate(500.0, 1000.0), Some(0.5));
        let late = estimate(1000.0, 1000.0).unwrap();
        let later = estimate(3000.0, 1000.0).unwrap();
        assert!(0.8 < late && late < later && later < 0.99);
        assert_eq!(estimate(10.0, 0.0), None);
    }

    #[test]
    fn rates_are_learnt() {
        let dir = std::env::temp_dir().join(format!("pocketdraft-rates-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let _ = std::fs::remove_file(dir.join(RATES_FILE));
        learn(Some(&dir), 2 * 1_048_576, Some(1000), 100, 10_000, false);
        let r = rates(Some(&dir));
        assert_eq!(r.read_ms_per_mb, (2000.0 + 500.0) / 2.0);
        assert_eq!(r.build_ms_per_face, (100.0 + 100.0) / 2.0);
        assert_eq!(r.warm_build_ms_per_face, Rates::default().warm_build_ms_per_face);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
