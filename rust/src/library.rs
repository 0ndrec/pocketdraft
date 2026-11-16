//! The library: every STEP file opened in the app, kept with its edits.
//!
//! Layout under the root (the app's private document directory on the device):
//!
//! ```text
//! library/index.json        entries, newest first
//! library/<id>/model.step   a copy of the opened file
//! library/<id>/spec.json    the drawing's Spec after the latest edit
//! library/<id>/thumb-iso.svg  the isometric view's visible edges, for the list
//! library/defaults.json     title-block values every new drawing starts with
//! library/settings.json     app settings (`Settings`)
//! ```
//!
//! The id is the start of the file's SHA-256 (draftwright computes it on open), plus the
//! part number for one part of an assembly, so opening the same file again reuses its entry
//! and restores its edits. Library failures never
//! stop a drawing from opening; they are reported and the drawing still shows.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

const INDEX: &str = "index.json";
const MODEL: &str = "model.step";
const SPEC: &str = "spec.json";
const THUMB: &str = "thumb-iso.svg";
/// Thumbnails made before they showed the isometric view alone; shown until the drawing is
/// opened again and its isometric thumbnail is made.
const THUMB_OLD: &str = "thumb.svg";
const DEFAULTS: &str = "defaults.json";
const SETTINGS: &str = "settings.json";

/// App-wide settings, kept beside the library.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Settings {
    /// A chosen paper size is kept even when the drawing does not fit it. Off (the default),
    /// a drawing that does not fit legibly moves to larger paper.
    #[serde(default)]
    pub exact_paper: bool,
    /// "Keep paper and scale": the first edit of a drawing whose paper or scale the core chose
    /// gives the core that paper and scale, so later edits do not choose them again (about ten
    /// times faster for a large part: docs/performance.md). `None` is on.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub keep_sheet: Option<bool>,
    /// "Credit draftwright in Drawn by": the core always writes `draftwright` (or
    /// `<author> / draftwright`) into the title block's "drawn by" cell. Off, the app takes it
    /// out of the finished drawing: the cell shows the author alone, or nothing. `None` is on.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credit: Option<bool>,
}

impl Settings {
    pub fn keeps_sheet(&self) -> bool {
        self.keep_sheet.unwrap_or(true)
    }
    pub fn credits(&self) -> bool {
        self.credit.unwrap_or(true)
    }
}

pub fn settings(root: &Path) -> Settings {
    fs::read(root.join(SETTINGS))
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

pub fn save_settings(root: &Path, settings: &Settings) -> io::Result<()> {
    fs::create_dir_all(root)?;
    fs::write(root.join(SETTINGS), serde_json::to_vec_pretty(settings).map_err(io::Error::other)?)
}

/// Title-block fields that may have a default: who and for whom, not what the part is.
pub const DEFAULT_KEYS: [&str; 5] = ["company", "drawn_by", "approved_by", "tolerance", "document_type"];

/// Largest thumbnail kept, in bytes: beyond it the outline is cut short. A thumbnail is a
/// recognisable silhouette, not a drawing; speed beats completeness.
const THUMB_MAX: usize = 24 * 1024;

/// One library entry: what the list shows.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    pub id: String,
    pub name: String,
    pub size: u64,
    /// When it was last opened or edited, Unix seconds.
    pub opened: u64,
    pub faces: usize,
    pub features: usize,
    pub problems: usize,
    /// Whether a Spec with the user's edits is stored.
    #[serde(default)]
    pub edited: bool,
    /// One part of an assembly (its index in the file), or the whole file.
    #[serde(default)]
    pub part: Option<usize>,
    #[serde(default)]
    pub part_name: Option<String>,
    /// The paper and scale Automatic chose, which the app gave the core as fixed: the
    /// drawing still shows as Automatic while its Spec holds exactly these.
    #[serde(default)]
    pub chosen_sheet: Option<(String, String)>,
}

impl Entry {
    /// The list's headline: the part and its file, or the file.
    pub fn title(&self) -> String {
        match &self.part_name {
            Some(part) => format!("{part} · {}", self.name),
            None => self.name.clone(),
        }
    }
}

static ROOT: OnceLock<PathBuf> = OnceLock::new();

/// Set the library root once: `<files dir>/library`, the app's private storage the Kotlin
/// side passes in (`Context.getFilesDir()`). Later calls keep the first.
pub fn init(files_dir: &Path) {
    ROOT.get_or_init(|| files_dir.join("library"));
}

pub fn root() -> Option<&'static Path> {
    ROOT.get().map(PathBuf::as_path)
}

