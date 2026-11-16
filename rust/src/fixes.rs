//! Interactive fixes: what to ask the user so a problem can be closed, and the Spec
//! operations for what they enter.
//!
//! The draftwright core offers ready-made edits itself (`ProblemRow::fixes`), but some
//! problems need facts the model does not carry: the general tolerance, the material, the
//! drawing number… Those problems map to a title-block field (`sheet/title/<key>`) and the
//! UI shows an input for it. Dimensions and text labels picked on the drawing get their
//! own fields: tolerance, fit and text before/after the value.

use draftwright::items::Op;
use serde_json::Value;

use crate::engine::{ItemInfo, ProblemRow, TitleField};

/// An editable field: a title-block cell or a property of a dimension or label.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Field {
    /// Spec key: `sheet/title/<key>` for title fields, `dim:<name>` for item fields.
    pub key: &'static str,
    /// Short name, shown on the field's chip.
    pub label: &'static str,
    pub hint: &'static str,
    /// Common values, shown as chips; a tap applies one at once.
    pub suggestions: &'static [&'static str],
}

/// Title-block fields in editor order. The sheet number is the core's own.
pub const FIELDS: &[Field] = &[
    Field {
        key: "title",
        label: "Title",
        hint: "Part name as printed in the title block.",
        suggestions: &[],
    },
    Field {
        key: "number",
        label: "Number",
        hint: "Drawing number. Empty: taken from the file name, or DWG-001.",
        suggestions: &[],
    },
    Field {
        key: "material",
        label: "Material",
        hint: "Material grade. The STEP file states none, or more than one.",
        suggestions: &["Steel C45", "AISI 304", "AISI 316L", "Al 6061-T6", "Al 7075-T6", "Brass CW614N", "POM-C"],
    },
    Field {
        key: "tolerance",
        label: "General tolerance",
        hint: "Applies to dimensions without their own tolerance. Only the designer can choose it.",
        suggestions: &["ISO 2768-m", "ISO 2768-f", "ISO 2768-c", "ISO 2768-mK", "±0.1"],
    },
    Field {
        key: "drawn_by",
        label: "Drawn by",
        hint: "Designer's name.",
        suggestions: &[],
    },
    Field {
        key: "approved_by",
        label: "Approved by",
        hint: "Approver's name.",
        suggestions: &[],
    },
    Field {
        key: "date",
        label: "Date",
        hint: "For example 2026-11-09.",
        suggestions: &[],
    },
    Field {
        key: "revision",
        label: "Revision",
        hint: "Revision letter or number.",
        suggestions: &["A", "B", "1"],
    },
    Field {
        key: "company",
        label: "Company",
        hint: "Owner of the document.",
        suggestions: &[],
    },
    Field {
        key: "document_type",
        label: "Document type",
        hint: "For example Detail drawing.",
        suggestions: &["Detail drawing", "Assembly drawing"],
    },
];

/// Fields of a dimension or text label picked on the drawing.
pub const TOLERANCE: Field = Field {
    key: "dim:tolerance",
    label: "Tolerance",
    hint: "±T, or +upper -lower such as +0.1 -0. Empty: the general tolerance.",
    suggestions: &["±0.1", "±0.05", "±0.02", "+0.1 -0", "+0 -0.1"],
};
pub const FIT: Field = Field {
    key: "dim:fit",
    label: "Fit",
    hint: "ISO 286 fit class of the diameter. Empty: no fit.",
    suggestions: &["H7", "H8", "h6", "g6", "k6", "js6"],
};
pub const PREFIX: Field = Field {
    key: "dim:prefix",
    label: "Before",
    hint: "Text printed before the value.",
    suggestions: &["2×", "SR", "~"],
};
pub const SUFFIX: Field = Field {
    key: "dim:suffix",
    label: "After",
    hint: "Text printed after the value.",
    suggestions: &["TYP.", "REF", "AFTER PLATING"],
};

pub fn field(key: &str) -> Option<&'static Field> {
    FIELDS.iter().find(|f| f.key == key)
}

/// Title-block cell (`title_block/<cell>`) → Spec key (`<key>`).
fn key_of_cell(cell: &str) -> Option<&'static str> {
    draftwright::title_block::KEYS
        .iter()
        .find(|(_, f)| *f == cell)
        .map(|(k, _)| *k)
}

