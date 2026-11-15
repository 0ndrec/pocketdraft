//! Pocketdraft's Rust layer: the draftwright drawing core behind a UniFFI interface.
//!
//! The Kotlin app (Jetpack Compose) calls these functions from background coroutines. Each
//! blocks until the engine thread (`engine.rs`, one long-lived thread with a large stack)
//! answers, so the UI thread never waits. The library root comes from [`init`].

#[allow(dead_code)]
mod cache;
#[allow(dead_code)]
mod engine;
#[allow(dead_code)]
mod fixes;
#[allow(dead_code)]
mod library;
mod log;
#[allow(dead_code)]
mod viewer;

use std::path::Path;
use std::sync::Mutex;

/// The problems of the last drawing handed to the UI: an answer typed into a problem reuses
/// the core's fix bundle for the same field (`fixes::ops_for`).
static PROBLEMS: Mutex<Vec<engine::ProblemRow>> = Mutex::new(Vec::new());

uniffi::setup_scaffolding!();

/// The planner makes very many small allocations. With the `mimalloc` feature they go to
/// mimalloc instead of the system's allocator (Android's is scudo): see docs/performance.md.
#[cfg(feature = "mimalloc")]
#[global_allocator]
static ALLOCATOR: mimalloc::MiMalloc = mimalloc::MiMalloc;

/// Any failure the UI shows: the core's message, readable as it is.
#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum DwError {
    /// `reason`, not `message`: Kotlin's Throwable already has a `message`.
    #[error("{reason}")]
    Failed { reason: String },
    /// An undo with nothing left to undo: news, not an error.
    #[error("Nothing to undo")]
    NothingToUndo,
}

impl From<String> for DwError {
    fn from(message: String) -> Self {
        if message == engine::NOTHING_TO_UNDO {
            DwError::NothingToUndo
        } else {
            DwError::Failed { reason: message }
        }
    }
}

type Answer<T> = Result<T, DwError>;

// --- records -------------------------------------------------------------------------

/// What the UI shows after each operation (`engine::DocView`).
#[derive(Debug, Clone, uniffi::Record)]
pub struct Doc {
    pub name: String,
    pub svg: String,
    pub sheet: u32,
    pub sheets: Vec<u32>,
    pub problems: Vec<Problem>,
    pub problem_total: u64,
    pub size: Option<String>,
    pub exact: bool,
    pub paper: String,
    /// Scale of the shown sheet, as drawn.
    pub scale: String,
    /// The paper and scale are given: edits keep them.
    pub kept: bool,
    pub overflow: bool,
    pub hidden: Option<bool>,
    pub faces: u64,
    pub features: u64,
    pub title: Vec<TitleField>,
    pub items: Vec<Item>,
    /// Label boxes for picking by tap, as the viewer page takes them (`hits:[…]`).
    pub hits_message: String,
    /// Dimensions the user removed from the drawing, to put back.
    pub removed: Vec<RemovedItem>,
    /// Datum letters on the sides of the part, by side (`bottom`, `top`, `left`, …).
    pub datums: std::collections::HashMap<String, String>,
    /// The end of the box dimensions are measured from (`min`, `max`), by axis (`x`, `y`, `z`);
    /// an axis not listed is chosen by the core.
    pub origins: std::collections::HashMap<String, String>,
    /// Datums nothing refers to are drawn too.
    pub uncited_datums: bool,
    /// Length units of the drawing (`mm`, or `in` with a core that draws in inches).
    pub units: String,
    /// The isometric view is drawn shaded.
    pub shaded: bool,
}