pub fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// Entry id for a file's SHA-256 (hex) and, for one part of an assembly, the part.
pub fn entry_id(sha256: &str, part: Option<usize>) -> String {
    let file: String = sha256.chars().filter(char::is_ascii_hexdigit).take(16).collect();
    match part {
        Some(p) => format!("{file}-p{p}"),
        None => file,
    }
}

/// The entry id of a STEP file's bytes (.gz allowed), as [`entry_id`] gives it after
/// analysis: the hash is of the STEP text. `None` when the bytes do not unpack.
pub fn id_of(bytes: &[u8], part: Option<usize>) -> Option<String> {
    use sha2::{Digest, Sha256};
    let step = draftwright::model::step_bytes(bytes).ok()?;
    let hex: String = Sha256::digest(&step).iter().map(|x| format!("{x:02x}")).collect();
    Some(entry_id(&hex, part))
}

/// All entries, most recently opened first. A missing or unreadable index is empty.
pub fn list(root: &Path) -> Vec<Entry> {
    let mut entries: Vec<Entry> = fs::read(root.join(INDEX))
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default();
    entries.sort_by(|a, b| b.opened.cmp(&a.opened));
    entries
}

fn write_index(root: &Path, entries: &[Entry]) -> io::Result<()> {
    fs::create_dir_all(root)?;
    let json = serde_json::to_vec_pretty(entries).map_err(io::Error::other)?;
    // Write then rename, so a crash mid-write never leaves a broken index.
    let tmp = root.join("index.json.tmp");
    fs::write(&tmp, json)?;
    fs::rename(tmp, root.join(INDEX))
}

/// Add or refresh an entry, keeping a copy of the file.
pub fn record(root: &Path, entry: Entry, bytes: &[u8]) -> io::Result<()> {
    let dir = root.join(&entry.id);
    fs::create_dir_all(&dir)?;
    let model = dir.join(MODEL);
    if fs::metadata(&model).map(|m| m.len()).ok() != Some(bytes.len() as u64) {
        fs::write(model, bytes)?;
    }
    let mut entries = list(root);
    let edited = entries.iter().find(|e| e.id == entry.id).is_some_and(|e| e.edited);
    entries.retain(|e| e.id != entry.id);
    entries.insert(0, Entry { edited: entry.edited || edited, ..entry });
    write_index(root, &entries)
}

/// Change an existing entry in place; an unknown id is ignored.
pub fn update(root: &Path, id: &str, change: impl FnOnce(&mut Entry)) -> io::Result<()> {
    let mut entries = list(root);
    let Some(entry) = entries.iter_mut().find(|e| e.id == id) else {
        return Ok(());
    };
    change(entry);
    write_index(root, &entries)
}

/// Store the drawing's Spec after an edit.
pub fn save_spec(root: &Path, id: &str, spec_json: &str) -> io::Result<()> {
    let dir = root.join(id);
    fs::create_dir_all(&dir)?;
    fs::write(dir.join(SPEC), spec_json)
}

/// The stored Spec, if the drawing was edited.
pub fn spec(root: &Path, id: &str) -> Option<String> {
    fs::read_to_string(root.join(id).join(SPEC)).ok()
}

/// An entry and its stored file.
pub fn load(root: &Path, id: &str) -> io::Result<(Entry, Vec<u8>)> {
    let entry = list(root)
        .into_iter()
        .find(|e| e.id == id)
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "not in the library"))?;
    let bytes = fs::read(root.join(id).join(MODEL))?;
    Ok((entry, bytes))
}