/// The title field to fill in to close this problem, when user data closes it.
pub fn ask_for(problem: &ProblemRow) -> Option<&'static Field> {
    let by_code = match problem.code.as_str() {
        "general_tolerance_missing" | "step_general_tolerance_disagreement" => Some("tolerance"),
        "step_material_unavailable" | "step_material_ambiguous" | "step_material_disagreement" => {
            Some("material")
        }
        _ => None,
    };
    let key = by_code.or_else(|| {
        problem.refs.iter().find_map(|r| {
            r.strip_prefix("sheet/title/")
                .or_else(|| r.strip_prefix("title_block/").and_then(key_of_cell))
        })
    })?;
    field(key)
}

/// A short headline for a problem code; unknown codes are shown as words.
pub fn headline(code: &str) -> String {
    let known = match code {
        "general_tolerance_missing" => "General tolerance missing",
        "step_general_tolerance_disagreement" => "General tolerance differs from PMI",
        "step_material_unavailable" => "STEP material unusable",
        "step_material_ambiguous" => "Several materials in STEP",
        "step_material_disagreement" => "Material differs from STEP",
        "title_field_overflow" => "Text overflows its cell",
        "annotation_off_sheet" => "Label runs off the sheet",
        "feature_not_dimensioned" => "Feature not dimensioned",
        "feature_not_located" => "Feature not located",
        "feature_no_centermark" => "Centre mark missing",
        "hole_requirement_missing" => "Hole parameter missing",
        "hole_requirement_unverifiable" => "Hole parameter unproven",
        "datum_undeclared" => "Datum not declared",
        "annotation_ink_overlap" => "Labels overlap",
        "leader_crossing" => "Leaders cross",
        _ => "",
    };
    if known.is_empty() {
        let mut words = code.replace('_', " ");
        if let Some(first) = words.get_mut(0..1) {
            first.make_ascii_uppercase();
        }
        words
    } else {
        known.to_string()
    }
}

/// How much of the drawing a core fix changes.
pub fn scope_label(scope: &str) -> &'static str {
    match scope {
        "local" => "Changes one item",
        "view" => "Lays out the view again",
        _ => "Lays out the whole sheet again",
    }
}

/// The value the user set for a title field (empty: automatic).
pub fn current(title: &[TitleField], key: &str) -> String {
    title
        .iter()
        .find(|f| f.key == key)
        .and_then(|f| f.authored.clone())
        .unwrap_or_default()
}

/// What the core draws when the field is left empty — shown as the placeholder.
pub fn automatic(title: &[TitleField], key: &str) -> String {
    match title.iter().find(|f| f.key == key) {
        Some(TitleField { authored: None, drawn: Some(v), .. }) => format!("Auto: {v}"),
        _ => String::new(),
    }
}

/// Whether the text has letters the drawing font lacks (IBM Plex Sans Condensed is
/// Latin-only): such letters do not show in the PDF.
pub fn unsupported_text(value: &str) -> bool {
    value.chars().any(|c| ('\u{0400}'..='\u{04FF}').contains(&c))
}

/// Set a title field; an empty value hands it back to the core.
pub fn set_title(key: &str, value: &str) -> Op {
    let value = value.trim();
    let path = format!("sheet/title/{key}");
    if value.is_empty() {
        Op { op: "remove".into(), path, value: None }
    } else {
        Op { op: "set".into(), path, value: Some(Value::String(value.to_string())) }
    }
}

/// Operations for a value the user typed. When the core offers a fix for the same field
/// (for example UNSPECIFIED together with keeping the sheet layout), its whole bundle is
/// reused with the typed value: the edit stays local and the sheet is not laid out again.
pub fn ops_for(problem: Option<&ProblemRow>, key: &str, value: &str) -> Vec<Op> {
    let path = format!("sheet/title/{key}");
    let value = value.trim();
    let template = problem.and_then(|p| {
        p.fixes
            .iter()
            .find(|f| f.ops.iter().any(|op| op.op == "set" && op.path == path))
    });
    match template {
        Some(fix) if !value.is_empty() => fix
            .ops
            .iter()
            .cloned()
            .map(|mut op| {
                if op.path == path {
                    op.value = Some(Value::String(value.to_string()));
                }
                op
            })
            .collect(),
        _ => vec![set_title(key, value)],
    }
}