/// What the drawing core in this build can do. The interface shows a control only when the
/// core has its feature, so moving to a newer core lights the controls up; see
/// [`core_features`] for the one place that changes then.
#[derive(Debug, Clone, uniffi::Record)]
pub struct CoreFeatures {
    /// Paper families, each with its sizes from smallest to largest.
    pub papers: Vec<PaperFamily>,
    /// Length units a drawing can be made in; the first is the default.
    pub units: Vec<String>,
    /// The isometric view can be drawn shaded.
    pub shaded: bool,
    /// A sheet can be saved as DXF.
    pub dxf: bool,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct PaperFamily {
    /// `ISO`, `ANSI`, `ARCH`.
    pub name: String,
    pub sizes: Vec<String>,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct Problem {
    pub key: String,
    pub severity: String,
    pub code: String,
    pub message: String,
    /// A short heading for the code ("Label runs off the sheet").
    pub headline: String,
    /// The title-block field the user can fill in to close it, when user data closes it.
    pub ask: Option<Field>,
    pub refs: Vec<String>,
    pub fixes: Vec<Fix>,
}

/// An editable field: a title-block cell (`key`: `sheet/title/<key>`) or a property of a
/// dimension or label (`dim:<name>`).
#[derive(Debug, Clone, uniffi::Record)]
pub struct Field {
    pub key: String,
    pub label: String,
    pub hint: String,
    /// Common values, shown as chips.
    pub suggestions: Vec<String>,
}

fn field(f: &fixes::Field) -> Field {
    Field {
        key: f.key.to_string(),
        label: f.label.to_string(),
        hint: f.hint.to_string(),
        suggestions: f.suggestions.iter().map(|s| s.to_string()).collect(),
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct TitleChange {
    pub key: String,
    pub value: String,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct Fix {
    pub description: String,
    /// How much of the drawing changes ("Changes one item", …).
    pub scope: String,
    /// The fix's operations, as JSON: handed back to [`apply_ops`] as they are.
    pub ops_json: String,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct TitleField {
    pub key: String,
    pub drawn: Option<String>,
    pub authored: Option<String>,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct Item {
    pub id: String,
    pub kind: String,
    pub label: String,
    /// The editor's heading.
    pub title: String,
    pub tolerance: Option<String>,
    pub fit: Option<String>,
    pub prefix: Option<String>,
    pub suffix: Option<String>,
    pub reference: bool,
}

/// A removed dimension: its ID and the text it would print.
#[derive(Debug, Clone, uniffi::Record)]
pub struct RemovedItem {
    pub id: String,
    pub label: String,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct Entry {
    pub id: String,
    pub title: String,
    /// "2 h ago · 27 faces · 12 problems · edited".
    pub summary: String,
    /// The isometric thumbnail (SVG), when one was made.
    pub thumbnail_svg: Option<String>,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct PartRow {
    pub index: u32,
    pub name: String,
    pub faces: u64,
    pub instances: u64,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct ModelFile {
    pub name: String,
    pub bytes: Vec<u8>,
}

fn doc(d: engine::DocView) -> Doc {
    *PROBLEMS.lock().unwrap_or_else(|e| e.into_inner()) = d.problems.clone();
    let items = d
        .items
        .iter()
        .map(|i| Item {
            id: i.id.clone(),
            kind: i.kind.clone(),
            label: i.label.clone(),
            title: fixes::item_title(i),
            tolerance: i.tolerance.clone(),
            fit: i.fit.clone(),
            prefix: i.prefix.clone(),
            suffix: i.suffix.clone(),
            reference: i.reference,
        })
        .collect();
    Doc {
        removed: d.removed.into_iter().map(|r| RemovedItem { id: r.id, label: r.label }).collect(),
        datums: d.datums.into_iter().collect(),
        origins: d.origins.into_iter().collect(),
        uncited_datums: d.uncited_datums,
        units: d.units,
        shaded: d.shaded,
        hits_message: viewer::hits(&d.hits),
        problems: d
            .problems
            .iter()
            .map(|p| Problem {
                key: p.key.clone(),
                severity: p.severity.clone(),
                code: p.code.clone(),
                message: p.message.clone(),
                headline: fixes::headline(&p.code),
                ask: fixes::ask_for(p).map(field),
                refs: p.refs.clone(),
                fixes: p
                    .fixes
                    .iter()
                    .map(|f| Fix {
                        description: f.description.clone(),
                        scope: fixes::scope_label(&f.scope).to_string(),
                        ops_json: serde_json::to_string(&f.ops).unwrap_or_else(|_| "[]".into()),
                    })
                    .collect(),
            })
            .collect(),
        title: d
            .title
            .iter()
            .map(|t| TitleField { key: t.key.clone(), drawn: t.drawn.clone(), authored: t.authored.clone() })
            .collect(),
        items,
        name: d.name,
        svg: d.svg,
        sheet: d.sheet,
        sheets: d.sheets,
        problem_total: d.problem_total as u64,
        size: d.size,
        exact: d.exact,
        paper: d.paper,
        scale: d.scale,
        kept: d.kept,
        overflow: d.overflow,
        hidden: d.hidden,
        faces: d.faces as u64,
        features: d.features as u64,
    }
}

fn answer(r: Result<engine::DocView, String>) -> Answer<Doc> {
    r.map(doc).map_err(DwError::from)
}

fn root() -> Answer<&'static Path> {
    library::root().ok_or_else(|| DwError::Failed { reason: "The library is not set up".into() })
}

// --- set-up and library ----------------------------------------------------------------

/// Set the app's private files directory (`Context.getFilesDir()`); the library lives in
/// `<files dir>/library`. Call once at start-up, before anything else.
#[uniffi::export]
pub fn init(files_dir: String) {
    library::init(Path::new(&files_dir));
    engine::set_keep_sheet(keep_sheet());
    engine::set_credit(credits_draftwright());
}

/// The library, newest first.
#[uniffi::export]
pub fn library_entries() -> Vec<Entry> {
    let Some(root) = library::root() else {
        return Vec::new();
    };
    let now = library::now();
    library::list(root)
        .into_iter()
        .map(|e| Entry {
            thumbnail_svg: library::thumbnail(root, &e.id),
            summary: library::summary(&e, now),
            title: e.title(),
            id: e.id,
        })
        .collect()
}

/// Remove an entry: its model, edits, thumbnail and caches.
#[uniffi::export]
pub fn remove_entry(id: String) -> Answer<()> {
    engine::forget(id.clone())?;
    library::remove(root()?, &id).map_err(|e| DwError::Failed { reason: e.to_string() })
}

/// The "Exact size" setting: a chosen paper is kept even when the drawing does not fit it.
#[uniffi::export]
pub fn exact_paper() -> bool {
    library::root().map(|r| library::settings(r).exact_paper).unwrap_or(false)
}

#[uniffi::export]
pub fn set_exact_paper(exact: bool) -> Answer<()> {
    let root = root()?;
    let settings = library::Settings { exact_paper: exact, ..library::settings(root) };
    library::save_settings(root, &settings).map_err(|e| DwError::Failed { reason: e.to_string() })
}

/// "Keep paper and scale" (on by default): edits plan on the paper and scale the drawing has,
/// instead of choosing them again each time.
#[uniffi::export]
pub fn keep_sheet() -> bool {
    library::root().map(|r| library::settings(r).keeps_sheet()).unwrap_or(true)
}

#[uniffi::export]
pub fn set_keep_sheet(on: bool) -> Answer<()> {
    engine::set_keep_sheet(on);
    let root = root()?;
    let settings = library::Settings { keep_sheet: Some(on), ..library::settings(root) };
    library::save_settings(root, &settings).map_err(|e| DwError::Failed { reason: e.to_string() })
}

/// "Credit draftwright in Drawn by" (on by default). Off, the title block's "drawn by" cell
/// shows the author alone, or nothing; show the sheet again to see it.
#[uniffi::export]
pub fn credits_draftwright() -> bool {
    library::root().map(|r| library::settings(r).credits()).unwrap_or(true)
}

#[uniffi::export]
pub fn set_credit(on: bool) -> Answer<()> {
    engine::set_credit(on);
    let root = root()?;
    let settings = library::Settings { credit: Some(on), ..library::settings(root) };
    library::save_settings(root, &settings).map_err(|e| DwError::Failed { reason: e.to_string() })
}

/// A title-block value every new drawing starts with.
#[derive(Debug, Clone, uniffi::Record)]
pub struct DefaultField {
    pub field: Field,
    pub value: String,
}

/// The title-block defaults for new drawings, in editor order.
#[uniffi::export]
pub fn title_defaults() -> Vec<DefaultField> {
    let stored = library::root().map(library::defaults).unwrap_or_default();
    fixes::FIELDS
        .iter()
        .filter(|f| library::DEFAULT_KEYS.contains(&f.key))
        .map(|f| DefaultField { field: field(f), value: stored.get(f.key).cloned().unwrap_or_default() })
        .collect()
}

/// Store the title-block defaults (`key`, `value`); an empty value clears one.
#[uniffi::export]
pub fn set_title_defaults(values: Vec<TitleChange>) -> Answer<()> {
    let values = values.into_iter().map(|c| (c.key, c.value)).collect();
    library::save_defaults(root()?, &values).map_err(|e| DwError::Failed { reason: e.to_string() })
}

// --- opening ---------------------------------------------------------------------------

/// Whether the bytes look like a STEP file (or a gzipped one).
#[uniffi::export]
pub fn looks_like_step(bytes: Vec<u8>) -> bool {
    engine::looks_like_step(&bytes)
}

/// The parts of a STEP file; more than one is an assembly.
#[uniffi::export]
pub fn parts(name: String, bytes: Vec<u8>) -> Answer<Vec<PartRow>> {
    engine::parts(name, bytes)
        .map(|rows| {
            rows.into_iter()
                .map(|p| PartRow { index: p.index as u32, name: p.name, faces: p.faces as u64, instances: p.instances as u64 })
                .collect()
        })
        .map_err(DwError::from)
}

/// Open a STEP file, or one part of it (`part`: index and name).
#[uniffi::export]
pub fn open_file(name: String, bytes: Vec<u8>, part_index: Option<u32>, part_name: Option<String>) -> Answer<Doc> {
    let part = part_index.map(|i| (i as usize, part_name.unwrap_or_else(|| format!("part {i}"))));
    answer(engine::open(engine::Opening { name, bytes, part, replaces: None }))
}

/// Open a library entry with its saved edits.
#[uniffi::export]
pub fn open_saved(id: String) -> Answer<Doc> {
    answer(engine::open_saved(id))
}

// --- the open drawing ------------------------------------------------------------------

#[uniffi::export]
pub fn show_sheet(sheet: u32) -> Answer<Doc> {
    answer(engine::show_sheet(sheet))
}

/// Paper size (`None`: automatic); see `engine::set_paper` for exact and preferred paper.
#[uniffi::export]
pub fn set_paper(size: Option<String>, exact: bool) -> Answer<Doc> {
    answer(engine::set_paper(size, exact))
}

/// Hidden lines on or off (`None`: the view's own convention).
#[uniffi::export]
pub fn set_hidden(hidden: Option<bool>) -> Answer<Doc> {
    let value = hidden.map_or(serde_json::Value::Null, serde_json::Value::Bool);
    answer(engine::set("sheet/hidden", value))
}

/// A datum letter on a side of the part (`bottom`, `top`, `left`, `right`, `front`, `back`);
/// `None` takes the side's datum away. A letter another side holds moves here.
#[uniffi::export]
pub fn set_datum(side: String, letter: Option<String>) -> Answer<Doc> {
    answer(engine::set_datum(side, letter))
}

/// The end of the part's box (`min`, `max`) that dimensions along an axis (`x`, `y`, `z`) are
/// measured from; `None`: the datum face across the axis, else the minimum.
#[uniffi::export]
pub fn set_origin(axis: String, end: Option<String>) -> Answer<Doc> {
    answer(engine::set_origin(axis, end))
}

/// Draw every declared datum, or only those a dimension or tolerance refers to.
#[uniffi::export]
pub fn set_uncited_datums(shown: bool) -> Answer<Doc> {
    let value = if shown { serde_json::Value::Bool(true) } else { serde_json::Value::Null };
    answer(engine::set("sheet/uncited_datums", value))
}

/// Apply operations (a fix's `ops_json`) as one edit.
#[uniffi::export]
pub fn apply_ops(ops_json: String) -> Answer<Doc> {
    let ops: Vec<draftwright::items::Op> =
        serde_json::from_str(&ops_json).map_err(|e| DwError::Failed { reason: format!("Invalid operations: {e}") })?;
    answer(engine::apply(ops))
}

/// Edit a dimension or label: only the fields that changed are written; an empty field
/// gives back the general tolerance or the automatic text.
#[uniffi::export]
pub fn edit_item(item: Item, tolerance: String, fit: String, prefix: String, suffix: String) -> Answer<Doc> {
    let info = engine::ItemInfo {
        id: item.id,
        kind: item.kind,
        label: item.label,
        tolerance: item.tolerance,
        fit: item.fit,
        prefix: item.prefix,
        suffix: item.suffix,
        reference: item.reference,
    };
    let ops = fixes::item_ops(&info, &fixes::ItemValues { tolerance, fit, prefix, suffix });
    if ops.is_empty() {
        return Err(DwError::Failed { reason: "No changes".into() });
    }
    answer(engine::apply(ops))
}

/// Remove a dimension from the drawing: one edit, undone like any other.
#[uniffi::export]
pub fn remove_dimension(id: String) -> Answer<Doc> {
    answer(engine::set_omitted(id, true))
}

/// Put a removed dimension back.
#[uniffi::export]
pub fn restore_dimension(id: String) -> Answer<Doc> {
    answer(engine::set_omitted(id, false))
}

/// Title-block fields in editor order.
#[uniffi::export]
pub fn title_fields() -> Vec<Field> {
    fixes::FIELDS.iter().map(field).collect()
}

/// The fields of a dimension (`kind` "dimension") or a text label.
#[uniffi::export]
pub fn item_fields(kind: String) -> Vec<Field> {
    let mut fields = Vec::new();
    if kind == "dimension" {
        fields.push(field(&fixes::TOLERANCE));
        fields.push(field(&fixes::FIT));
    }
    fields.push(field(&fixes::SUFFIX));
    fields.push(field(&fixes::PREFIX));
    fields
}

/// Fill in the field a problem asks for (its `ask`): the core's fix bundle for the field is
/// reused with the typed value, so the edit stays local.
#[uniffi::export]
pub fn answer_problem(problem_key: String, value: String) -> Answer<Doc> {
    let problem = PROBLEMS.lock().unwrap_or_else(|e| e.into_inner()).iter().find(|p| p.key == problem_key).cloned();
    let Some(problem) = problem else {
        return Err(DwError::Failed { reason: "The problem is gone; look again".into() });
    };
    let Some(f) = fixes::ask_for(&problem) else {
        return Err(DwError::Failed { reason: "This problem takes no value".into() });
    };
    answer(engine::apply(fixes::ops_for(Some(&problem), f.key, &value)))
}

/// Change title-block fields as one edit.
#[uniffi::export]
pub fn set_titles(changes: Vec<TitleChange>) -> Answer<Doc> {
    let problems = PROBLEMS.lock().unwrap_or_else(|e| e.into_inner()).clone();
    let changes: Vec<(&str, String)> = changes.iter().map(|c| (c.key.as_str(), c.value.clone())).collect();
    let ops = fixes::title_ops(&problems, &changes);
    if ops.is_empty() {
        return Err(DwError::Failed { reason: "No changes".into() });
    }
    answer(engine::apply(ops))
}

/// Whether the text has letters the drawing font lacks (it is Latin-only).
#[uniffi::export]
pub fn unsupported_text(value: String) -> bool {
    fixes::unsupported_text(&value)
}

#[uniffi::export]
pub fn undo() -> Answer<Doc> {
    answer(engine::undo())
}

/// The drawing as PDF, every sheet.
#[uniffi::export]
pub fn pdf() -> Answer<Vec<u8>> {
    engine::pdf().map_err(DwError::from)
}

/// The shown sheet as DXF.
#[uniffi::export]
pub fn dxf() -> Answer<Vec<u8>> {
    engine::dxf().map_err(DwError::from)
}

// --- what the core can do --------------------------------------------------------------

/// What the drawing core can do: with draftwright 0.6, ISO, ANSI and ARCH paper,
/// millimetres and inches, and a shaded isometric view.
#[uniffi::export]
pub fn core_features() -> CoreFeatures {
    let family = |name: &str, pages: &[(&str, f64, f64)]| PaperFamily {
        name: name.into(),
        sizes: pages.iter().map(|p| p.0.to_string()).collect(),
    };
    CoreFeatures {
        papers: vec![
            family("ISO", &draftwright::layout::PAGES),
            family("ANSI", &draftwright::layout::ANSI_PAGES),
            family("ARCH", &draftwright::layout::ARCH_PAGES),
        ],
        units: vec!["mm".into(), "in".into()],
        shaded: true,
        dxf: true,
    }
}

fn unsupported(what: &str) -> DwError {
    DwError::Failed { reason: format!("This version of the drawing core has no {what}") }
}

/// The drawing's length units (`mm`, `in`); refused when the core has no such units.
#[uniffi::export]
pub fn set_units(units: String) -> Answer<Doc> {
    if !core_features().units.contains(&units) {
        return Err(unsupported(&format!("“{units}” units")));
    }
    answer(engine::set("units", serde_json::Value::String(units)))
}

/// The isometric view shaded or as lines.
#[uniffi::export]
pub fn set_shaded(on: bool) -> Answer<Doc> {
    if !core_features().shaded {
        return Err(unsupported("shaded views"));
    }
    answer(engine::set("views/iso/style", serde_json::Value::String(if on { "shaded" } else { "wireframe" }.into())))
}

/// The open drawing's STEP file, for sharing.
#[uniffi::export]
pub fn model() -> Answer<ModelFile> {
    engine::model().map(|(name, bytes)| ModelFile { name, bytes }).map_err(DwError::from)
}

// --- the viewer page -------------------------------------------------------------------

/// The drawing viewer page, starting in the theme's background (`#RRGGBB`).
#[uniffi::export]
pub fn viewer_html(background: String) -> String {
    viewer::html(&background)
}

/// The page message that shows a sheet: fitted to the screen (a new drawing or sheet) or
/// kept where it is (an edit).
#[uniffi::export]
pub fn viewer_show(fit: bool, svg: String) -> String {
    viewer::message(fit, &svg)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The tests share the one engine thread and its open drawing: one at a time.
    static ONE: Mutex<()> = Mutex::new(());

    #[test]
    fn an_assembly_offers_the_cores_fixes() {
        let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
        let bytes = include_bytes!("../tests/fixtures/assembly.step").to_vec();
        let d = open_file("assembly.step".into(), bytes, None, None).expect("open");
        let with_fixes: Vec<_> = d.problems.iter().filter(|p| !p.fixes.is_empty()).map(|p| p.code.clone()).collect();
        eprintln!("{} problems, fixes on {:?}", d.problems.len(), with_fixes);
        assert!(with_fixes.len() > 1, "{with_fixes:?}");
    }

    #[test]
    fn fixes_survive_a_reopen() {
        let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
        let dir = std::env::temp_dir().join(format!("dw-ffi-plans-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        init(dir.to_string_lossy().into_owned());
        let bytes = include_bytes!("../tests/fixtures/assembly.step").to_vec();
        for round in 0..2 {
            let d = open_file("assembly.step".into(), bytes.clone(), None, None).expect("open");
            let n = d.problems.iter().filter(|p| !p.fixes.is_empty()).count();
            eprintln!("round {round}: {} problems, {n} with fixes", d.problems.len());
        }
    }

    #[test]
    fn datums_and_origins_reach_the_drawing() {
        let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
        let dir = std::env::temp_dir().join(format!("dw-ffi-datums-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        init(dir.to_string_lossy().into_owned());
        let bytes = include_bytes!("../tests/fixtures/assembly.step").to_vec();
        let d = open_file("assembly.step".into(), bytes, None, None).expect("open");
        assert!(d.datums.is_empty() && d.origins.is_empty() && !d.uncited_datums);

        // The plate's sides are tap targets: the bottom edge in the front view, and so on.
        for side in ["bottom", "top", "left", "right", "front", "back"] {
            assert!(d.hits_message.contains(&format!("\"side:{side}@")), "{side}");
        }
        let edges = d.hits_message.matches("\"side:bottom@").count();
        // Never on the isometric view: its edges are no side of the part.
        assert!(!d.hits_message.contains("@v:iso"), "{}", d.hits_message);

        let d = set_datum("bottom".into(), Some("A".into())).expect("datum A");
        // The drawn datum symbol is one more target for its side.
        assert_eq!(d.hits_message.matches("\"side:bottom@").count(), edges + 1, "{}", d.hits_message);
        assert_eq!(d.datums.get("bottom").map(String::as_str), Some("A"));
        // A letter names one face: giving it to another side moves it.
        let d = set_datum("top".into(), Some("A".into())).expect("move A");
        assert_eq!(d.datums.get("top").map(String::as_str), Some("A"));
        assert!(!d.datums.contains_key("bottom"), "{:?}", d.datums);
        let d = set_datum("top".into(), None).expect("no datum");
        assert!(d.datums.is_empty(), "{:?}", d.datums);

        let d = set_origin("x".into(), Some("max".into())).expect("origin");
        assert_eq!(d.origins.get("x").map(String::as_str), Some("max"));
        let d = set_origin("x".into(), None).expect("origin back");
        assert!(d.origins.is_empty());
        // Nothing to take away is not an error.
        set_origin("y".into(), None).expect("no origin to remove");

        assert!(set_uncited_datums(true).expect("uncited").uncited_datums);
        assert!(!set_uncited_datums(false).expect("cited only").uncited_datums);
        assert!(set_datum("bottom".into(), Some("a1".into())).is_err(), "a bad letter is refused");
    }

    #[test]
    fn a_removed_dimension_leaves_the_sheet_and_comes_back() {
        let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
        let dir = std::env::temp_dir().join(format!("dw-ffi-removed-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        init(dir.to_string_lossy().into_owned());
        let bytes = include_bytes!("../tests/fixtures/GRM-02_shaft.step").to_vec();
        let d = open_file("GRM-02_shaft.step".into(), bytes, None, None).expect("open");
        assert!(d.removed.is_empty());
        let item = d.items.iter().find(|i| i.kind == "dimension").expect("a dimension").clone();
        let shown = |d: &Doc| d.items.iter().any(|i| i.id == item.id);

        let d = remove_dimension(item.id.clone()).expect("remove");
        assert!(!shown(&d), "{} is still an item", item.id);
        assert_eq!(d.removed.len(), 1);
        assert_eq!(d.removed[0].id, item.id);
        assert_eq!(d.removed[0].label, item.label);

        let d = restore_dimension(item.id.clone()).expect("restore");
        assert!(shown(&d) && d.removed.is_empty());

        // Some of the core's ready fixes keep the layout ("changes one item"), and the core then
        // refuses removals: the app releases the guard for such an edit.
        let ops = d
            .problems
            .iter()
            .flat_map(|p| p.fixes.iter())
            .find(|f| f.ops_json.contains("keep_layout"))
            .map(|f| f.ops_json.clone())
            .expect("a fix that keeps the layout");
        let d = apply_ops(ops).expect("keep the layout");
        let other = d.items.iter().find(|i| i.kind == "dimension").expect("a dimension").clone();
        let d = remove_dimension(other.id.clone()).expect("remove with the layout kept");
        assert!(d.removed.iter().any(|r| r.id == other.id));
        let d = restore_dimension(other.id).expect("restore");
        let shown = |d: &Doc| d.items.iter().any(|i| i.id == item.id);
        // Undo takes a removal back as well.
        remove_dimension(item.id.clone()).expect("remove again");
        let d = undo().expect("undo");
        assert!(shown(&d) && d.removed.is_empty());
    }

    /// Developer tool: where the time goes when a model is opened and edited, through the same
    /// calls the app makes. The engine's notes (`edit: … ms (plan …, svg …)`) give each step's
    /// breakdown; the `BENCH` lines the time the caller waits.
    /// `DW_STEP=part.step [DW_LIB=dir] [DW_PIN=size|scale|keep] [DW_STOP=1] [DW_BATCH=n] cargo test --release --lib bench_edits -- --ignored --nocapture`
    /// - `DW_LIB`: a library directory to keep between runs (the second run opens from its caches);
    /// - `DW_PIN`: fix the planned paper (`size`), the scale too (`scale`), or keep the layout
    ///   (`keep`: `sheet/keep_layout` with one sheet) before the edits;
    /// - `DW_STOP`: stop before the edits (to time opening alone);
    /// - `DW_BATCH`: instead of the edits, remove *n* dimensions one by one and then in one edit.
    #[test]
    #[ignore]
    fn bench_edits() {
        use std::time::Instant;
        let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
        let path = std::env::var("DW_STEP").expect("DW_STEP");
        let dir = match std::env::var("DW_LIB") {
            Ok(dir) => std::path::PathBuf::from(dir),
            Err(_) => {
                let dir = std::env::temp_dir().join(format!("dw-ffi-bench-{}", std::process::id()));
                let _ = std::fs::remove_dir_all(&dir);
                dir
            }
        };
        init(dir.to_string_lossy().into_owned());
        let bytes = std::fs::read(&path).expect("read");
        let name = std::path::Path::new(&path).file_name().unwrap().to_string_lossy().into_owned();

        let step = |what: &str, run: &mut dyn FnMut() -> Answer<Doc>| -> Option<Doc> {
            let started = Instant::now();
            let result = run();
            let ms = started.elapsed().as_millis();
            match result {
                Ok(d) => {
                    eprintln!("BENCH {what}: {ms} ms; svg {} KB, {} items, {} problems, paper {}", d.svg.len() / 1024, d.items.len(), d.problem_total, d.paper);
                    Some(d)
                }
                Err(e) => {
                    eprintln!("BENCH {what}: {ms} ms; REFUSED {e:?}");
                    None
                }
            }
        };
        let d = step("open", &mut || open_file(name.clone(), bytes.clone(), None, None)).expect("open");
        // A drawing shown from its snapshot has its session built behind it: wait for it.
        let d = step("first call (waits for the session)", &mut || show_sheet(1)).unwrap_or(d);

        let set = |path: &str, value: serde_json::Value| draftwright::items::Op { op: "set".into(), path: path.into(), value: Some(value) };
        let (size, scale) = engine::planned().expect("planned");
        eprintln!("BENCH planned {size} at {scale}");
        let pin = std::env::var("DW_PIN").unwrap_or_default();
        let mut ops = Vec::new();
        if ["size", "scale", "keep"].contains(&pin.as_str()) {
            ops.push(set("sheet/size", serde_json::json!(size)));
            ops.push(set("sheet/preferred_max_size", serde_json::Value::Null));
        }
        if ["scale", "keep"].contains(&pin.as_str()) {
            ops.push(set("sheet/scale", serde_json::json!(scale)));
        }
        if pin == "keep" {
            ops.push(set("sheets/max", serde_json::json!(1)));
            ops.push(set("sheet/keep_layout", serde_json::json!(true)));
        }
        if !ops.is_empty() {
            let json = serde_json::to_string(&ops).unwrap();
            step(&format!("pin {pin}"), &mut || apply_ops(json.clone()));
        }

        if std::env::var("DW_STOP").is_ok() {
            return;
        }
        let dims: Vec<Item> = d.items.iter().filter(|i| i.kind == "dimension").cloned().collect();
        let dim = |n: usize| dims[n % dims.len()].clone();
        if let Ok(n) = std::env::var("DW_BATCH") {
            // Removals as separate edits and as one edit: a plan costs the same for any number
            // of operations. Two different sets of dimensions, so that neither run meets a
            // drawing the core has planned before (it remembers those within a process).
            let n: usize = n.parse().expect("DW_BATCH is a count");
            let pick = |from: usize| -> Vec<String> { (0..n).map(|k| dim(from + k * dims.len() / (n + 1)).id).collect() };
            let omit = |id: &String| format!("dimensions/{}/omit", draftwright::items::key(id));
            let started = Instant::now();
            for id in pick(3) {
                step("remove one", &mut || remove_dimension(id.clone()));
            }
            eprintln!("BENCH {n} removals, one edit each: {} ms", started.elapsed().as_millis());
            let all: Vec<_> = pick(7).iter().map(|id| set(&omit(id), serde_json::json!(true))).collect();
            let json = serde_json::to_string(&all).unwrap();
            step(&format!("remove {n} others in one edit"), &mut || apply_ops(json.clone()));
            return;
        }
        step("tolerance on a dimension", &mut || edit_item(dim(0), "±0.1".into(), String::new(), String::new(), String::new()));
        step("tolerance on another", &mut || edit_item(dim(dims.len() / 2), "±0.05".into(), String::new(), String::new(), String::new()));
        step("remove a dimension", &mut || remove_dimension(dim(dims.len() / 3).id));
        step("restore it", &mut || restore_dimension(dim(dims.len() / 3).id));
        step("title block field", &mut || set_titles(vec![TitleChange { key: "material".into(), value: "AISI 304".into() }]));
        step("datum A on the bottom", &mut || set_datum("bottom".into(), Some("A".into())));
        step("hidden lines off", &mut || set_hidden(Some(false)));
        step("undo", &mut || undo());
        step("paper A2", &mut || set_paper(Some("A2".into()), false));
        step("paper automatic", &mut || set_paper(None, false));
    }

    #[test]
    fn the_first_edit_keeps_the_paper_and_scale() {
        let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
        let dir = std::env::temp_dir().join(format!("dw-ffi-keep-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        init(dir.to_string_lossy().into_owned());
        assert!(keep_sheet(), "on by default");
        // The library is set once per process: other tests may have edited this drawing there.
        for e in library_entries() {
            remove_entry(e.id).expect("remove");
        }
        let bytes = include_bytes!("../tests/fixtures/assembly.step").to_vec();
        let d = open_file("assembly.step".into(), bytes, None, None).expect("open");
        assert!(!d.kept && d.size.is_none());
        let (paper, scale) = (d.paper.clone(), d.scale.clone());

        // An edit keeps the sheet as it was laid out: same paper, same scale.
        let item = d.items.iter().find(|i| i.kind == "dimension").unwrap().clone();
        let d = edit_item(item, "±0.1".into(), String::new(), String::new(), String::new()).expect("edit");
        assert!(d.kept, "the sheet is kept");
        assert_eq!((d.paper.as_str(), d.scale.as_str(), d.size.as_deref()), (paper.as_str(), scale.as_str(), Some(paper.as_str())));
        // Undo takes the edit back with the kept sheet.
        let d = undo().expect("undo");
        assert!(!d.kept && d.size.is_none());

        // Another paper takes its own scale; the next edit keeps it.
        let d = set_paper(Some("A3".into()), true).expect("A3");
        assert!(!d.kept, "the core chose the scale for A3");
        let d = set_hidden(Some(false)).expect("hidden");
        assert!(d.kept && d.paper == "A3");
        // Automatic lays it out again.
        let d = set_paper(None, false).expect("automatic");
        assert!(!d.kept && d.size.is_none());

        // Off: edits leave the choice to the core.
        set_keep_sheet(false).expect("off");
        assert!(!keep_sheet());
        let d = set_hidden(Some(true)).expect("hidden");
        assert!(!d.kept && d.size.is_none());
        set_keep_sheet(true).expect("on");
    }

    #[test]
    fn the_core_features_match_the_core() {
        let f = core_features();
        assert_eq!(f.papers.len(), 3);
        assert_eq!(f.papers[0].sizes, ["A4", "A3", "A2", "A1", "A0"]);
        assert_eq!(f.papers[1].sizes[1], "ANSI B");
        assert_eq!(f.papers[2].sizes[4], "ARCH E");
        assert_eq!(f.units, ["mm", "in"]);
        assert!(f.dxf && f.shaded);
        assert!(set_units("feet".into()).is_err(), "no such units");
    }

    #[test]
    fn inches_us_paper_and_shading_reach_the_drawing() {
        let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
        let dir = std::env::temp_dir().join(format!("dw-ffi-units-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        init(dir.to_string_lossy().into_owned());
        let bytes = include_bytes!("../tests/fixtures/assembly.step").to_vec();
        let d = open_file("assembly.step".into(), bytes, None, None).expect("open");
        assert_eq!(d.units, "mm");
        assert!(!d.shaded);

        let d = set_units("in".into()).expect("inches");
        assert_eq!(d.units, "in");
        let d = set_paper(Some("ANSI B".into()), true).expect("ANSI B");
        assert_eq!(d.paper, "ANSI B");
        let d = set_shaded(true).expect("shaded");
        assert!(d.shaded && d.units == "in");
        assert!(dxf().expect("dxf").len() > 1000);
        let d = set_shaded(false).expect("lines");
        assert!(!d.shaded);
    }

    #[test]
    fn the_credit_leaves_the_drawn_by_cell_when_turned_off() {
        let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
        let dir = std::env::temp_dir().join(format!("dw-ffi-credit-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        init(dir.to_string_lossy().into_owned());
        for e in library_entries() {
            remove_entry(e.id).expect("remove");
        }
        assert!(credits_draftwright(), "on by default");
        let bytes = include_bytes!("../tests/fixtures/assembly.step").to_vec();
        let d = open_file("assembly.step".into(), bytes, None, None).expect("open");
        let drawn = |d: &Doc| d.title.iter().find(|t| t.key == "drawn_by").and_then(|t| t.drawn.clone());
        let mentions = |d: &Doc| d.svg.matches("draftwright").count();
        assert_eq!(drawn(&d).as_deref(), Some("draftwright"));
        let (with, link) = (mentions(&d), d.svg.matches("<a ").count());
        let whole = d.svg.len();
        assert_eq!(link, 1, "the credit's link");

        set_credit(false).expect("off");
        let d = show_sheet(1).expect("again");
        assert_eq!(drawn(&d), None, "nothing in the cell without an author");
        // The cell's text, and the two addresses in its link.
        assert_eq!(mentions(&d), with - 3, "the cell's text is gone");
        assert_eq!(d.svg.matches("<a ").count(), 0, "and its link");
        // The label's glyphs are outlines: eleven letters of them are gone too.
        assert!(d.svg.len() + 1500 < whole, "{} against {whole}", d.svg.len());
        assert!(pdf().expect("pdf").starts_with(b"%PDF"));
        // The DXF leaves the credit out too.
        let dxf = String::from_utf8(dxf().expect("dxf")).expect("text DXF");
        assert!(dxf.contains("SECTION") && dxf.contains("ENTITIES"), "a DXF");
        assert!(!dxf.contains("draftwright.io"), "no credit link");
        assert!(!dxf.lines().any(|l| l.trim() == "draftwright"), "no credit text in the DXF");

        // An author stands alone.
        let d = set_titles(vec![TitleChange { key: "drawn_by".into(), value: "M. Ivanova".into() }]).expect("author");
        assert_eq!(drawn(&d).as_deref(), Some("M. Ivanova"));
        assert!(d.svg.contains("M. Ivanova") && !d.svg.contains("M. Ivanova / draftwright"));
        // Its glyphs stay and the credit's go: the label is one outline of many loops.
        let alone = d.svg.len();
        set_credit(true).expect("on");
        let credited = show_sheet(1).expect("again").svg.len();
        set_credit(false).expect("off");
        assert!(alone + 1500 < credited, "{alone} against {credited}");
        assert!(alone > whole - 3000, "the author's glyphs are still drawn: {alone} against {whole}");

        set_credit(true).expect("on");
        let d = show_sheet(1).expect("again");
        assert_eq!(drawn(&d).as_deref(), Some("M. Ivanova / draftwright"));

        // Off before a drawing is opened: a cold open draws without the credit too.
        set_credit(false).expect("off");
        for e in library_entries() {
            remove_entry(e.id).expect("remove");
        }
        let bytes = include_bytes!("../tests/fixtures/assembly.step").to_vec();
        let d = open_file("assembly.step".into(), bytes, None, None).expect("open");
        assert_eq!(drawn(&d), None);
        assert_eq!(d.svg.matches("<a ").count(), 0, "{}", mentions(&d));
        assert_eq!(mentions(&d), with - 3);
        set_credit(true).expect("on");

        // Defaults for new drawings round-trip.
        set_title_defaults(vec![TitleChange { key: "company".into(), value: "ACME".into() }]).expect("defaults");
        let company = title_defaults().into_iter().find(|f| f.field.key == "company").expect("company");
        assert_eq!(company.value, "ACME");
        set_title_defaults(Vec::new()).expect("clear");
    }
}