/// The title-block defaults (`key → value`); none when never set.
pub fn defaults(root: &Path) -> BTreeMap<String, String> {
    fs::read(root.join(DEFAULTS))
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

/// Store the defaults; empty values are dropped.
pub fn save_defaults(root: &Path, values: &BTreeMap<String, String>) -> io::Result<()> {
    let kept: BTreeMap<&String, &str> = values
        .iter()
        .filter(|(k, v)| DEFAULT_KEYS.contains(&k.as_str()) && !v.trim().is_empty())
        .map(|(k, v)| (k, v.trim()))
        .collect();
    fs::create_dir_all(root)?;
    fs::write(root.join(DEFAULTS), serde_json::to_vec_pretty(&kept).map_err(io::Error::other)?)
}

/// The Spec a new drawing starts with: the core's default plus the title-block defaults.
pub fn defaults_spec(values: &BTreeMap<String, String>) -> String {
    let mut spec = serde_json::to_value(draftwright::model::Spec::default()).unwrap_or_default();
    let title: serde_json::Map<String, serde_json::Value> = values
        .iter()
        .filter(|(k, v)| DEFAULT_KEYS.contains(&k.as_str()) && !v.trim().is_empty())
        .map(|(k, v)| (k.clone(), serde_json::Value::String(v.trim().to_string())))
        .collect();
    if !title.is_empty() {
        if let Some(sheet) = spec.get_mut("sheet").and_then(serde_json::Value::as_object_mut) {
            sheet.insert("title".into(), serde_json::Value::Object(title));
        } else if let Some(obj) = spec.as_object_mut() {
            obj.insert("sheet".into(), serde_json::json!({ "title": title }));
        }
    }
    spec.to_string()
}

/// Store the list thumbnail made from the drawing's SVG (skipped when it already exists:
/// the outline depends on the model, not on edits).
pub fn save_thumbnail(root: &Path, id: &str, svg: &str, iso: Option<[f64; 4]>) -> io::Result<()> {
    let path = root.join(id).join(THUMB);
    if path.exists() {
        return Ok(());
    }
    match thumbnail_svg(svg, iso) {
        Some(thumb) => fs::write(path, thumb),
        None => Ok(()),
    }
}

/// The stored thumbnail, if there is one.
pub fn thumbnail(root: &Path, id: &str) -> Option<String> {
    let dir = root.join(id);
    fs::read_to_string(dir.join(THUMB))
        .or_else(|_| fs::read_to_string(dir.join(THUMB_OLD)))
        .ok()
}

/// A small SVG of the sheet's visible model edges (the core's `part` layer): no dimensions,
/// text, hidden lines, frame or title block; coordinates cut to 0.1 mm; elements past
/// [`THUMB_MAX`] dropped. One pass over the layer, no geometry library: cheap to make and
/// cheap to draw at 56 dp. `None` when the SVG has no such layer.
pub fn thumbnail_svg(svg: &str, iso: Option<[f64; 4]>) -> Option<String> {
    let at = svg.find("id=\"part\"")?;
    let open_end = at + svg[at..].find('>')? + 1;
    let close = open_end + svg[open_end..].find("</g>")?;
    let mut body = String::new();
    let mut bounds = Bounds::default();
    for element in svg[open_end..close].split('<').skip(1) {
        let element = element.trim();
        if element.is_empty() || element.starts_with('/') {
            continue;
        }
        let short = round_numbers(element);
        // Only the isometric view: an edge belongs to the view whose page area holds its
        // centre. Without an isometric view on the sheet every view is kept.
        if let Some([ax0, ay0, ax1, ay1]) = iso {
            let mut own = Bounds::default();
            own.add_element(&short);
            let Some([x0, y0, x1, y1]) = own.0 else { continue };
            let (cx, cy) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0);
            if cx < ax0.min(ax1) || cx > ax0.max(ax1) || cy < ay0.min(ay1) || cy > ay0.max(ay1) {
                continue;
            }
        }
        if body.len() + short.len() > THUMB_MAX {
            break;
        }
        bounds.add_element(&short);
        body.push('<');
        body.push_str(&short);
    }
    let [x0, y0, x1, y1] = bounds.rect()?;
    // The layer sits in the sheet's y-up frame: flip it, and frame the outline with a margin.
    let (w, h) = (x1 - x0, y1 - y0);
    let m = 0.04 * w.max(h);
    let stroke = (w.max(h) / 70.0).max(0.1);
    Some(format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"{:.1} {:.1} {:.1} {:.1}\">\
         <g transform=\"scale(1,-1)\" fill=\"none\" stroke=\"currentColor\" stroke-width=\"{stroke:.2}\" \
         stroke-linecap=\"round\">{body}</g></svg>",
        x0 - m,
        -y1 - m,
        w + 2.0 * m,
        h + 2.0 * m,
    ))
}

/// Cut every decimal number to one digit after the point.
fn round_numbers(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut decimals: Option<usize> = None;
    for c in text.chars() {
        match (c, decimals) {
            ('.', None) => {
                decimals = Some(0);
                out.push(c);
            }
            (d, Some(n)) if d.is_ascii_digit() => {
                if n < 1 {
                    out.push(d);
                }
                decimals = Some(n + 1);
            }
            _ => {
                decimals = None;
                out.push(c);
            }
        }
    }
    out
}

/// The extent of `line`, `circle` and `path` (M, L, A) elements.
#[derive(Default)]
struct Bounds(Option<[f64; 4]>);

