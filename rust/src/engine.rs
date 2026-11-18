//! The drawing engine: a live draftwright session on its own thread.
//!
//! The session lives on one long-lived thread (an "actor") with a large stack: the
//! geometry kernel is recursive and Android's default thread stack may not suffice. The
//! UI never touches the session directly: every call sends a job there and blocks its own
//! (background) thread until the answer comes; the Kotlin side calls from a coroutine on
//! `Dispatchers.Default`, so the UI does not freeze while a drawing is built. A panic in the
//! core is caught and turned into an error instead of crashing the app.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::OnceLock;
use std::sync::mpsc;

use draftwright::items::Op;

use crate::{cache, library, progress};
use crate::log::note;
use draftwright_wasm::{CoreSession, Session};
use serde_json::Value;

/// The answer to an undo with nothing left to undo: news, not an error.
pub const NOTHING_TO_UNDO: &str = "Nothing to undo";

/// Engine thread stack. Analysing complex STEP models recurses deeply.
const ENGINE_STACK: usize = 256 * 1024 * 1024;
/// Problems that mean the drawing does not fit its paper.
const OVERFLOW: [&str; 5] = [
    "annotation_off_sheet",
    "annotation_outside_area",
    "views_do_not_fit",
    "page_fit_uncertain",
    "pinned_view_off_sheet",
];
/// How many problems the list shows (dense parts have hundreds).
const MAX_PROBLEMS: usize = 300;

/// What the UI shows after each operation.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct DocView {
    pub name: String,
    /// SVG of the current sheet.
    pub svg: String,
    pub sheet: u32,
    pub sheets: Vec<u32>,
    pub problems: Vec<ProblemRow>,
    pub problem_total: usize,
    /// Undo has an edit of the user's to take back (none after reopening: the history is
    /// the session's).
    #[serde(default)]
    pub can_undo: bool,
    /// Paper the user chose (`None`: chosen automatically): exact (`sheet/size`) or
    /// preferred (`sheet/preferred_max_size`, larger paper when the drawing does not fit).
    pub size: Option<String>,
    /// The chosen paper is `sheet/size`, not a preferred one.
    #[serde(default)]
    pub exact: bool,
    /// Paper of the shown sheet, as drawn.
    #[serde(default)]
    pub paper: String,
    /// Scale of the shown sheet, as drawn (`1:2`).
    #[serde(default)]
    pub scale: String,
    /// The Spec gives both the paper and the scale: edits do not choose them again.
    #[serde(default)]
    pub kept: bool,
    /// Something is drawn off the sheet or the views do not fit it.
    #[serde(default)]
    pub overflow: bool,
    /// Hidden-line display from the Spec (`None`: the default).
    pub hidden: Option<bool>,
    /// Length units from the Spec (`mm`, `in`).
    #[serde(default = "millimetres")]
    pub units: String,
    /// The isometric view is drawn shaded.
    #[serde(default)]
    pub shaded: bool,
    pub faces: usize,
    pub features: usize,
    /// Title-block fields of the first sheet, as drawn now.
    pub title: Vec<TitleField>,
    /// Where item labels lie on the sheet, for picking by tap.
    pub hits: Vec<Hit>,
    /// Items of the current sheet that can be edited.
    pub items: Vec<ItemInfo>,
    /// Dimensions the user removed from the drawing (`dimensions/<id>/omit`), on any sheet.
    #[serde(default)]
    pub removed: Vec<Removed>,
    /// Datum letters the Spec sets on the sides of the part (`faces/<side>/datum`), by side.
    #[serde(default)]
    pub datums: std::collections::BTreeMap<String, String>,
    /// Where location dimensions are measured from, by part axis (`origin/<axis>`: `min` or
    /// `max`); an axis not listed takes its datum face, else the box minimum.
    #[serde(default)]
    pub origins: std::collections::BTreeMap<String, String>,
    /// Datums nothing refers to are drawn too (`sheet/uncited_datums`).
    #[serde(default)]
    pub uncited_datums: bool,
}

/// A dimension the user removed: its ID and the text it would print.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Removed {
    pub id: String,
    pub label: String,
}

/// An item label's box on the sheet, in SVG coordinates (mm; y down, as in `viewBox`).
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Hit {
    /// Plan item ID (`d:envelope/width.length`), or `title:<key>` for a title-block cell.
    pub id: String,
    pub rect: [f64; 4],
}

/// An editable dimension or label: what the sheet shows and what the Spec sets.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ItemInfo {
    pub id: String,
    /// `dimension` or `annotation`.
    pub kind: String,
    /// Text on the sheet.
    pub label: String,
    pub tolerance: Option<String>,
    pub fit: Option<String>,
    pub prefix: Option<String>,
    pub suffix: Option<String>,
    /// Reference dimension (in parentheses).
    pub reference: bool,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ProblemRow {
    pub key: String,
    pub severity: String,
    /// The core's group for the issues list: `fix_before_printing`, `tidy` or `information`.
    #[serde(default)]
    pub tier: String,
    pub code: String,
    pub message: String,
    /// What else the problem names: Spec paths (`sheet/title/tolerance`), title-block
    /// cells (`title_block/material`), STEP sources.
    pub refs: Vec<String>,
    /// The core's ready-made fixes, from local ones to a sheet re-layout.
    pub fixes: Vec<FixRow>,
}

/// One fix offered by the core: its operations apply together.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct FixRow {
    pub description: String,
    /// `local`, `view` or `sheet`: how much of the drawing changes.
    pub scope: String,
    pub ops: Vec<Op>,
}

/// A title-block field: Spec key (`sheet/title/<key>`), the value on the sheet, and the
/// value the user set (`None`: the core chooses one or leaves it blank).
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TitleField {
    pub key: String,
    pub drawn: Option<String>,
    pub authored: Option<String>,
}

struct Actor {
    session: Option<Session>,
    name: String,
    sheet: u32,
    /// The open drawing's library entry; its Spec is saved after every edit.
    entry: Option<String>,
    /// The opened file's name and bytes (for sharing the STEP).
    file: String,
    bytes: Vec<u8>,
    /// Projection passes the session had made when its projections were last cached.
    kept: usize,
    /// Why the session queued behind a cached sheet could not be built.
    failed: Option<String>,
    /// The user's edits that undo can take back. The session's history may hold one more
    /// step below them, the paper and scale fixed when the drawing was opened ([`fix_sheet`]),
    /// which is not the user's and is never undone.
    edits: usize,
    /// The paper and scale Automatic chose and the app fixed ([`fix_sheet`]): a Spec that
    /// gives exactly these is shown as Automatic.
    chosen: Option<(String, String)>,
}

/// A stopwatch for the engine thread's notes: named laps since the job began, so one line in
/// the log says where an operation's time went (`adb logcat -s Draftwright`).
mod laps {
    use std::cell::RefCell;
    use std::time::Instant;

