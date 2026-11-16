//! Cached instances: what opening a drawing again may reuse instead of computing.
//!
//! Opening a drawing has three costs: analysing the STEP model (recognising its features),
//! projecting its views (hidden-line removal: by far the largest, tens of seconds for a big
//! part on a phone), and planning the sheet from the Spec. The first two depend only on the
//! model and the engine, never on the user's edits, so they are kept per library entry:
//!
//! ```text
//! library/<id>/cache/engine               the engine fingerprint the files below belong to
//! library/<id>/cache/analysis.json.gz     the model's Analysis
//! library/<id>/cache/projections.json.gz  projected views and cuts, grown as views appear
//! library/<id>/cache/sheet.json.gz        sheet 1 as last shown, and the Spec it shows
//! ```
//!
//! Planning the sheet depends on the Spec and is done by the core's session every time one is
//! built: the core is used as it is, and its session takes no stored plan. So the last drawn
//! sheet is kept instead: the UI's whole view of it (SVG, problems, title block, tap targets)
//! for one exact Spec, shown before any session exists, while the session plans behind it.
//!
//! The rules:
//!
//! 1. **Identity.** An entry's id is the model's SHA-256 (and part number), so its cache can
//!    only describe that model. A changed file is a new entry with a new cache; "Update
//!    model" removes the old entry, cache included. A cached Analysis whose source hash does
//!    not match the model is discarded.
//! 2. **Engine.** Everything is only valid for the engine that wrote it: the `engine` file
//!    holds the core's source fingerprint and this module's format. Any difference (an app
//!    update) discards the whole cache directory before anything is read.
//! 3. **Edits never invalidate the model's pieces.** Analysis and projections do not depend
//!    on the Spec. The sheet does: it is kept with a hash of the exact Spec it was drawn from
//!    and shown only when the Spec a session would start from (the saved edits, or the
//!    title-block defaults for a drawing never edited) hashes the same. Every edit on sheet 1
//!    rewrites it.
//! 4. **Show first, build behind.** A matching sheet is shown at once; the session is built
//!    from the cached analysis and projections by the next job on the engine thread, so any
//!    action the user takes waits for it, and none can reach the previous drawing. A build
//!    that fails turns the next action into its error.
//! 5. **The open drawing stays in memory.** Going back to the library keeps the session;
//!    opening the same entry again reuses it with no work at all.
//! 6. **Writes are cheap and safe.** Projections are written after an open or edit only
//!    when the session projected something new; serialising happens on the engine thread,
//!    compressing and writing on a background thread, via a temporary file and a rename.
//!    A file that fails to read is deleted and the drawing opens cold.
//! 7. **Bounded.** One entry's projections above [`ENTRY_MAX`] are not kept; all caches together are held
//!    under [`TOTAL_MAX`] by dropping the caches of the drawings opened longest ago (their
//!    models and edits stay).
//! 8. **Disposable.** Deleting any cache file only costs time: nothing else reads it.

use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use draftwright::haecceity::{self, Part};
use draftwright::model::{self, Analysis, Projections};
use flate2::{read::GzDecoder, write::GzEncoder, Compression};

use crate::engine::DocView;
use crate::library;
use crate::log::note;

const DIR: &str = "cache";
const ENGINE: &str = "engine";
const ANALYSIS: &str = "analysis.json.gz";
const PROJECTIONS: &str = "projections.json.gz";
const SHEET: &str = "sheet.json.gz";
/// Bumped when this module's file layout changes.
const FORMAT: &str = "cache-8";

/// Largest projections file kept for one entry, compressed.
pub const ENTRY_MAX: u64 = 24 * 1024 * 1024;
/// Largest total of all entries' caches.
pub const TOTAL_MAX: u64 = 96 * 1024 * 1024;

fn fingerprint() -> String {
    format!("{FORMAT} {}", draftwright::stateless::ENGINE_VERSION)
}

fn dir(root: &Path, id: &str) -> PathBuf {
    root.join(id).join(DIR)
}

/// The entry's cache directory when it belongs to this engine. A stale one is removed.
fn valid_dir(root: &Path, id: &str) -> Option<PathBuf> {
    let dir = dir(root, id);
    match fs::read_to_string(dir.join(ENGINE)) {
        Ok(engine) if engine == fingerprint() => Some(dir),
        Ok(_) => {
            let _ = fs::remove_dir_all(&dir);
            None
        }
        Err(_) => None,
    }
}