impl Bounds {
    fn add(&mut self, x: f64, y: f64) {
        let r = self.0.get_or_insert([x, y, x, y]);
        *r = [r[0].min(x), r[1].min(y), r[2].max(x), r[3].max(y)];
    }

    fn rect(&self) -> Option<[f64; 4]> {
        self.0.filter(|[x0, y0, x1, y1]| x1 > x0 || y1 > y0)
    }

    fn add_element(&mut self, element: &str) {
        let attr = |name: &str| -> Option<f64> {
            let key = format!(" {name}=\"");
            let start = element.find(&key)? + key.len();
            element[start..].split('"').next()?.parse().ok()
        };
        if element.starts_with("line") {
            if let (Some(x1), Some(y1), Some(x2), Some(y2)) = (attr("x1"), attr("y1"), attr("x2"), attr("y2")) {
                self.add(x1, y1);
                self.add(x2, y2);
            }
        } else if element.starts_with("circle") {
            if let (Some(cx), Some(cy), Some(r)) = (attr("cx"), attr("cy"), attr("r")) {
                self.add(cx - r, cy - r);
                self.add(cx + r, cy + r);
            }
        } else if let Some(d) = element.find(" d=\"").map(|i| &element[i + 4..]) {
            let d = d.split('"').next().unwrap_or_default();
            let mut command = 'M';
            let mut numbers: Vec<f64> = Vec::new();
            let flush = |command: char, numbers: &mut Vec<f64>, bounds: &mut Bounds| {
                match command {
                    'A' => {
                        for arc in numbers.chunks_exact(7) {
                            bounds.add(arc[5], arc[6]);
                        }
                    }
                    _ => {
                        for pair in numbers.chunks_exact(2) {
                            bounds.add(pair[0], pair[1]);
                        }
                    }
                }
                numbers.clear();
            };
            for token in d.split(|c: char| c == ' ' || c == ',').filter(|t| !t.is_empty()) {
                let first = token.chars().next().unwrap_or(' ');
                if first.is_ascii_alphabetic() {
                    flush(command, &mut numbers, self);
                    command = first.to_ascii_uppercase();
                    if let Ok(n) = token[1..].parse() {
                        numbers.push(n);
                    }
                } else if let Ok(n) = token.parse() {
                    numbers.push(n);
                }
            }
            flush(command, &mut numbers, self);
        }
    }
}

/// Remove an entry with its file and edits.
pub fn remove(root: &Path, id: &str) -> io::Result<()> {
    let mut entries = list(root);
    entries.retain(|e| e.id != id);
    write_index(root, &entries)?;
    match fs::remove_dir_all(root.join(id)) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    }
}

/// "just now", "5 min ago", "3 h ago", "yesterday", "4 days ago", or the date.
pub fn when(opened: u64, now: u64) -> String {
    let ago = now.saturating_sub(opened);
    match ago {
        0..60 => "just now".into(),
        60..3600 => format!("{} min ago", ago / 60),
        3600..86_400 => format!("{} h ago", ago / 3600),
        86_400..172_800 => "yesterday".into(),
        172_800..604_800 => format!("{} days ago", ago / 86_400),
        _ => date(opened),
    }
}

/// `YYYY-MM-DD` (UTC) of a Unix time: days to a civil date (Howard Hinnant's algorithm).
fn date(secs: u64) -> String {
    let z = (secs / 86_400) as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}")
}