/// Operations for changed title fields (`(key, new value)`), merged into one edit. A
/// field that a current problem's fix also sets reuses that fix's bundle ([`ops_for`]).
pub fn title_ops(problems: &[ProblemRow], changes: &[(&str, String)]) -> Vec<Op> {
    let mut ops: Vec<Op> = Vec::new();
    for (key, value) in changes {
        let path = format!("sheet/title/{key}");
        let problem = problems.iter().find(|p| {
            p.fixes
                .iter()
                .any(|f| f.ops.iter().any(|op| op.op == "set" && op.path == path))
        });
        for op in ops_for(problem, key, value) {
            ops.retain(|o| o.path != op.path);
            ops.push(op);
        }
    }
    ops
}

/// A tolerance as the core reads it: `+-0.1` and `+/-0.1` → `±0.1`, a bare number →
/// `±number`, a decimal comma → point.
pub fn normalize_tolerance(value: &str) -> String {
    let v = value.trim().replace("+/-", "±").replace("+-", "±").replace('−', "-");
    let v = v.split_whitespace().collect::<Vec<_>>().join(" ");
    if !v.is_empty() && v.chars().all(|c| c.is_ascii_digit() || c == '.' || c == ',') {
        format!("±{}", v.replace(',', "."))
    } else {
        v.replace(',', ".")
    }
}

/// The values of an item's editor.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ItemValues {
    pub tolerance: String,
    pub fit: String,
    pub prefix: String,
    pub suffix: String,
}

impl ItemValues {
    pub fn of(item: &ItemInfo) -> Self {
        let text = |v: &Option<String>| v.as_deref().unwrap_or_default().trim().to_string();
        Self {
            tolerance: text(&item.tolerance),
            fit: text(&item.fit),
            prefix: text(&item.prefix),
            suffix: text(&item.suffix),
        }
    }
}

/// Spec operations for editing a dimension or text label: only what changed. An empty
/// field removes the entry, giving back the general tolerance and the automatic text.
pub fn item_ops(item: &ItemInfo, new: &ItemValues) -> Vec<Op> {
    let group = if item.kind == "dimension" { "dimensions" } else { "annotations" };
    let base = format!("{group}/{}", draftwright::items::key(&item.id));
    let old = ItemValues::of(item);
    let mut ops = Vec::new();
    let mut field = |name: &str, was: &str, now: &str| {
        let now = now.trim();
        if now == was.trim() {
            return;
        }
        let path = format!("{base}/{name}");
        ops.push(if now.is_empty() {
            Op { op: "remove".into(), path, value: None }
        } else {
            Op { op: "set".into(), path, value: Some(Value::String(now.to_string())) }
        });
    };
    if item.kind == "dimension" {
        field("tolerance", &old.tolerance, &normalize_tolerance(&new.tolerance));
        field("fit", &old.fit, &new.fit);
    }
    let (p, s) = (new.prefix.trim(), new.suffix.trim());
    if p != old.prefix || s != old.suffix {
        let path = format!("{base}/text");
        ops.push(if p.is_empty() && s.is_empty() {
            Op { op: "remove".into(), path, value: None }
        } else {
            let mut text = serde_json::Map::new();
            if !p.is_empty() {
                text.insert("prefix".into(), Value::String(format!("{p} ")));
            }
            if !s.is_empty() {
                text.insert("suffix".into(), Value::String(format!(" {s}")));
            }
            Op { op: "set".into(), path, value: Some(Value::Object(text)) }
        });
    }
    ops
}