/// The entry's cache directory, created and stamped with this engine when missing or stale.
fn writable_dir(root: &Path, id: &str) -> io::Result<PathBuf> {
    if let Some(dir) = valid_dir(root, id) {
        return Ok(dir);
    }
    let dir = dir(root, id);
    fs::create_dir_all(&dir)?;
    write_atomic(&dir.join(ENGINE), fingerprint().as_bytes())?;
    Ok(dir)
}

fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, bytes)?;
    fs::rename(tmp, path)
}

fn read_gz(path: &Path) -> io::Result<Vec<u8>> {
    let mut out = Vec::new();
    GzDecoder::new(fs::File::open(path)?).read_to_end(&mut out)?;
    Ok(out)
}

fn gz(bytes: &[u8]) -> io::Result<Vec<u8>> {
    // Speed over size: the fastest level still shrinks this JSON several times.
    let mut encoder = GzEncoder::new(Vec::with_capacity(bytes.len() / 4), Compression::fast());
    encoder.write_all(bytes)?;
    encoder.finish()
}

/// A cached piece that cannot be used: forget it so the next open does not try again.
fn discard(path: &Path, why: impl std::fmt::Display) {
    note!("cache: dropping {}: {why}", path.display());
    let _ = fs::remove_file(path);
}

/// The model's part and Analysis from the cache: the Analysis read back, the part read from
/// the STEP bytes again (a small share of analysing it) and set to the analysed frame.
pub fn analysed(root: &Path, id: &str, bytes: &[u8], part: Option<usize>) -> Option<(Part, Analysis)> {
    let path = valid_dir(root, id)?.join(ANALYSIS);
    if !path.exists() {
        return None;
    }
    let analysis: Analysis = match read_gz(&path).and_then(|b| serde_json::from_slice(&b).map_err(io::Error::other)) {
        Ok(a) => a,
        Err(e) => {
            discard(&path, e);
            return None;
        }
    };
    if library::entry_id(&analysis.binding.source_sha256, part) != id {
        discard(&path, "it describes another model");
        return None;
    }
    // The part the Analysis was made of: the one chosen, or the one part every body of an
    // assembly coincided with.
    let read = model::step_bytes(bytes).ok().and_then(|b| match analysis.binding.part_index {
        None => haecceity::read_step(&b).ok(),
        Some(k) => haecceity::step::StepFile::read(&b).and_then(|file| file.part(k)).ok(),
    });
    match read.and_then(|p| model::as_analysed(p, &analysis).ok()) {
        Some(p) => Some((p, analysis)),
        None => {
            discard(&path, "the model no longer reads as analysed");
            None
        }
    }
}

/// Keep the model's Analysis (once; it never changes for an entry).
pub fn keep_analysis(root: &Path, id: &str, analysis: &Analysis) {
    let kept = writable_dir(root, id).and_then(|dir| {
        let path = dir.join(ANALYSIS);
        if path.exists() {
            return Ok(());
        }
        let json = serde_json::to_vec(analysis).map_err(io::Error::other)?;
        write_atomic(&path, &gz(&json)?)
    });
    if let Err(e) = kept {
        note!("cache: could not keep the analysis of {id}: {e}");
    }
}

/// The entry's projections, or empty ones (a cold start).
pub fn projections(root: &Path, id: &str) -> Projections {
    let Some(path) = valid_dir(root, id).map(|d| d.join(PROJECTIONS)).filter(|p| p.exists()) else {
        return Projections::default();
    };
    match read_gz(&path).and_then(|b| Projections::from_disk(&b).map_err(io::Error::other)) {
        Ok(p) => p,
        Err(e) => {
            discard(&path, e);
            Projections::default()
        }
    }
}