/// The list's supporting line: when, size of the model, problems, edits.
pub fn summary(entry: &Entry, now: u64) -> String {
    let problems = match entry.problems {
        0 => "no problems".to_string(),
        1 => "1 problem".to_string(),
        n => format!("{n} problems"),
    };
    let mut line = format!("{} · {} faces · {problems}", when(entry.opened, now), entry.faces);
    if entry.edited {
        line.push_str(" · edited");
    }
    line
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("dw-library-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    fn entry(id: &str, opened: u64) -> Entry {
        Entry {
            id: id.into(),
            name: format!("{id}.step"),
            size: 3,
            opened,
            faces: 10,
            features: 4,
            problems: 1,
            edited: false,
            part: None,
            part_name: None,
            chosen_sheet: None,
        }
    }

    #[test]
    fn records_lists_loads_and_removes() {
        let root = temp_root("crud");
        assert!(list(&root).is_empty());
        record(&root, entry("aaa", 10), b"one").unwrap();
        record(&root, entry("bbb", 20), b"two").unwrap();
        let ids: Vec<_> = list(&root).into_iter().map(|e| e.id).collect();
        assert_eq!(ids, ["bbb", "aaa"]);

        // Opening again moves it to the front and keeps a single entry.
        record(&root, entry("aaa", 30), b"one").unwrap();
        let ids: Vec<_> = list(&root).into_iter().map(|e| e.id).collect();
        assert_eq!(ids, ["aaa", "bbb"]);

        let (loaded, bytes) = load(&root, "aaa").unwrap();
        assert_eq!((loaded.name.as_str(), bytes.as_slice()), ("aaa.step", b"one".as_slice()));
        remove(&root, "aaa").unwrap();
        assert_eq!(list(&root).len(), 1);
        assert!(load(&root, "aaa").is_err());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn edits_survive_reopening() {
        let root = temp_root("spec");
        record(&root, entry("ccc", 10), b"x").unwrap();
        save_spec(&root, "ccc", "{\"sheet\":{}}").unwrap();
        update(&root, "ccc", |e| e.edited = true).unwrap();
        // A plain re-open (edited: false) keeps the edited mark and the Spec.
        record(&root, entry("ccc", 20), b"x").unwrap();
        assert!(list(&root)[0].edited);
        assert_eq!(spec(&root, "ccc").as_deref(), Some("{\"sheet\":{}}"));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn defaults_round_trip_into_a_spec() {
        let root = temp_root("defaults");
        let mut values = BTreeMap::new();
        values.insert("company".to_string(), " ACME ".to_string());
        values.insert("drawn_by".to_string(), "".to_string());
        values.insert("material".to_string(), "not a default".to_string());
        save_defaults(&root, &values).unwrap();
        let stored = defaults(&root);
        assert_eq!(stored.len(), 1);
        assert_eq!(stored["company"], "ACME");
        let spec: serde_json::Value = serde_json::from_str(&defaults_spec(&stored)).unwrap();
        assert_eq!(spec.pointer("/sheet/title/company").and_then(|v| v.as_str()), Some("ACME"));
        let parsed: Result<draftwright::model::Spec, _> = serde_json::from_value(spec);
        assert!(parsed.is_ok(), "{parsed:?}");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn thumbnail_keeps_only_the_outline() {
        let svg = r#"<svg viewBox="0 -210 297 210"><g transform="scale(1,-1)">
            <g fill="none" stroke="rgb(0,0,0)" stroke-width="0.5" id="part">
              <line x1="28.13001" y1="63.00001" x2="148.13001" y2="73.00001" />
              <circle cx="48.13001" cy="121.87" r="6" fill="none" />
              <path d="M 10.5,20.25 A 5,5 0 0 1 15.5,25.75" />
            </g>
            <g id="dims"><text x="1" y="2">60</text></g></g></svg>"#;
        let thumb = thumbnail_svg(svg, None).unwrap();
        assert!(thumb.contains("x1=\"28.1\""), "{thumb}");
        assert!(!thumb.contains("<text"));
        assert!(thumb.contains("stroke=\"currentColor\""));
        // x 10.5..148.1, y 20.2..127.8, flipped and with a 4% margin.
        assert!(thumb.contains("viewBox=\"5.0 -133.3"), "{thumb}");
        assert!(thumbnail_svg("<svg></svg>", None).is_none());
        // With the isometric view's area only the circle (centre 48.1, 121.8) is inside.
        let iso = thumbnail_svg(svg, Some([40.0, 110.0, 60.0, 130.0])).unwrap();
        assert!(iso.contains("<circle") && !iso.contains("<line") && !iso.contains("<path"), "{iso}");
        assert_eq!(round_numbers("a 1.23456 b -0.987 c 12"), "a 1.2 b -0.9 c 12");
    }

    #[test]
    fn formats_times_and_summaries() {
        assert_eq!(when(1000, 1030), "just now");
        assert_eq!(when(1000, 1000 + 5 * 60), "5 min ago");
        assert_eq!(when(0, 3 * 3600), "3 h ago");
        assert_eq!(when(0, 86_400 + 10), "yesterday");
        assert_eq!(when(0, 4 * 86_400), "4 days ago");
        assert_eq!(date(1_794_182_400), "2026-11-09");
        assert_eq!(entry_id("ABCDEF0123456789abcdef", None), "ABCDEF0123456789");
        assert_eq!(entry_id("ABCDEF0123456789abcdef", Some(2)), "ABCDEF0123456789-p2");
        let mut e = entry("a", 0);
        e.edited = true;
        assert_eq!(summary(&e, 30), "just now · 10 faces · 1 problem · edited");
    }
}