/// The heading of an item's editor: what is printed on the sheet.
pub fn item_title(item: &ItemInfo) -> String {
    match (item.kind.as_str(), item.label.is_empty()) {
        ("dimension", true) => "Dimension".into(),
        (_, true) => "Label".into(),
        _ => item.label.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::FixRow;

    fn row(code: &str, refs: &[&str]) -> ProblemRow {
        ProblemRow {
            key: "0".into(),
            severity: "warning".into(),
            tier: "tidy".into(),
            code: code.into(),
            message: String::new(),
            refs: refs.iter().map(|s| s.to_string()).collect(),
            fixes: Vec::new(),
        }
    }

    fn set(path: &str, v: Value) -> Op {
        Op { op: "set".into(), path: path.into(), value: Some(v) }
    }

    fn tolerance_problem() -> ProblemRow {
        let mut p = row("general_tolerance_missing", &["sheet/title/tolerance"]);
        p.fixes.push(FixRow {
            description: String::new(),
            scope: "local".into(),
            ops: vec![
                set("sheet/title/tolerance", Value::String("UNSPECIFIED".into())),
                set("sheet/keep_layout", Value::Bool(true)),
            ],
        });
        p
    }

    fn dim(tolerance: Option<&str>) -> ItemInfo {
        ItemInfo {
            id: "d:envelope/width.length".into(),
            kind: "dimension".into(),
            label: "60".into(),
            tolerance: tolerance.map(str::to_string),
            fit: None,
            prefix: None,
            suffix: None,
            reference: false,
        }
    }

    #[test]
    fn maps_problems_to_title_fields() {
        assert_eq!(ask_for(&row("general_tolerance_missing", &[])).unwrap().key, "tolerance");
        assert_eq!(ask_for(&row("step_material_ambiguous", &[])).unwrap().key, "material");
        assert_eq!(
            ask_for(&row("title_field_overflow", &["title_block/drawing_number"])).unwrap().key,
            "number"
        );
        assert_eq!(ask_for(&row("x", &["sheet/title/date"])).unwrap().key, "date");
        assert!(ask_for(&row("feature_not_dimensioned", &["f:hole:1"])).is_none());
    }

    #[test]
    fn user_value_reuses_the_core_fix_bundle() {
        let p = tolerance_problem();
        let ops = ops_for(Some(&p), "tolerance", "ISO 2768-m");
        assert_eq!(ops.len(), 2);
        assert_eq!(ops[0].value, Some(Value::String("ISO 2768-m".into())));
        assert_eq!(ops[1].path, "sheet/keep_layout");
        assert_eq!(ops_for(Some(&p), "tolerance", "").len(), 1);
        assert_eq!(ops_for(None, "material", "AISI 304").len(), 1);
    }

    #[test]
    fn title_changes_merge_into_one_edit() {
        let problems = [tolerance_problem()];
        let ops = title_ops(
            &problems,
            &[("tolerance", "ISO 2768-f".into()), ("material", "AISI 304".into())],
        );
        let paths: Vec<_> = ops.iter().map(|o| o.path.as_str()).collect();
        assert_eq!(paths, ["sheet/title/tolerance", "sheet/keep_layout", "sheet/title/material"]);
    }

    #[test]
    fn empty_value_removes_field() {
        assert_eq!(set_title("material", "  ").op, "remove");
        let op = set_title("material", " AISI 304 ");
        assert_eq!(op.value, Some(Value::String("AISI 304".into())));
    }

    #[test]
    fn tolerance_is_normalized() {
        assert_eq!(normalize_tolerance(" +-0.1 "), "±0.1");
        assert_eq!(normalize_tolerance("0,05"), "±0.05");
        assert_eq!(normalize_tolerance("+0.1  -0"), "+0.1 -0");
        assert_eq!(normalize_tolerance(""), "");
    }

    #[test]
    fn item_edits_touch_only_changed_fields() {
        let item = dim(None);
        let mut v = ItemValues::of(&item);
        assert!(item_ops(&item, &v).is_empty());
        v.tolerance = "0.1".into();
        let ops = item_ops(&item, &v);
        assert_eq!(ops.len(), 1);
        assert_eq!(ops[0].path, "dimensions/d:envelope~1width.length/tolerance");
        assert_eq!(ops[0].value, Some(Value::String("±0.1".into())));
        let item = dim(Some("±0.1"));
        let mut v = ItemValues::of(&item);
        v.tolerance.clear();
        v.suffix = "TYP.".into();
        let ops = item_ops(&item, &v);
        assert_eq!(ops[0].op, "remove");
        assert_eq!(ops[1].path, "dimensions/d:envelope~1width.length/text");
        assert_eq!(ops[1].value, Some(serde_json::json!({"suffix": " TYP."})));
    }

    #[test]
    fn every_spec_title_key_has_a_field() {
        for (key, _) in draftwright::title_block::KEYS.iter() {
            assert!(key == &"sheet" || field(key).is_some(), "{key}");
        }
        assert!(unsupported_text("\u{0421}\u{0442}"));
        assert!(!unsupported_text("Steel C45 ±0.1"));
        assert_eq!(headline("odd_new_code"), "Odd new code");
    }
}