    thread_local! {
        static LAPS: RefCell<(Instant, Instant, Vec<(&'static str, u128)>)> =
            RefCell::new((Instant::now(), Instant::now(), Vec::new()));
    }

    /// Start over: every job on the engine thread does.
    pub fn start() {
        LAPS.with(|l| *l.borrow_mut() = (Instant::now(), Instant::now(), Vec::new()));
    }

    /// The time since the last lap (or the start) goes to *name*.
    pub fn lap(name: &'static str) {
        LAPS.with(|l| {
            let mut l = l.borrow_mut();
            let now = Instant::now();
            let ms = now.duration_since(l.1).as_millis();
            l.1 = now;
            l.2.push((name, ms));
        });
    }

    /// `1234 ms (plan 1200, svg 20, …)`: the whole, then each lap, in milliseconds.
    pub fn text() -> String {
        LAPS.with(|l| {
            let l = l.borrow();
            let laps: Vec<String> = l.2.iter().map(|(name, ms)| format!("{name} {ms}")).collect();
            format!("{} ms ({})", l.0.elapsed().as_millis(), laps.join(", "))
        })
    }
}

/// Whether the "drawn by" cell keeps the core's credit ("Credit draftwright in Drawn by").
static CREDIT: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

pub fn set_credit(on: bool) {
    CREDIT.store(on, std::sync::atomic::Ordering::Relaxed);
}

fn credited() -> bool {
    CREDIT.load(std::sync::atomic::Ordering::Relaxed)
}

/// The author the title block is drawn with (the plan's `sheet.title`, as the core fills the
/// "drawn by" cell from it), trimmed; empty when none.
fn author(core: &CoreSession) -> String {
    core.plan().sheet.title.get("drawn_by").map(|a| a.trim().to_string()).unwrap_or_default()
}

/// The box of a curve's defining points (its poles bound it), `[x0, y0, x1, y1]`.
fn curve_box(curve: &draftwright::output::Curve, b: &mut [f64; 4]) {
    use draftwright::output::Curve;
    let mut add = |x: f64, y: f64| *b = [b[0].min(x), b[1].min(y), b[2].max(x), b[3].max(y)];
    match curve {
        Curve::Line(p, q) => {
            add(p[0], p[1]);
            add(q[0], q[1]);
        }
        Curve::Conic { centre, radii, .. } => {
            let r = radii[0].abs().max(radii[1].abs());
            add(centre[0] - r, centre[1] - r);
            add(centre[0] + r, centre[1] + r);
        }
        Curve::Beziers(pieces) => pieces.iter().flatten().for_each(|p| add(p[0], p[1])),
        Curve::Rational(pieces) => pieces.iter().flatten().filter(|p| p[2] != 0.0).for_each(|p| add(p[0] / p[2], p[1] / p[2])),
    }
}

/// With the credit off, take draftwright out of a finished drawing's "drawn by" cell. The
/// core always writes it there and has no setting for it, so the app edits the drawing it
/// gets. A label is drawn twice: its glyphs as filled outlines, and its text invisibly over
/// them (for searching). So the credit's glyphs are removed (every filled face inside the
/// credit's part of the label's box), the text becomes the author alone, and the cell's link
/// goes. The text run stays in place, empty at most: annotations refer to runs by position.
fn uncredit(drawing: &mut draftwright::output::Drawing, author: &str) {
    use draftwright::output::{HAlign, Item, VAlign};
    if credited() {
        return;
    }
    // The cell reads `draftwright`, or `<author> / draftwright` (the core's `attribution`).
    let credit = draftwright::title_block::attribution(author);
    let sans = draftwright::title_block::font();
    let mut boxes = Vec::new();
    for run in &mut drawing.text {
        if run.text.trim() != credit || run.rotation.abs() > 1e-6 {
            continue;
        }
        let width = sans.advance(&run.text, run.size);
        let left = run.at[0]
            + match run.h_align {
                HAlign::Left => 0.0,
                HAlign::Center => -width / 2.0,
                HAlign::Right => -width,
            };
        let (down, up) = match run.v_align {
            VAlign::Baseline => (-0.35 * run.size, 1.0 * run.size),
            VAlign::Middle => (-0.7 * run.size, 0.7 * run.size),
        };
        // The author's glyphs stay: the credit starts after them.
        let from = left + if author.is_empty() { -0.2 * run.size } else { sans.advance(&format!("{author} "), run.size) - 0.05 * run.size };
        // The label's whole box, and where in it the credit starts.
        boxes.push(([left - 0.2 * run.size, run.at[1] + down, left + width + 0.3 * run.size, run.at[1] + up], from));
        run.text = author.to_string();
    }
    if boxes.is_empty() {
        note!("credit: no label reads {credit:?}");
    }
    // A label's glyphs may be one face of many loops, or a face a glyph: within the label's
    // box, every loop that starts at or after the credit goes, and a face left empty with it.
    let bounds = |curves: &[draftwright::output::Curve]| {
        let mut b = [f64::MAX, f64::MAX, f64::MIN, f64::MIN];
        curves.iter().for_each(|c| curve_box(c, &mut b));
        b
    };
    for layer in &mut drawing.layers {
        for item in &mut layer.items {
            // draftwright 0.6 keeps a label's outlines as `Glyph`s of its text run; older cores
            // as plain faces. Both are found by the label's box.
            let (Item::Face(loops) | Item::Glyph(loops, _)) = item else { continue };
            let all: Vec<draftwright::output::Curve> = loops.iter().flatten().cloned().collect();
            let b = bounds(&all);
            let Some((_, from)) = boxes.iter().find(|(k, _)| b[0] >= k[0] && b[1] >= k[1] && b[2] <= k[2] && b[3] <= k[3]) else {
                continue;
            };
            loops.retain(|l| bounds(l)[0] < *from - 0.02);
        }
        layer.items.retain(|item| !matches!(item, Item::Face(loops) | Item::Glyph(loops, _) if loops.is_empty()));
    }
    // draftwright 0.6 writes the title block to DXF as native text from its own copy of the
    // runs: the credit leaves that copy too (the run goes when no author is left).
    let structure = &mut drawing.sheet.dxf;
    for run in structure.labels.iter_mut().chain(structure.title_fields.iter_mut().map(|(_, run)| run)) {
        if run.text.trim() == credit {
            run.text = author.to_string();
        }
    }
    structure.labels.retain(|run| !run.text.trim().is_empty());
    structure.title_fields.retain(|(_, run)| !run.text.trim().is_empty());
    drawing.link = None;
}

/// What a kept sheet is stored under: the Spec, and whether the credit was drawn (the same
/// Spec draws differently with it off). The first line names how a new drawing is laid out:
/// sheets kept before the paper and scale were fixed on opening ([`fix_sheet`]) show the
/// layout the core chose, which the session no longer draws, so they are not used.
fn snapshot_key(spec: &str) -> String {
    let layout = if KEEP_SHEET.load(std::sync::atomic::Ordering::Relaxed) { "fixed on opening" } else { "chosen" };
    if credited() { format!("{layout}\n{spec}") } else { format!("{layout}\n{spec}\n(no credit)") }
}

/// Whether edits give the core the paper and scale it chose ("Keep paper and scale").
static KEEP_SHEET: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

pub fn set_keep_sheet(on: bool) {
    KEEP_SHEET.store(on, std::sync::atomic::Ordering::Relaxed);
}

/// The operations that give the core the paper and scale the drawing is planned on, when
/// "Keep paper and scale" is on and the Spec leaves either to the core. Nothing when the edit
/// itself sets the sheet (a paper choice, a fix that refits the sheet).
fn keep_sheet_ops(session: &Session, ops: &[Op]) -> Vec<Op> {
    if !KEEP_SHEET.load(std::sync::atomic::Ordering::Relaxed)
        || ops.iter().any(|o| o.path == "sheet" || o.path.starts_with("sheet/size") || o.path.starts_with("sheet/scale")
            || o.path.starts_with("sheet/preferred") || o.path.starts_with("sheets") || o.path.starts_with("sheet/keep_layout"))
    {
        return Vec::new();
    }
    let core = &session.0;
    let given = |field: &str| serde_json::to_value(&core.spec().sheet).ok().and_then(|s| s.get(field).cloned()).is_some_and(|v| !v.is_null());
    let sheet = &core.plan().sheet;
    let set = |path: &str, value: Value| Op { op: "set".into(), path: path.into(), value: Some(value) };
    let mut pin = Vec::new();
    if !given("size") {
        pin.push(set("sheet/size", Value::String(sheet.size.to_string())));
        pin.push(set("sheet/preferred_max_size", Value::Null));
    }
    if !given("scale") {
        pin.push(set("sheet/scale", Value::String(sheet.scale.to_string())));
    }
    pin
}

/// Apply *ops* as one edit (one undo step), with the paper and scale kept when the setting
/// asks for it; a pin the core refuses is dropped and the edit planned as it is.
///
/// One of the core's ready fixes ("only this item") leaves `sheet/keep_layout` on, and with
/// it the core refuses any edit that changes what is drawn (removing a dimension, a datum):
/// "This edit cannot keep the retained sheet layout". Such an edit is applied again with the
/// guard released, as the core's message says to; the paper and scale stay as given.
fn edit(a: &mut Actor, ops: Vec<Op>, what: &str) -> Result<DocView, String> {
    let session = a.session.as_mut().ok_or_else(|| no_session(&a.failed))?;
    let mut all = keep_sheet_ops(session, &ops);
    let pinned = !all.is_empty();
    all.extend(ops.iter().cloned());
    let mut result = session.apply(&all, true);
    if result.is_err() && pinned {
        if let Err(e) = &result {
            note!("keep sheet: not kept ({}); planning the edit alone", e.message);
        }
        all = ops.clone();
        result = session.apply(&all, true);
    }
    let guarded = session.0.spec().sheet.keep_layout == Some(true) && !ops.iter().any(|o| o.path.starts_with("sheet/keep_layout"));
    if result.is_err() && guarded {
        note!("{what}: refused with the layout kept; applying it with the layout released");
        all.push(Op { op: "remove".into(), path: "sheet/keep_layout".into(), value: None });
        result = session.apply(&all, true);
    }
    result.map_err(err)?;
    a.edits += 1;
    finish(a, what)
}

/// With "Keep paper and scale" on, give the core the paper and scale it has just chosen, so
/// that the drawing is shown as later edits keep it. The core lays a sheet out differently
/// when the same paper and scale are given rather than chosen (views move by up to tens of
/// mm), so fixing them on the first edit moved every view of a drawing the user had only
/// filled the title block of. Done when a drawing is built and after Automatic paper, before
/// the sheet is shown. A pin the core refuses leaves the drawing as it was chosen.
/// Answers the paper and scale fixed, or `None` when nothing was.
fn fix_sheet(session: &mut Session) -> Option<(String, String)> {
    if !automatic(session) {
        return None;
    }
    let pin = keep_sheet_ops(session, &[]);
    if pin.is_empty() {
        return None;
    }
    match session.apply(&pin, true) {
        Ok(_) => Some(fixed(session)),
        Err(e) => {
            note!("keep sheet: the chosen paper and scale not fixed ({})", e.message);
            None
        }
    }
}

/// Whether the core chooses both paper and scale (no paper, preferred paper or scale given).
fn automatic(session: &Session) -> bool {
    let sheet = &session.0.spec().sheet;
    sheet.size.is_none() && sheet.preferred_max_size.is_none() && sheet.scale.is_none()
}

/// The paper and scale the Spec gives (empty where it gives none).
fn fixed(session: &Session) -> (String, String) {
    let sheet = &session.0.spec().sheet;
    (sheet.size.clone().unwrap_or_default(), sheet.scale.clone().unwrap_or_default())
}

/// After an edit on the engine thread: the new view of the drawing, saved with its library
/// entry, and a note of where the time went.
fn finish(a: &mut Actor, what: &str) -> Result<DocView, String> {
    laps::lap("plan");
    let doc = view(a)?;
    remember(a, &doc);
    if let Some(session) = a.session.as_ref() {
        let stats = session.0.stats();
        let sheet = &session.0.plan().sheet;
        note!(
            "{what}: {}; {} at {}, svg {} KB, {} problems, {} items, {} tap targets; core plans {}, sheet runs {}",
            laps::text(),
            sheet.size,
            sheet.scale,
            doc.svg.len() / 1024,
            doc.problem_total,
            doc.items.len(),
            doc.hits.len(),
            stats.plans,
            stats.sheet_steps,
        );
    }
    Ok(doc)
}

/// The answer to an action with no session: the reason a queued build failed, if one did.
fn no_session(failed: &Option<String>) -> String {
    failed.clone().unwrap_or_else(|| "No file is open".to_string())
}

type Job = Box<dyn FnOnce(&mut Actor) + Send>;

fn sender() -> &'static mpsc::Sender<Job> {
    static TX: OnceLock<mpsc::Sender<Job>> = OnceLock::new();
    TX.get_or_init(|| {
        let (tx, rx) = mpsc::channel::<Job>();
        std::thread::Builder::new()
            .name("draftwright-engine".into())
            .stack_size(ENGINE_STACK)
            .spawn(move || {
                let mut actor = Actor {
                    session: None,
                    name: String::new(),
                    sheet: 1,
                    entry: None,
                    file: String::new(),
                    bytes: Vec::new(),
                    kept: 0,
                    edits: 0,
                    chosen: None,
                    failed: None,
                };
                for job in rx {
                    job(&mut actor);
                }
            })
            .expect("spawn draftwright engine thread");
        tx
    })
}

/// Run `f` on the session on the engine thread and wait for its answer.
fn call<T, F>(f: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce(&mut Actor) -> Result<T, String> + Send + 'static,
{
    let (reply_tx, reply_rx) = mpsc::channel();
    let job: Job = Box::new(move |actor| {
        laps::start();
        let result = catch_unwind(AssertUnwindSafe(|| f(actor)))
            .unwrap_or_else(|panic| Err(format!("Core failure: {}", panic_text(&*panic))));
        let _ = reply_tx.send(result);
    });
    sender()
        .send(job)
        .map_err(|_| "The engine thread has stopped".to_string())?;
    reply_rx
        .recv()
        .map_err(|_| "The engine thread has stopped".to_string())?
}

fn panic_text(panic: &(dyn std::any::Any + Send)) -> String {
    if let Some(s) = panic.downcast_ref::<&str>() {
        (*s).to_string()
    } else if let Some(s) = panic.downcast_ref::<String>() {
        s.clone()
    } else {
        "unknown panic".to_string()
    }
}

fn err(e: draftwright::api::Error) -> String {
    e.message
}

/// Whether the bytes look like STEP: an ISO 10303-21 header at the start, or gzip
/// (draftwright unpacks compressed STEP itself).
pub fn looks_like_step(bytes: &[u8]) -> bool {
    if bytes.starts_with(&[0x1f, 0x8b]) {
        return true;
    }
    let head = &bytes[..bytes.len().min(512)];
    head.windows(12).any(|w| w.eq_ignore_ascii_case(b"ISO-10303-21"))
}

/// One part of a STEP file, as the part picker lists it.
#[derive(Clone, Debug, PartialEq)]
pub struct PartRow {
    pub index: usize,
    pub name: String,
    pub faces: usize,
    pub instances: usize,
}

/// The parts of a STEP file. More than one means an assembly: the user picks one part, or
/// the whole model.
pub fn parts(name: String, bytes: Vec<u8>) -> Result<Vec<PartRow>, String> {
    if !looks_like_step(&bytes) {
        return Err(not_step(&name));
    }
    call(move |_| {
        let json = draftwright_wasm::list_parts_json(&name, &bytes).map_err(err)?;
        let value: Value = serde_json::from_str(&json).map_err(|e| e.to_string())?;
        Ok(value
            .get("parts")
            .and_then(Value::as_array)
            .map(|parts| {
                parts
                    .iter()
                    .map(|p| PartRow {
                        index: p.get("part").and_then(Value::as_u64).unwrap_or(0) as usize,
                        name: p.get("name").and_then(Value::as_str).unwrap_or("part").to_string(),
                        faces: p.get("faces").and_then(Value::as_u64).unwrap_or(0) as usize,
                        instances: p.get("instances").and_then(Value::as_u64).unwrap_or(1) as usize,
                    })
                    .collect()
            })
            .unwrap_or_default())
    })
}

fn not_step(name: &str) -> String {
    format!("\"{name}\" is not a STEP file: no ISO-10303-21 header. Open a .step or .stp file.")
}

/// What to open: a file, optionally one part of an assembly, optionally replacing an
/// earlier library entry whose edits it takes over ("Update model").
pub struct Opening {
    pub name: String,
    pub bytes: Vec<u8>,
    /// `(index, name)` of one part of an assembly; `None` is the whole file.
    pub part: Option<(usize, String)>,
    pub replaces: Option<String>,
}

impl Opening {
    pub fn file(name: String, bytes: Vec<u8>) -> Self {
        Self { name, bytes, part: None, replaces: None }
    }
}

/// Analyse a STEP file (bytes, .gz allowed) and open a session. A file opened before comes
/// back with its saved edits; a new one starts from the title-block defaults. A new
/// session starts with an empty undo history.
///
/// Cached instances (see [`cache`]): the drawing still open from before is reused as it
/// is; otherwise the model's Analysis and projected views come from its cache when there,
/// and the sheet is planned from the saved Spec.
pub fn open(opening: Opening) -> Result<DocView, String> {
    if !looks_like_step(&opening.bytes) {
        return Err(not_step(&opening.name));
    }
    call(move |a| {
        let Opening { name, bytes, part, replaces } = opening;
        let started = std::time::Instant::now();
        let part_index = part.as_ref().map(|p| p.0);
        let root = library::root();
        let known = library::id_of(&bytes, part_index);

        // The drawing in memory (left for the library a moment ago): nothing to compute.
        if replaces.is_none() && a.session.is_some() && known.is_some() && a.entry == known && a.bytes == bytes {
            a.sheet = 1;
            let doc = view(a)?;
            if let (Some(root), Some(id)) = (root, known.as_deref()) {
                let _ = library::update(root, id, |e| e.opened = library::now());
            }
            note!("open {name}: in memory, {} ms", started.elapsed().as_millis());
            return Ok(doc);
        }

        // The sheet as it was last drawn from these same edits: shown at once, while the
        // session is built by the next job on this thread. Whatever the user does next
        // queues behind that build.
        if let (None, Some(root), Some(id)) = (&replaces, root, known.as_deref()) {
            let (_, starting) = starting_spec(Some(root), id, None);
            if let Some(doc) = cache::sheet(root, id, &snapshot_key(&starting)) {
                a.session = None;
                a.entry = None;
                a.failed = None;
                let job: Job = Box::new(move |a| {
                    let built = catch_unwind(AssertUnwindSafe(|| build(a, name, bytes, part, None, started)))
                        .unwrap_or_else(|panic| Err(format!("Core failure: {}", panic_text(&*panic))));
                    if let Err(e) = built {
                        note!("open: the session behind a cached sheet failed: {e}");
                        a.failed = Some(e);
                    }
                });
                if sender().send(job).is_ok() {
                    note!("open {}: sheet from cache, {} ms; session follows", doc.name, started.elapsed().as_millis());
                    return Ok(doc);
                }
                return Err("The engine thread has stopped".to_string());
            }
        }
        build(a, name, bytes, part, replaces, started)
    })
}

/// This entry's saved edits, or those of the entry an updated model replaces; and the Spec a
/// session starts from (the saved one, or the title-block defaults).
fn starting_spec(root: Option<&std::path::Path>, id: &str, replaces: Option<&str>) -> (Option<String>, String) {
    let saved = root.and_then(|r| library::spec(r, id).or_else(|| replaces.and_then(|old| library::spec(r, old))));
    let starting = match &saved {
        Some(json) => json.clone(),
        None => library::defaults_spec(&root.map(library::defaults).unwrap_or_default()),
    };
    (saved, starting)
}

/// Build the session of a STEP file from cached pieces where they are valid, record it in
/// the library and cache what it computed.
fn build(
    a: &mut Actor,
    name: String,
    bytes: Vec<u8>,
    part: Option<(usize, String)>,
    replaces: Option<String>,
    started: std::time::Instant,
) -> Result<DocView, String> {
        a.failed = None;
        let part_index = part.as_ref().map(|p| p.0);
        let root = library::root();
        let rates = progress::rates(root);
        let _progress = progress::begin(Some(rates.read_ms_per_mb * bytes.len() as f64 / 1_048_576.0));
        let known = library::id_of(&bytes, part_index);
        let analyse = || {
            draftwright::model::analyse_native_bytes_part(&name, &bytes, part_index)
                .map_err(|e| err(e.into()))
        };
        let cached = root
            .zip(known.as_deref())
            .and_then(|(r, id)| cache::analysed(r, id, &bytes, part_index));
        let warm = cached.is_some();
        let (shape, analysis) = match cached {
            Some(found) => found,
            None => analyse()?,
        };
        let analysed_ms = started.elapsed().as_millis();
        let faces = progress::Faces::of(&shape);
        let build_rate = if warm { rates.warm_build_ms_per_face } else { rates.build_ms_per_face };
        progress::stage(
            progress::Stage::Building,
            Some(build_rate * faces.total as f64),
            Some(progress::describe(faces, analysis.features.len())),
        );
        let id = library::entry_id(&analysis.binding.source_sha256, part_index);
        let projections = root.map(|r| cache::projections(r, &id)).unwrap_or_default();
        let (saved, starting) = starting_spec(root, &id, replaces.as_deref());
        // A Spec the core accepts as it is opens without history. One it refuses (made for
        // another revision of the part, or an older draftwright) goes through `open_spec`,
        // which sets stale entries aside, as one undoable edit.
        let direct = match serde_json::from_str::<draftwright::model::Spec>(&starting) {
            Ok(spec) => CoreSession::with_projections(shape, analysis, spec, projections).map_err(err),
            Err(e) => Err(e.to_string()),
        };
        // The user's edits in the session's history: the migration below is one.
        let (mut session, edits) = match direct {
            Ok(core) => (Session(core), 0),
            Err(why) => {
                // Rare: read the model again (the core's Part is not cloneable) and let
                // `open_spec` migrate what it can.
                note!("library: saved edits of {id} need migrating: {why}");
                if warm {
                    // Not knowing which was refused, the cache or the Spec: rebuild both.
                    if let Some(r) = root {
                        cache::clear(r, &id);
                    }
                }
                let (shape, analysis) = analyse()?;
                let mut fresh = Session(
                    CoreSession::new(shape, analysis, draftwright::model::Spec::default()).map_err(err)?,
                );
                let migrated = match fresh.open_spec_json(&starting) {
                    Ok(_) => 1,
                    Err(e) => {
                        note!("library: saved edits of {id} not restored: {}", e.message);
                        0
                    }
                };
                (fresh, migrated)
            }
        };
        // Below a migration the pin would be the first step undo takes back: only without one.
        let mut chosen = if edits == 0 { fix_sheet(&mut session) } else { None };
        if chosen.is_some() {
            note!("open: paper and scale fixed as chosen, {} ms in", started.elapsed().as_millis());
        } else if let Some(r) = root {
            // A drawing reopened with its edits: what Automatic chose for it then.
            chosen = library::list(r).into_iter().find(|e| e.id == id).and_then(|e| e.chosen_sheet);
        }
        a.session = Some(session);
        a.edits = edits;
        a.chosen = chosen;
        a.name = match &part {
            Some((_, part_name)) => format!("{part_name} · {name}"),
            None => name.clone(),
        };
        a.file = name.clone();
        a.bytes = bytes.clone();
        a.sheet = 1;
        a.entry = Some(id.clone());
        a.kept = 0;
        let session_ms = started.elapsed().as_millis();
        progress::stage(progress::Stage::Drawing, None, None);
        progress::learn(root, bytes.len(), (!warm).then_some(analysed_ms), faces.total, session_ms - analysed_ms, warm);
        let doc = view(a)?;
        if let Some(root) = root {
            let entry = library::Entry {
                id: id.clone(),
                name,
                size: bytes.len() as u64,
                opened: library::now(),
                faces: doc.faces,
                features: doc.features,
                problems: doc.problem_total,
                edited: saved.is_some(),
                part: part.as_ref().map(|p| p.0),
                part_name: part.map(|p| p.1),
                chosen_sheet: a.chosen.clone(),
            };
            let mut stored = library::record(root, entry, &bytes)
                .and_then(|()| library::save_thumbnail(root, &id, &doc.svg, iso_area(a)));
            if let Some(old) = replaces.as_deref().filter(|old| *old != id) {
                // The new model takes over the edits; the old entry goes.
                let spec = a.session.as_ref().map(Session::spec_json).unwrap_or_default();
                stored = stored
                    .and_then(|()| library::save_spec(root, &id, &spec))
                    .and_then(|()| library::update(root, &id, |e| e.edited = true))
                    .and_then(|()| library::remove(root, old));
            }
            if let Err(e) = stored {
                note!("library: could not record {}: {e}", doc.name);
            }
            if !warm {
                if let Some(session) = &a.session {
                    cache::keep_analysis(root, &id, session.0.analysis());
                }
            }
            keep(a);
            if doc.sheet == 1 {
                cache::keep_sheet(root, &id, &snapshot_key(&starting), &doc);
            }
        }
        let how = if !warm { "cold" } else if a.kept == 0 { "from cache" } else { "from cache, some views projected" };
        note!(
            "open {}: {how}, {} ms (model {analysed_ms} ms, session +{} ms, sheet and library +{} ms)",
            doc.name,
            started.elapsed().as_millis(),
            session_ms - analysed_ms,
            started.elapsed().as_millis() - session_ms,
        );
        Ok(doc)
}

/// The page area of the isometric view on the shown sheet, for the list thumbnail.
fn iso_area(a: &Actor) -> Option<[f64; 4]> {
    let core = &a.session.as_ref()?.0;
    core.plan()
        .views
        .iter()
        .find(|v| v.id == "v:iso" && v.sheet == a.sheet)
        .map(|v| v.area)
}

/// Open a drawing from the library, with its saved edits.
/// Forget the session kept in memory when it belongs to library entry *id* (removed from
/// the library): opening the file again then starts afresh, not with the removed edits.
pub fn forget(id: String) -> Result<(), String> {
    call(move |a| {
        if a.entry.as_deref() == Some(id.as_str()) {
            a.session = None;
            a.entry = None;
            a.bytes.clear();
        }
        Ok(())
    })
}

pub fn open_saved(id: String) -> Result<DocView, String> {
    let root = library::root().ok_or("The library is not available")?;
    let (entry, bytes) =
        library::load(root, &id).map_err(|e| format!("Could not open it from the library: {e}"))?;
    let part = entry
        .part
        .map(|i| (i, entry.part_name.clone().unwrap_or_else(|| format!("part {}", i + 1))));
    open(Opening { name: entry.name, bytes, part, replaces: None })
}

/// The open drawing's STEP file, for sharing.
pub fn model() -> Result<(String, Vec<u8>), String> {
    call(|a| {
        if a.bytes.is_empty() {
            return Err(no_session(&a.failed));
        }
        Ok((a.file.clone(), a.bytes.clone()))
    })
}

/// Cache the session's projections when it projected anything since they were last kept
/// (a cold open, a new view, another sheet).
fn keep(a: &mut Actor) {
    let (Some(root), Some(id), Some(session)) = (library::root(), a.entry.as_deref(), a.session.as_ref()) else {
        return;
    };
    let stats = session.0.stats();
    let made = stats.projections + stats.bounds;
    if made > a.kept {
        cache::keep_projections(root, id, session.0.projections());
        a.kept = made;
    }
}

/// After an edit: store the Spec and the new problem count with the library entry, and
/// cache any newly projected views.
fn remember(a: &mut Actor, doc: &DocView) {
    keep(a);
    laps::lap("projections");
    let (Some(root), Some(id), Some(session)) = (library::root(), a.entry.as_deref(), a.session.as_ref()) else {
        return;
    };
    let spec = session.spec_json();
    let saved = library::save_spec(root, id, &spec).and_then(|()| {
        let chosen = a.chosen.clone();
        library::update(root, id, |e| {
            e.chosen_sheet = chosen;
            e.edited = true;
            e.opened = library::now();
            e.problems = doc.problem_total;
        })
    });
    laps::lap("save");
    if let Err(e) = saved {
        note!("library: could not save the edits of {id}: {e}");
    } else if doc.sheet == 1 {
        cache::keep_sheet(root, id, &snapshot_key(&spec), doc);
        laps::lap("snapshot");
    }
}

/// Change one Spec field (`sheet/size`, `sheet/hidden`, `sheet/title/material`, …).
/// `Value::Null` removes the field, restoring the automatic choice.
pub fn set(path: &'static str, value: Value) -> Result<DocView, String> {
    call(move |a| {
        let op = Op {
            op: "set".into(),
            path: path.into(),
            value: Some(value),
        };
        edit(a, vec![op], "edit")
    })
}

/// The drawing's length units (`mm`, `in`), as one edit.
///
/// Tolerances the user typed are written in the new units ([`fixes::convert_tolerance`]):
/// the core prints them as typed. The scale is chosen again for the new units, on the same
/// paper (a paper the core chose is chosen again too): a scale kept from an inch drawing
/// (4:1) is not one of the metric ones, and the core refuses it.
pub fn set_units(units: String) -> Result<DocView, String> {
    call(move |a| {
        let session = a.session.as_mut().ok_or_else(|| no_session(&a.failed))?;
        let from = session.0.plan().sheet.units;
        let to = match units.as_str() {
            "in" => draftwright::units::Units::In,
            _ => draftwright::units::Units::Mm,
        };
        if from == to {
            return view(a);
        }
        let set = |path: String, value: Value| Op { op: "set".into(), path, value: Some(value) };
        let factor = from.millimetres() / to.millimetres();
        let spec = session.0.spec();
        let mut ops = vec![set("units".into(), Value::String(units))];
        for (id, dim) in &spec.dimensions {
            if let Some(t) = dim.tolerance.as_deref().filter(|t| !t.trim().is_empty()) {
                let path = format!("dimensions/{}/tolerance", draftwright::items::key(id));
                ops.push(set(path, Value::String(crate::fixes::convert_tolerance(t, factor))));
            }
        }
        if let Some(t) = spec.envelope_tolerance.as_deref().filter(|t| !t.trim().is_empty()) {
            ops.push(set("envelope_tolerance".into(), Value::String(crate::fixes::convert_tolerance(t, factor))));
        }
        let chosen = a.chosen.as_ref() == Some(&fixed(session));
        if chosen {
            ops.extend(paper_ops(None, false));
        } else if spec.sheet.scale.is_some() {
            ops.push(set("sheet/scale".into(), Value::Null));
        }
        if spec.sheet.keep_layout == Some(true) {
            ops.push(Op { op: "remove".into(), path: "sheet/keep_layout".into(), value: None });
        }
        session.apply(&ops, true).map_err(err)?;
        // The scale (and paper) chosen for the new units, fixed in the same edit when "Keep
        // paper and scale" is on, as Automatic paper does.
        let pin = keep_sheet_ops(session, &[]);
        if !pin.is_empty() {
            session.0.undo().map_err(err)?;
            let mut all = ops.clone();
            all.extend(pin);
            match session.apply(&all, true) {
                Ok(_) => {
                    if chosen {
                        a.chosen = Some(fixed(session));
                    }
                }
                Err(e) => {
                    note!("keep sheet: the scale for the new units not fixed ({})", e.message);
                    session.apply(&ops, true).map_err(err)?;
                }
            }
        }
        a.edits += 1;
        finish(a, "units")
    })
}

/// Choose the paper: `None` lets the core choose. An exact paper is kept even when the
/// drawing runs off it. Otherwise the paper is tried as it is and, when the drawing does not
/// fit it legibly, set as the preferred one instead (`sheet/preferred_max_size`), from which
/// the core moves to larger paper: one edit either way.
pub fn set_paper(size: Option<String>, exact: bool) -> Result<DocView, String> {
    call(move |a| {
        choose_paper(a, size, exact)?;
        a.edits += 1;
        finish(a, "paper")
    })
}

fn choose_paper(a: &mut Actor, size: Option<String>, exact: bool) -> Result<(), String> {
    let automatic = size.is_none();
    let session = a.session.as_mut().ok_or_else(|| no_session(&a.failed))?;
    session.apply(&paper_ops(size.clone(), true), true).map_err(err)?;
    let overflows = session.0.state().problems.iter().any(|p| OVERFLOW.contains(&p.code.as_str()));
    if size.is_some() && !exact && overflows {
        session.0.undo().map_err(err)?;
        session.apply(&paper_ops(size, false), true).map_err(err)?;
    }
    if automatic {
        // Automatic, then the paper and scale it chose fixed in the same edit (see
        // [`fix_sheet`]): one undo step, and the next edit does not move the views.
        let pin = keep_sheet_ops(session, &[]);
        if !pin.is_empty() {
            session.0.undo().map_err(err)?;
            let mut ops = paper_ops(None, false);
            ops.extend(pin);
            match session.apply(&ops, true) {
                Ok(_) => a.chosen = Some(fixed(session)),
                Err(e) => {
                    note!("keep sheet: Automatic paper not fixed ({})", e.message);
                    session.apply(&paper_ops(None, false), true).map_err(err)?;
                }
            }
        }
    }
    Ok(())
}

/// `exact`: the paper as `sheet/size`; otherwise as the preferred one. Either clears the other,
/// and the scale: the core chooses one for the new paper (the next edit keeps it, with "Keep
/// paper and scale" on). `None` (Automatic) lays the drawing out again from scratch.
fn paper_ops(size: Option<String>, exact: bool) -> Vec<Op> {
    let value = |on: bool| size.clone().filter(|_| on).map_or(Value::Null, Value::String);
    vec![
        Op { op: "set".into(), path: "sheet/size".into(), value: Some(value(exact)) },
        Op { op: "set".into(), path: "sheet/preferred_max_size".into(), value: Some(value(!exact)) },
        Op { op: "set".into(), path: "sheet/scale".into(), value: Some(Value::Null) },
    ]
}

/// Remove a dimension from the drawing (`dimensions/<id>/omit`), or put a removed one back.
/// The core may leave out the dimensions that only make sense with it, and says so in a
/// problem; putting it back returns them.
pub fn set_omitted(id: String, omit: bool) -> Result<DocView, String> {
    let path = format!("dimensions/{}/omit", draftwright::items::key(&id));
    apply(vec![if omit {
        Op { op: "set".into(), path, value: Some(Value::Bool(true)) }
    } else {
        Op { op: "remove".into(), path, value: None }
    }])
}

/// Put a datum letter on a side of the part (`bottom`, `top`, `left`, `right`, `front`,
/// `back`), or take the side's datum away (`None`). A letter names one face, so a letter
/// another side holds moves here: one edit.
pub fn set_datum(side: String, letter: Option<String>) -> Result<DocView, String> {
    call(move |a| {
        let session = a.session.as_mut().ok_or_else(|| no_session(&a.failed))?;
        let at = |side: &str| format!("faces/{side}/datum");
        let set = |path: String, value: Value| Op { op: "set".into(), path, value: Some(value) };
        let mut ops: Vec<Op> = session
            .0
            .spec()
            .faces
            .iter()
            .filter(|(other, face)| **other != side && letter.is_some() && face.datum == letter)
            .map(|(other, _)| set(at(other), Value::Null))
            .collect();
        ops.push(set(at(&side), letter.map_or(Value::Null, Value::String)));
        edit(a, ops, "datum")
    })
}

/// Choose the end of the part's box that location dimensions along an axis (`x`, `y`, `z`)
/// are measured from: `min` or `max`. `None` returns the choice to the core: the datum face
/// across that axis, else the box minimum.
pub fn set_origin(axis: String, end: Option<String>) -> Result<DocView, String> {
    call(move |a| {
        let session = a.session.as_mut().ok_or_else(|| no_session(&a.failed))?;
        let path = format!("origin/{axis}");
        let op = match end {
            Some(end) => Op { op: "set".into(), path, value: Some(Value::String(end)) },
            None if session.0.spec().origin.contains_key(&axis) => Op { op: "remove".into(), path, value: None },
            None => return view(a),
        };
        edit(a, vec![op], "origin")
    })
}

/// Apply a set of operations as one edit (one undo step): a core fix or values the user
/// entered.
pub fn apply(ops: Vec<Op>) -> Result<DocView, String> {
    call(move |a| edit(a, ops, "edit"))
}

pub fn undo() -> Result<DocView, String> {
    call(|a| {
        let session = a.session.as_mut().ok_or_else(|| no_session(&a.failed))?;
        if a.edits == 0 {
            // Below the user's edits lies at most the paper and scale fixed on opening.
            return Err(NOTHING_TO_UNDO.to_string());
        }
        session
            .0
            .undo()
            .map_err(|_| NOTHING_TO_UNDO.to_string())?;
        a.edits -= 1;
        finish(a, "undo")
    })
}

/// The paper and scale the open drawing's first sheet is planned on (for the benchmark).
#[cfg(test)]
pub fn planned() -> Result<(String, String), String> {
    call(|a| {
        let session = a.session.as_ref().ok_or_else(|| no_session(&a.failed))?;
        let sheet = &session.0.plan().sheet;
        Ok((sheet.size.to_string(), sheet.scale.to_string()))
    })
}

pub fn show_sheet(sheet: u32) -> Result<DocView, String> {
    call(move |a| {
        a.sheet = sheet;
        let doc = view(a)?;
        keep(a);
        // The first sheet shown again (after the credit setting changed, say) is the sheet to
        // show first next time.
        if let (1, Some(root), Some(id), Some(session)) = (doc.sheet, library::root(), a.entry.as_deref(), a.session.as_ref()) {
            cache::keep_sheet(root, id, &snapshot_key(&session.spec_json()), &doc);
        }
        Ok(doc)
    })
}

/// PDF of all sheets with embedded fonts.
pub fn pdf() -> Result<Vec<u8>, String> {
    call(|a| {
        let session = a.session.as_ref().ok_or_else(|| no_session(&a.failed))?;
        // As the core's own PDF, from drawings the credit setting has been applied to.
        let core = &session.0;
        let author = author(core);
        let numbers = std::iter::once(1).chain(core.plan().sheets.iter().map(|s| s.number));
        let mut drawings = Vec::new();
        for number in numbers {
            let mut drawing = core.drawing_on_sheet(number).map_err(err)?;
            uncredit(&mut drawing, &author);
            drawings.push(drawing);
        }
        Ok(draftwright::output::Drawing::pdf_pages(&drawings, &|name| match name {
            "IBMPlexMono-Regular.ttf" => Some(draftwright::api::MONO.to_vec()),
            draftwright::title_block::FONT => Some(draftwright::title_block::FONT_DATA.to_vec()),
            _ => None,
        }))
    })
}

/// DXF of the shown sheet (the core writes one sheet per file), with the credit setting applied.
pub fn dxf() -> Result<Vec<u8>, String> {
    call(|a| {
        let session = a.session.as_ref().ok_or_else(|| no_session(&a.failed))?;
        let core = &session.0;
        let mut drawing = core.drawing_on_sheet(a.sheet).map_err(err)?;
        uncredit(&mut drawing, &author(core));
        Ok(drawing.dxf())
    })
}

fn millimetres() -> String {
    "mm".into()
}

fn view(a: &mut Actor) -> Result<DocView, String> {
    let session = a.session.as_ref().ok_or_else(|| no_session(&a.failed))?;
    let core = &session.0;

    let mut sheets = vec![1u32];
    for planned in &core.plan().sheets {
        if !sheets.contains(&planned.number) {
            sheets.push(planned.number);
        }
    }
    if !sheets.contains(&a.sheet) {
        a.sheet = 1;
    }
    // One render of the sheet gives both the SVG and the label boxes for tap picking.
    let mut drawing = core.drawing_on_sheet(a.sheet).map_err(err)?;
    let author = author(core);
    uncredit(&mut drawing, &author);
    laps::lap("drawing");
    let svg = drawing.svg();
    laps::lap("svg");

    let spec = serde_json::to_value(core.spec()).map_err(|e| e.to_string())?;
    let text = |path: &str| spec.pointer(path).and_then(Value::as_str).map(str::to_string);
    let exact = text("/sheet/size");
    let preferred = text("/sheet/preferred_max_size");
    let planned = core.plan().sheets.iter().find(|s| s.number == a.sheet).unwrap_or(&core.plan().sheet);
    let paper = planned.size.clone();
    let scale = planned.scale.to_string();
    let kept = exact.is_some() && spec.pointer("/sheet/scale").is_some_and(|v| !v.is_null());
    // The paper and scale Automatic chose, fixed by the app: still Automatic to the user.
    let as_chosen = kept && preferred.is_none() && a.chosen.as_ref() == Some(&fixed(session));
    let hidden = spec.pointer("/sheet/hidden").and_then(Value::as_bool);
    let units = text("/units").unwrap_or_else(millimetres);
    let shaded = text("/views/iso/style").as_deref() == Some("shaded");

    let all = &core.state().problems;
    let overflow = all.iter().any(|p| OVERFLOW.contains(&p.code.as_str()));
    let mut problems: Vec<ProblemRow> = all
        .iter()
        .enumerate()
        .map(|(i, p)| ProblemRow {
            key: format!("{i}:{}", p.id),
            severity: p.severity.clone(),
            tier: p.labels().tier.as_str().to_string(),
            code: p.code.clone(),
            message: p.message.clone(),
            refs: p.refs.clone(),
            fixes: p
                .offered_fixes()
                .into_iter()
                .map(|f| FixRow {
                    description: f.description(),
                    scope: serde_json::to_value(f.scope())
                        .ok()
                        .and_then(|v| v.as_str().map(str::to_string))
                        .unwrap_or_default(),
                    ops: f.ops,
                })
                .collect(),
        })
        .collect();
    // In the core's group order (fix before printing, tidy, information), then by severity.
    problems.sort_by_key(|p| {
        let tier = draftwright::finding::Tier::ALL.iter().position(|t| t.as_str() == p.tier).unwrap_or(usize::MAX);
        let severity = match p.severity.as_str() {
            "error" => 0,
            "warning" => 1,
            _ => 2,
        };
        (tier, severity)
    });
    problems.truncate(MAX_PROBLEMS);
    laps::lap("problems");

    let authored = spec.pointer("/sheet/title").and_then(Value::as_object);
    let cells = &core.plan().sheet.title_cells;
    let title = draftwright::title_block::KEYS
        .iter()
        .filter(|(key, _)| *key != "sheet")
        .map(|(key, field)| TitleField {
            key: key.to_string(),
            drawn: cells
                .iter()
                .find(|c| c.field == *field)
                .and_then(|c| c.value.clone()),
            authored: authored
                .and_then(|t| t.get(*key))
                .and_then(Value::as_str)
                .map(str::to_string),
        })
        .collect();

    // With the credit off the "drawn by" cell shows the author alone.
    let mut title: Vec<TitleField> = title;
    if !credited() {
        for field in title.iter_mut().filter(|f| f.key == "drawn_by") {
            field.drawn = (!author.is_empty()).then(|| author.clone());
        }
    }
    let mut hits = hits(&drawing, if a.sheet == 1 { cells } else { &[] });
    // A datum symbol on a side of the part is that side's tap target, as the side's edges are.
    for g in core.plan().gdt.iter().filter(|g| g.kind == "datum_feature") {
        let side = g.source.strip_prefix("spec:faces/").and_then(|rest| rest.strip_suffix("/datum"));
        if let (Some(side), Some(hit)) = (side, hits.iter_mut().find(|h| h.id == g.id)) {
            hit.id = format!("{SIDE}{side}@datum");
        }
    }
    hits.extend(side_hits(core.analysis(), core.plan(), a.sheet));
    laps::lap("tap targets");
    let items = draftwright::items::items(core.plan())
        .into_iter()
        // Only what is drawn: a removed or covered item has nothing to tap.
        .filter(|i| i.sheet == a.sheet && i.state == draftwright::items::State::Shown && (i.kind == "dimension" || i.kind == "annotation"))
        .map(|i| {
            let group = if i.kind == "dimension" { "dimensions" } else { "annotations" };
            let entry = spec.get(group).and_then(|g| g.get(&i.id));
            let text = |field: &str| {
                entry
                    .and_then(|e| e.pointer(field))
                    .and_then(Value::as_str)
                    .map(str::to_string)
            };
            ItemInfo {
                label: i.label.clone().unwrap_or_default(),
                tolerance: text("/tolerance"),
                fit: text("/fit"),
                prefix: text("/text/prefix"),
                suffix: text("/text/suffix"),
                reference: text("/style").as_deref() == Some("reference"),
                id: i.id,
                kind: i.kind,
            }
        })
        .collect();

    let datums = core
        .spec()
        .faces
        .iter()
        .filter_map(|(side, face)| Some((side.clone(), face.datum.clone()?)))
        .collect();

    let every = draftwright::items::items(core.plan());
    let removed = core
        .spec()
        .dimensions
        .iter()
        .filter(|(_, d)| d.omit)
        .map(|(id, _)| Removed {
            label: every.iter().find(|i| i.id == *id).and_then(|i| i.label.clone()).unwrap_or_else(|| id.clone()),
            id: id.clone(),
        })
        .collect();

    laps::lap("items");

    Ok(DocView {
        removed,
        datums,
        origins: core.spec().origin.clone(),
        uncited_datums: core.spec().sheet.uncited_datums == Some(true),
        name: a.name.clone(),
        svg,
        sheet: a.sheet,
        sheets,
        problems,
        problem_total: all.len(),
        can_undo: a.edits > 0,
        exact: exact.is_some() && !as_chosen,
        size: if as_chosen { None } else { exact.or(preferred) },
        paper,
        scale,
        kept,
        overflow,
        hidden,
        units,
        shaded,
        faces: session.face_count(),
        features: session.feature_count(),
        title,
        hits,
        items,
    })
}

/// The prefix of a tap target that is a side of the part: `side:<side>@<where>`, where
/// `<where>` (the view and face, or `datum` for the side's datum symbol) keeps each target's ID
/// its own, so the page highlights the one tapped.
pub const SIDE: &str = "side:";
/// Half the width of the tap strip along a face seen edge-on, mm on the sheet.
const SIDE_REACH: f64 = 1.5;

/// The part axis a vector lies along, and whether it points up that axis; `None` for a vector
/// along no axis (the isometric's direction, which is not a unit vector either).
fn principal(v: [f64; 3]) -> Option<(usize, bool)> {
    let length = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if length <= 0.0 {
        return None;
    }
    let k = (0..3).find(|&k| v[k].abs() / length > 0.999)?;
    Some((k, v[k] > 0.0))
}

/// Tap targets for the sides of the part a datum can sit on (`bottom`, `top`, `left`, `right`,
/// `front`, `back`), from the faces themselves: each planar face that makes up a side, where a
/// principal view on *sheet* shows it. A face seen edge-on is a strip along its line, as long
/// as the face; a face turned to the viewer is its own outline's box (not in a section, which
/// shows the cut). Faces turned away, the isometric and detail views give none. SVG coordinates, as [`Hit`].
fn side_hits(analysis: &draftwright::model::Analysis, plan: &draftwright::model::Plan, sheet: u32) -> Vec<Hit> {
    let [lo, hi] = analysis.envelope;
    let centre = [0.5 * (lo[0] + hi[0]), 0.5 * (lo[1] + hi[1]), 0.5 * (lo[2] + hi[2])];
    // A side's faces, as the core finds them (`faces/<side>`): planes on that end of the
    // part's box, facing out. Each with its index and box.
    let faces_of = |k: usize, high: bool| {
        let at = if high { hi[k] } else { lo[k] };
        let tol = 1e-4 * (1.0 + (hi[k] - lo[k]).abs());
        analysis.face_attachments.iter().enumerate().filter_map(move |(index, f)| {
            let on_side = f.surface.as_deref() == Some("plane")
                && f.axis == Some(k)
                && f.support.first().is_some_and(|s| (s - at).abs() <= tol)
                && f.normal.is_some_and(|n| (n[k] > 0.0) == high);
            Some((index, f.bounds.filter(|_| on_side)?))
        })
    };
    let mut out = Vec::new();
    for view in plan.views.iter().filter(|v| v.sheet == sheet && v.detail.is_none()) {
        let (d, up) = (view.direction, view.up);
        // The view's right on the page: up × towards-the-viewer.
        let right = [
            up[1] * d[2] - up[2] * d[1],
            up[2] * d[0] - up[0] * d[2],
            up[0] * d[1] - up[1] * d[0],
        ];
        let (Some((kd, towards)), Some((ku, up_high)), Some((kr, right_high))) =
            (principal(d), principal(up), principal(right))
        else {
            continue; // the isometric
        };
        // A model coordinate along the view's right or up axis, on the page (sheet mm, y up).
        let page_x = |x: f64| view.at[0] + if right_high { 1.0 } else { -1.0 } * (x - centre[kr]) * view.scale;
        let page_y = |y: f64| view.at[1] + if up_high { 1.0 } else { -1.0 } * (y - centre[ku]) * view.scale;
        for (side, k, high) in draftwright::spec_gdt::SIDES {
            // Turned away from the viewer; or a section, which shows the cut, not the face.
            if k == kd && (high != towards || view.id.starts_with("v:section")) {
                continue;
            }
            for (index, b) in faces_of(k, high) {
                let (x0, x1) = (page_x(b[kr]), page_x(b[kr + 3]));
                let (y0, y1) = (page_y(b[ku]), page_y(b[ku + 3]));
                let (mut x0, mut x1, mut y0, mut y1) = (x0.min(x1), x0.max(x1), y0.min(y1), y0.max(y1));
                // Edge-on, the face is a line: give it a strip to tap.
                if k == kr {
                    (x0, x1) = (x0 - SIDE_REACH, x1 + SIDE_REACH);
                } else if k == ku {
                    (y0, y1) = (y0 - SIDE_REACH, y1 + SIDE_REACH);
                }
                out.push(Hit { id: format!("{SIDE}{side}@{}#{index}", view.id), rect: [x0, -y1, x1, -y0] });
            }
        }
    }
    out
}

/// Label boxes on the sheet: from the text runs of each plan label and from the title-block
/// cells. Sheet coordinates (y up) are converted to SVG coordinates (y down).
fn hits(
    drawing: &draftwright::output::Drawing,
    cells: &[draftwright::title_block::TitleCell],
) -> Vec<Hit> {
    use draftwright::output::{HAlign, VAlign};
    let sans = draftwright::title_block::font();
    let mut out = Vec::new();
    for annotation in &drawing.annotations {
        let mut rect: Option<[f64; 4]> = None;
        for &run in &annotation.runs {
            let Some(t) = drawing.text.get(run) else { continue };
            let width = if t.font.contains("Mono") {
                t.text.chars().count() as f64 * 0.6 * t.size
            } else {
                sans.advance(&t.text, t.size)
            };
            let x0 = match t.h_align {
                HAlign::Left => 0.0,
                HAlign::Center => -width / 2.0,
                HAlign::Right => -width,
            };
            let (y0, y1) = match t.v_align {
                VAlign::Baseline => (-0.25 * t.size, 0.8 * t.size),
                VAlign::Middle => (-0.5 * t.size, 0.5 * t.size),
            };
            let (sin, cos) = t.rotation.to_radians().sin_cos();
            for (dx, dy) in [(x0, y0), (x0 + width, y0), (x0, y1), (x0 + width, y1)] {
                let x = t.at[0] + dx * cos - dy * sin;
                let y = t.at[1] + dx * sin + dy * cos;
                let r = rect.get_or_insert([x, y, x, y]);
                *r = [r[0].min(x), r[1].min(y), r[2].max(x), r[3].max(y)];
            }
        }
        if let Some([x0, y0, x1, y1]) = rect {
            out.push(Hit { id: annotation.name.clone(), rect: [x0, -y1, x1, -y0] });
        }
    }
    for c in cells {
        let Some((key, _)) = draftwright::title_block::KEYS.iter().find(|(_, f)| *f == c.field) else {
            continue;
        };
        if *key == "sheet" {
            continue;
        }
        let [x0, y0, x1, y1] = c.cell;
        out.push(Hit {
            id: format!("title:{key}"),
            rect: [x0.min(x1), -y0.max(y1), x0.max(x1), -y0.min(y1)],
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Developer tool: what a session costs to build under different sheet settings: the
    /// core's automatic choice of paper and scale against a paper, or a paper and scale, given.
    /// `DW_STEP=part.step [DW_ONLY="paper A2"] cargo test --release --lib bench_open_specs -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn bench_open_specs() {
        let path = std::env::var("DW_STEP").expect("DW_STEP");
        let bytes = std::fs::read(&path).expect("read");
        let specs = [
            ("automatic", "{}"),
            ("paper A3", r#"{"sheet":{"size":"A3"}}"#),
            ("paper A2", r#"{"sheet":{"size":"A2"}}"#),
            ("paper A1", r#"{"sheet":{"size":"A1"}}"#),
            ("A2 at 1:2", r#"{"sheet":{"size":"A2","scale":"1:2"}}"#),
            ("A3 at 1:2", r#"{"sheet":{"size":"A3","scale":"1:2"}}"#),
        ];
        // The core remembers what it computed for a part within a process, so a later session
        // in the same run is cheaper than it would be alone: `DW_ONLY` runs one of them.
        let only = std::env::var("DW_ONLY").ok();
        for (what, spec) in specs {
            if only.as_deref().is_some_and(|o| o != what) {
                continue;
            }
            let started = std::time::Instant::now();
            let (part, analysis) = draftwright::model::analyse_native_bytes_part(&path, &bytes, None).expect("analyse");
            let analysed = started.elapsed().as_millis();
            let spec: draftwright::model::Spec = serde_json::from_str(spec).expect("spec");
            match CoreSession::new(part, analysis, spec) {
                Ok(session) => {
                    let sheet = &session.plan().sheet;
                    eprintln!(
                        "BENCH session {what}: {} ms after {analysed} ms of analysis; {} at {}, {} problems, sheet runs {}",
                        started.elapsed().as_millis() - analysed,
                        sheet.size,
                        sheet.scale,
                        session.state().problems.len(),
                        session.stats().sheet_steps,
                    );
                }
                Err(e) => eprintln!("BENCH session {what}: refused: {}", e.message),
            }
        }
    }

    /// Developer tool: the views and side tap targets of a STEP file.
    /// `DW_STEP=part.step cargo test --release --lib print_side_hits -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn print_side_hits() {
        let path = std::env::var("DW_STEP").expect("DW_STEP");
        let bytes = std::fs::read(&path).expect("read");
        let session = Session::open(&path, &bytes).expect("open");
        let plan = session.0.plan();
        for v in &plan.views {
            eprintln!("view {} sheet {} dir {:?} up {:?} at {:?} scale {} area {:?} detail {}", v.id, v.sheet, v.direction, v.up, v.at, v.scale, v.area, v.detail.is_some());
        }
        eprintln!("envelope {:?}", session.0.analysis().envelope);
        for h in side_hits(session.0.analysis(), plan, 1) {
            eprintln!("hit {} {:?}", h.id, h.rect);
        }
    }

    #[test]
    fn thumbnail_keeps_the_isometric_view() {
        let bytes = include_bytes!("../assets/plate.step");
        let session = Session::open("plate.step", bytes).expect("open");
        let svg = session.render_sheet_svg(1).expect("svg");
        let iso = session.0.plan().views.iter().find(|v| v.id == "v:iso").map(|v| v.area);
        assert!(iso.is_some());
        let all = crate::library::thumbnail_svg(&svg, None).unwrap();
        let only = crate::library::thumbnail_svg(&svg, iso).unwrap();
        let count = |t: &str| t.matches('<').count();
        // The isometric view has fewer edges than the whole sheet, and some.
        assert!(count(&only) > 4 && count(&only) < count(&all), "{} vs {}", count(&only), count(&all));
        // Its frame is the isometric view's area, not the sheet's.
        let width = |t: &str| -> f64 {
            let vb = t.split("viewBox=\"").nth(1).unwrap().split('"').next().unwrap();
            vb.split(' ').nth(2).unwrap().parse().unwrap()
        };
        assert!(width(&only) < width(&all) * 0.6, "{} vs {}", width(&only), width(&all));
    }

    fn actor(name: &str, bytes: &[u8]) -> Actor {
        Actor {
            session: Some(Session::open(name, bytes).expect("open")),
            name: name.into(),
            sheet: 1,
            entry: None,
            file: String::new(),
            bytes: Vec::new(),
            kept: 0,
            failed: None,
            edits: 0,
            chosen: None,
        }
    }

    fn choose(a: &mut Actor, size: Option<&str>, exact: bool) -> (DocView, Value) {
        choose_paper(a, size.map(str::to_string), exact).expect("paper");
        let spec = serde_json::to_value(a.session.as_ref().unwrap().0.spec()).unwrap();
        (view(a).expect("view"), spec["sheet"].clone())
    }

    #[test]
    fn a_chosen_paper_that_fits_is_kept() {
        // "Keep paper and scale" is one setting for the process: no test turns it off meanwhile.
        let _one = crate::tests::ONE.lock().unwrap_or_else(|e| e.into_inner());
        let mut a = actor("plate.step", include_bytes!("../assets/plate.step"));
        for exact in [false, true] {
            let (doc, sheet) = choose(&mut a, Some("A3"), exact);
            assert_eq!((doc.size.as_deref(), doc.exact, doc.paper.as_str()), (Some("A3"), true, "A3"));
            assert_eq!((sheet["size"].as_str(), sheet.get("preferred_max_size")), (Some("A3"), None));
            assert!(!doc.overflow);
        }
        // Automatic: the paper and scale it chose are fixed, and the drawing shows as Automatic.
        let (doc, sheet) = choose(&mut a, None, false);
        assert_eq!((doc.size, doc.exact), (None, false));
        assert_eq!((sheet["size"].as_str(), sheet.get("preferred_max_size")), (Some(doc.paper.as_str()), None));
        assert_eq!(sheet["scale"].as_str(), Some(doc.scale.as_str()));
    }

    #[test]
    fn a_drawing_too_dense_for_its_paper_grows_unless_exact() {
        let _one = crate::tests::ONE.lock().unwrap_or_else(|e| e.into_inner());
        let bytes = include_bytes!("../tests/fixtures/GRM-02_shaft.step");
        let mut a = actor("GRM-02_shaft.step", bytes);
        let (exact, _) = choose(&mut a, Some("A4"), true);
        assert!(exact.exact && exact.paper == "A4");
        let (doc, sheet) = choose(&mut a, Some("A4"), false);
        assert!(!doc.overflow, "{:?}", doc.problems.iter().map(|p| &p.code).collect::<Vec<_>>());
        if exact.overflow {
            assert_eq!((doc.size.as_deref(), doc.exact), (Some("A4"), false));
            // Runs off an exact A4: as the preferred size it grows to larger paper.
            assert_ne!(doc.paper, "A4", "larger paper");
            assert_eq!((sheet.get("size"), sheet["preferred_max_size"].as_str()), (None, Some("A4")));
            // One edit: undo returns to the exact A4.
            a.session.as_mut().unwrap().0.undo().unwrap();
            assert!(view(&mut a).unwrap().exact);
        } else {
            // draftwright 0.6 (from alpha.3) fits the shaft on A4 (none of the fixtures runs off
            // it any more): a chosen paper that fits is kept as it is.
            eprintln!("the shaft fits an exact A4: the growing branch is not reached");
            assert_eq!((doc.paper.as_str(), doc.exact), ("A4", true));
        }
    }

    /// The views of the shown sheet: id and page area.
    fn view_areas(a: &Actor) -> Vec<(String, [f64; 4])> {
        a.session.as_ref().unwrap().0.plan().views.iter().map(|v| (v.id.clone(), v.area)).collect()
    }

    #[test]
    fn a_title_block_edit_leaves_the_views_where_they_are() {
        let _one = crate::tests::ONE.lock().unwrap_or_else(|e| e.into_inner());
        for (name, bytes) in [
            ("GRM-02_shaft.step", &include_bytes!("../tests/fixtures/GRM-02_shaft.step")[..]),
            ("assembly.step", &include_bytes!("../tests/fixtures/assembly.step")[..]),
        ] {
            let mut a = actor(name, bytes);
            // As a drawing is built: the chosen paper and scale fixed before it is shown.
            assert!(fix_sheet(a.session.as_mut().unwrap()).is_some(), "{name}: the sheet fixed");
            view(&mut a).unwrap();
            let before = view_areas(&a);
            for (key, value) in [("title", "BRACKET, LONG NAME FOR THE TITLE"), ("material", "AL 6082-T6"), ("drawn_by", "M. S.")] {
                let op = Op { op: "set".into(), path: format!("sheet/title/{key}"), value: Some(Value::String(value.into())) };
                edit(&mut a, vec![op], "title").expect("title edit");
                let after = view_areas(&a);
                let moved: Vec<_> = before
                    .iter()
                    .zip(&after)
                    .filter(|(b, a)| b.0 != a.0 || b.1.iter().zip(&a.1).any(|(x, y)| (x - y).abs() > 1e-6))
                    .collect();
                assert!(moved.is_empty(), "{name}: setting {key} moved {moved:?}");
            }
            // Undo takes back the three edits and stops above the fixed sheet.
            for _ in 0..3 {
                a.session.as_mut().unwrap().0.undo().unwrap();
                a.edits -= 1;
            }
            assert_eq!(view_areas(&a), before);
        }
    }

    #[test]
    fn automatic_paper_is_fixed_in_the_same_edit() {
        let _one = crate::tests::ONE.lock().unwrap_or_else(|e| e.into_inner());
        let bytes = include_bytes!("../tests/fixtures/assembly.step");
        let mut a = actor("assembly.step", bytes);
        choose_paper(&mut a, Some("A2".into()), false).unwrap();
        choose_paper(&mut a, None, false).unwrap();
        let spec = serde_json::to_value(a.session.as_ref().unwrap().0.spec()).unwrap();
        let sheet = &a.session.as_ref().unwrap().0.plan().sheet;
        assert_eq!(spec["sheet"]["size"].as_str(), Some(sheet.size.to_string().as_str()));
        assert_eq!(spec["sheet"]["scale"].as_str(), Some(sheet.scale.to_string().as_str()));
        // One undo step back to A2 (it fits, so it is kept exact).
        a.session.as_mut().unwrap().0.undo().unwrap();
        let spec = serde_json::to_value(a.session.as_ref().unwrap().0.spec()).unwrap();
        assert_eq!((spec["sheet"]["size"].as_str(), spec["sheet"]["scale"].as_str()), (Some("A2"), None));
    }

    #[test]
    fn hit_boxes_cover_their_labels() {
        let bytes = include_bytes!("../assets/plate.step");
        let session = Session::open("plate.step", bytes).expect("open");
        let drawing = session.0.drawing_on_sheet(1).expect("drawing");
        let hits = hits(&drawing, &session.0.plan().sheet.title_cells);
        let find = |id: &str| hits.iter().find(|h| h.id == id).unwrap_or_else(|| panic!("{id}"));
        // In the SVG "60" is centred at (88.13, -54.0); "5" is rotated 90° at (157.13, -68.0).
        let [x0, y0, x1, y1] = find("d:envelope/width.length").rect;
        assert!(x0 < 88.13 && 88.13 < x1 && y0 < -54.5 && -54.5 < y1, "{:?}", [x0, y0, x1, y1]);
        let [x0, y0, x1, y1] = find("d:envelope/height.length").rect;
        assert!(x0 < 156.5 && 156.5 < x1 && y0 < -68.0 && -68.0 < y1, "{:?}", [x0, y0, x1, y1]);
        // The title cell lies on the sheet and is no wider than it.
        let [x0, y0, x1, y1] = find("title:title").rect;
        assert!(0.0 <= x0 && x1 <= 297.0 && -210.0 <= y0 && y1 <= 0.0 && x0 < x1 && y0 < y1, "{:?}", [x0, y0, x1, y1]);
        for t in &drawing.text {
            if t.text == "PLATE" {
                assert!(x0 < t.at[0] && t.at[0] < x1 && y0 < -t.at[1] && -t.at[1] < y1, "{:?} {:?}", t.at, [x0, y0, x1, y1]);
            }
        }
    }
}