/// Keep the session's projections: serialised here, compressed and written on a background
/// thread so the drawing shows without waiting for the disk. Then hold all caches under
/// [`TOTAL_MAX`], never touching *id*'s.
pub fn keep_projections(root: &Path, id: &str, projections: &Projections) {
    let json = match projections.disk_bytes() {
        Ok(json) => json,
        Err(e) => {
            note!("cache: could not serialise the projections of {id}: {e}");
            return;
        }
    };
    let (root, owned) = (root.to_path_buf(), id.to_string());
    let write = move || {
        let id = owned;
        let kept = gz(&json).and_then(|packed| {
            if packed.len() as u64 > ENTRY_MAX {
                return Ok(());
            }
            let dir = writable_dir(&root, &id)?;
            write_atomic(&dir.join(PROJECTIONS), &packed)
        });
        if let Err(e) = kept {
            note!("cache: could not keep the projections of {id}: {e}");
        }
        trim(&root, &id, TOTAL_MAX);
    };
    if std::thread::Builder::new().name("draftwright-cache".into()).spawn(write).is_err() {
        note!("cache: no thread to write the projections of {id}");
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
struct Sheet {
    /// SHA-256 of the Spec JSON the sheet was drawn from.
    spec: String,
    doc: DocView,
}

fn spec_key(spec_json: &str) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(spec_json.as_bytes()).iter().map(|x| format!("{x:02x}")).collect()
}

/// The entry's sheet 1 as last drawn, when drawn from exactly *spec_json*.
pub fn sheet(root: &Path, id: &str, spec_json: &str) -> Option<DocView> {
    let path = valid_dir(root, id)?.join(SHEET);
    if !path.exists() {
        return None;
    }
    match read_gz(&path).and_then(|b| serde_json::from_slice::<Sheet>(&b).map_err(io::Error::other)) {
        Ok(kept) if kept.spec == spec_key(spec_json) => Some(kept.doc),
        // Another Spec (edits made since, or other defaults): kept until rewritten.
        Ok(_) => None,
        Err(e) => {
            discard(&path, e);
            None
        }
    }
}

/// Keep sheet 1 as drawn from *spec_json*, written on a background thread.
pub fn keep_sheet(root: &Path, id: &str, spec_json: &str, doc: &DocView) {
    let json = match serde_json::to_vec(&Sheet { spec: spec_key(spec_json), doc: doc.clone() }) {
        Ok(json) => json,
        Err(e) => {
            note!("cache: could not serialise the sheet of {id}: {e}");
            return;
        }
    };
    let (root, owned) = (root.to_path_buf(), id.to_string());
    let write = move || {
        let id = owned;
        let kept = gz(&json).and_then(|packed| write_atomic(&writable_dir(&root, &id)?.join(SHEET), &packed));
        if let Err(e) = kept {
            note!("cache: could not keep the sheet of {id}: {e}");
        }
    };
    if std::thread::Builder::new().name("draftwright-cache".into()).spawn(write).is_err() {
        note!("cache: no thread to write the sheet of {id}");
    }
}

/// Bytes held by an entry's cache.
pub fn size(root: &Path, id: &str) -> u64 {
    fn sum(dir: &Path) -> u64 {
        fs::read_dir(dir)
            .map(|files| {
                files
                    .flatten()
                    .filter_map(|f| f.metadata().ok().map(|m| (f.path(), m)))
                    .map(|(path, m)| if m.is_dir() { sum(&path) } else { m.len() })
                    .sum()
            })
            .unwrap_or(0)
    }
    sum(&dir(root, id))
}

/// Drop the caches of the entries opened longest ago until all of them fit in *max*; the
/// entry *keep* (the open drawing) is never dropped.
pub fn trim(root: &Path, keep: &str, max: u64) {
    let entries = library::list(root); // newest first
    let mut sizes: Vec<(String, u64)> = entries.iter().map(|e| (e.id.clone(), size(root, &e.id))).collect();
    let mut total: u64 = sizes.iter().map(|s| s.1).sum();
    while total > max {
        let Some(at) = sizes.iter().rposition(|(id, bytes)| id != keep && *bytes > 0) else {
            break;
        };
        let (id, bytes) = sizes.remove(at);
        if fs::remove_dir_all(dir(root, &id)).is_ok() {
            total -= bytes;
        } else {
            break;
        }
    }
}

/// Drop one entry's cache (a warm open the core refused).
pub fn clear(root: &Path, id: &str) {
    let _ = fs::remove_dir_all(dir(root, id));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("dw-cache-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        root
    }

    const PLATE: &[u8] = include_bytes!("../assets/plate.step");

    #[test]
    fn analysis_round_trips_and_engine_change_discards_it() {
        let root = temp_root("analysis");
        let (_, analysis) = model::analyse_native_bytes_part("plate.step", PLATE, None).unwrap();
        let id = library::entry_id(&analysis.binding.source_sha256, None);
        assert_eq!(library::id_of(PLATE, None).as_deref(), Some(id.as_str()), "the id is known before analysing");
        assert!(analysed(&root, &id, PLATE, None).is_none(), "nothing cached yet");

        keep_analysis(&root, &id, &analysis);
        let (_, back) = analysed(&root, &id, PLATE, None).expect("a hit");
        assert_eq!(serde_json::to_value(&back).unwrap(), serde_json::to_value(&analysis).unwrap());

        // Another id (another model) never reads this entry's analysis.
        fs::create_dir_all(root.join("0000000000000000")).unwrap();
        fs::rename(dir(&root, &id), dir(&root, "0000000000000000")).unwrap();
        assert!(analysed(&root, "0000000000000000", PLATE, None).is_none());
        assert!(!dir(&root, "0000000000000000").join(ANALYSIS).exists(), "the mismatch is dropped");

        // A cache stamped by another engine is removed before anything is read.
        keep_analysis(&root, &id, &analysis);
        fs::write(dir(&root, &id).join(ENGINE), "cache-0 older").unwrap();
        assert!(analysed(&root, &id, PLATE, None).is_none());
        assert!(!dir(&root, &id).exists());

        // A damaged file is a miss, and is deleted.
        keep_analysis(&root, &id, &analysis);
        fs::write(dir(&root, &id).join(ANALYSIS), b"not gzip").unwrap();
        assert!(analysed(&root, &id, PLATE, None).is_none());
        assert!(!dir(&root, &id).join(ANALYSIS).exists());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn warm_session_draws_the_same_sheet() {
        use draftwright::model::Spec;
        use draftwright::session::Session;
        let root = temp_root("projections");
        let (part, analysis) = model::analyse_native_bytes_part("plate.step", PLATE, None).unwrap();
        let id = library::entry_id(&analysis.binding.source_sha256, None);
        keep_analysis(&root, &id, &analysis);
        let cold = Session::new(part, analysis, Spec::default()).unwrap();
        let svg = cold.drawing_on_sheet(1).unwrap().svg();
        assert!(cold.stats().projections > 0);

        keep_projections(&root, &id, cold.projections());
        let path = dir(&root, &id).join(PROJECTIONS);
        for _ in 0..100 {
            if path.exists() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        let (part, analysis) = analysed(&root, &id, PLATE, None).unwrap();
        let warm = Session::with_projections(part, analysis, Spec::default(), projections(&root, &id)).unwrap();
        assert_eq!(warm.stats().projections, 0, "every view came from the cache");
        assert_eq!(warm.drawing_on_sheet(1).unwrap().svg(), svg);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn sheet_is_shown_only_for_its_own_spec() {
        let root = temp_root("sheet");
        let doc = DocView {
            name: "plate.step".into(),
            svg: "<svg/>".into(),
            sheet: 1,
            sheets: vec![1],
            problems: Vec::new(),
            problem_total: 0,
            size: None,
            exact: false,
            paper: "A4".into(),
            scale: "1:1".into(),
            kept: false,
            overflow: false,
            hidden: None,
            units: "mm".into(),
            shaded: false,
            faces: 6,
            features: 1,
            title: Vec::new(),
            hits: Vec::new(),
            items: Vec::new(),
            removed: Vec::new(),
            datums: Default::default(),
            origins: Default::default(),
            uncited_datums: false,
        };
        keep_sheet(&root, "abcd", r#"{"sheet":{}}"#, &doc);
        let path = dir(&root, "abcd").join(SHEET);
        for _ in 0..100 {
            if path.exists() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert_eq!(sheet(&root, "abcd", r#"{"sheet":{}}"#), Some(doc));
        assert_eq!(sheet(&root, "abcd", r#"{"sheet":{"size":"A3"}}"#), None, "another Spec");
        assert!(path.exists(), "a mismatch keeps the file");
        fs::write(&path, b"junk").unwrap();
        assert_eq!(sheet(&root, "abcd", r#"{"sheet":{}}"#), None);
        assert!(!path.exists(), "a damaged sheet is dropped");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn trim_drops_the_oldest_caches_first() {
        let root = temp_root("trim");
        for (i, id) in ["aaaa", "bbbb", "cccc"].iter().enumerate() {
            let entry = library::Entry {
                id: id.to_string(),
                name: format!("{id}.step"),
                size: 1,
                opened: 100 + i as u64, // cccc newest
                faces: 0,
                features: 0,
                problems: 0,
                edited: false,
                part: None,
                part_name: None,
                chosen_sheet: None,
            };
            library::record(&root, entry, b"x").unwrap();
            let d = writable_dir(&root, id).unwrap();
            fs::write(d.join(PROJECTIONS), vec![0u8; 1000]).unwrap();
        }
        let one = size(&root, "aaaa");
        // Room for two: the oldest (aaaa) goes; the open one is kept even when oldest.
        trim(&root, "cccc", 2 * one);
        assert_eq!(size(&root, "aaaa"), 0);
        assert!(size(&root, "bbbb") > 0 && size(&root, "cccc") > 0);
        trim(&root, "bbbb", one);
        assert!(size(&root, "bbbb") > 0 && size(&root, "cccc") == 0);
        assert!(root.join("cccc").join("model.step").exists(), "models stay");
        let _ = fs::remove_dir_all(&root);
    }
}
