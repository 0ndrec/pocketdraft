# Developing Pocketdraft

A guide for whoever works on the app next: how it is put together, how a change goes from
the drawing core to the screen, how to test it, and the traps already found. Build steps are in the
[README](../README.md); performance measurements are in
[performance.md](performance.md).

## The ground rules

1. **The drawing core is used as released and never changed.** `draftwright` comes from
   `https://github.com/pzfreo/draftwright-rust` at a tag (`rust/Cargo.toml`), with no
   `[patch]` and no vendored copy. Everything the app adds is built on the core's public
   interface, in this repository. When something seems to need a core change, find a way in
   the app's layer (its own caches, the core's public types, editing a finished drawing), or
   ask the core's author upstream. To check that the core is untouched: no `[patch]` in the
   manifests, `Cargo.lock` resolves `draftwright` to the tag's commit, and
   `git ls-remote https://github.com/pzfreo/draftwright-rust refs/tags/<tag>` gives the same
   commit.
2. **English only** in the UI, code, comments and documentation.
3. **Material Design 3** for every screen: Compose Material 3 components, the theme's colour
   roles (no fixed colours in the UI), bottom sheets for choices on a phone.
4. **Performance before polish** where they conflict (thumbnails, caches, the kept sheet).
5. **No network, no permissions, no tracking.** The app must stay acceptable to F-Droid
   (see its Inclusion Policy): free dependencies only, from Maven Central, Google's Maven or
   crates.io.

## How the app is put together

```text
 Kotlin / Compose (android/app/src/main/java/app/pocketdraft)
   MainActivity ── AppViewModel ── ui/*  (screens and sheets)
                        │  calls on Dispatchers.Default
                        ▼
 UniFFI bindings (generated into android/app/build/rust/bindings, never committed)
                        │  JNA
                        ▼
 Rust layer: crate draftwright-ffi (rust/src)
   lib.rs ── the exported interface: records and functions
   engine.rs ── one engine thread owning the open session
   cache.rs, library.rs, fixes.rs, viewer.rs, log.rs
                        │
                        ▼
 draftwright core (git dependency, unmodified): analysis, planning, drawing, PDF
```

### The Rust layer (`rust/src`)

| File | What it holds |
|---|---|
| `lib.rs` | Everything Kotlin can call (`#[uniffi::export]`) and the records it gets back (`Doc`, `Problem`, `Fix`, `Item`, …). Converts the engine's `DocView` into `Doc`. Integration tests that drive the app's own calls. |
| `engine.rs` | The engine thread and the open drawing (`Actor`): opening, every edit, the view of the drawing (`view`), tap targets, the kept sheet, the credit in "Drawn by", timing notes. |
| `cache.rs` | Per-drawing caches: the model's analysis, projected views, and the last drawn sheet (shown at once on reopening). |
| `library.rs` | The library on disk: entries, saved edits (the Spec), thumbnails, settings, title-block defaults. |
| `fixes.rs` | Field descriptions for the editors and the Spec operations a user's value turns into. |
| `viewer.rs` | The HTML page the WebView shows the drawing in: zoom, pan, tap picking, the messages it exchanges with Kotlin. |
| `log.rs` | `note!`: one line to the Android log (tag `Draftwright`), or standard error on a computer. |

**One engine thread.** The core's session is not shared between threads and its geometry
code recurses deeply, so `engine.rs` keeps it on one thread with a 256 MB stack. Every
exported function sends a job to that thread with `call(...)` and blocks until it answers.
Kotlin calls them from `Dispatchers.Default`, never from the main thread. Jobs run one at a
time, in order: a slow edit delays whatever comes after it.

**Edits go through the Spec.** The core describes a drawing as an analysis of the model plus
a *Spec* (what the user decided: paper, tolerances, datums, title block…). Every edit is a
list of operations on the Spec (`draftwright::items::Op`: `set`, `remove`, `add` at a path
such as `dimensions/<id>/tolerance`), applied as one undo step. In `engine.rs` all edits go
through `edit(a, ops, what)`, which:
- adds the paper and scale when "Keep paper and scale" is on and the Spec leaves them to the
  core (see [performance.md](performance.md): this is what keeps edits fast);
- retries without that pin if the core refuses it;
- retries with `sheet/keep_layout` released when one of the core's "only this item" fixes
  left it on and the core refuses an edit that changes what is drawn;
- then `finish`: rebuilds the view (`view`), saves the Spec with the library entry
  (`remember`), and logs where the time went.

The core's own Spec reference and the list of paths it accepts are in its repository; the
quickest way to learn a path is the core's ready fixes (`Problem.fixes[].ops`) and the
`options(...)` of an item.

**What Kotlin sees.** After every call Kotlin gets a whole `Doc`: the SVG of the shown sheet,
the problems with their fixes, the editable items, the title block, tap targets
(`hits_message`), paper and scale, the datums and origins, the removed dimensions. The UI never
holds a partial state: it replaces `doc` with the answer.

### The Kotlin app (`android/app/src/main/java/app/pocketdraft`)

| File | What it holds |
|---|---|
| `MainActivity.kt` | Starts the view model with the app's files directory; passes on files from "Open with" and "Share". |
| `AppViewModel.kt` | All state (`doc`, `busy`, `message`, which sheet is open, the settings) and every call into Rust. `run(label, then) { op }` runs one core call off the main thread, shows progress, stores the answer and turns a failure into a snackbar message. |
| `ui/App.kt` | The root: the drawing screen always composed, the library drawn over it while no drawing is open, the snackbar, the part picker, Settings. |
| `ui/LibraryScreen.kt` | The library list with thumbnails, "Open STEP file". |
| `ui/DrawingScreen.kt` | The drawing: top bar (back, undo, menu), bottom bar (problems, paper, title block, PDF), the WebView viewer. |
| `ui/Sheets.kt` | Bottom sheets: problems, a dimension's editor, the title block, removed dimensions. |
| `ui/Datums.kt` | Datums and origins, and the chooser a tapped face opens. |
| `ui/PartPicker.kt` | What to draw from an assembly. |
| `ui/SettingsScreen.kt` | App settings and title-block defaults. |
| `ui/Theme.kt` | Material 3 colours (the system palette on Android 12+). |

**The viewer.** The drawing is an SVG shown by `viewer.rs`'s page in a WebView that is
created once and kept (the drawing screen stays composed under the library), so it never
reloads. Kotlin sends it messages with `evaluateJavascript("window.bridge.onMessage(...)")`:
`fit:<svg>` (a new drawing: fit it), `keep:<svg>` (an edit: keep zoom and position),
`hits:[...]` (tap targets), `sel:<id>` (highlight), `bg:<colour>`, `clear:`. The page answers
through a `@JavascriptInterface` called `DwBridge`: `ready`, `drawn`, `tap:<id>`.

**Tap targets.** `engine::view` lists a box for every label on the sheet (from the drawing's
text runs), every title-block cell (`title:<key>`) and every face of the part that can be a
datum (`side:<side>@<view>#<face>`, from `engine::side_hits`). The page picks the nearest box
to a tap; Kotlin opens the editor that the id's prefix names.

## Adding a feature

A typical feature (say, a new setting for the drawing) touches four places, in this order:

1. **Find the Spec path in the core.** Look at the core's Spec types (`src/model.rs` in its
   repository, `pub struct Spec` and the structs under it) and try the edit in a test before
   anything else: `session.apply(&[op], true)` either works or says why not.
2. **Rust layer.** Add the operation in `engine.rs` (through `edit(...)`), anything the UI
   must show to `DocView` and `view(...)`, and an exported function and record fields in
   `lib.rs`. Add an integration test in `lib.rs` that opens `tests/fixtures/assembly.step`
   and makes the edit through the exported function.
3. **View model.** A function in `AppViewModel.kt` that calls the new export through `run(...)`.
4. **UI.** The control in the right place, Material 3, English text. A tool used in every
   session goes on a bar; a setting used once per drawing goes in the top bar's menu; an
   app-wide setting goes in Settings.

The Kotlin bindings are regenerated by the build: a new exported Rust function appears in
Kotlin (`uniffi.draftwright_ffi.<camelCaseName>`) after the next Gradle build. A Rust name
that is also a Kotlin name in the same file can be imported under another name
(`import uniffi.draftwright_ffi.undo as coreUndo`).

### When a change alters what is drawn

The last drawn sheet is cached and shown before the session is ready
(`cache::keep_sheet` / `cache::sheet`). If a change makes the same Spec draw differently (a
new field in `DocView`, a change to how the drawing is edited), **bump `FORMAT` in
`cache.rs`**, or users see the old picture until their next edit. Bumping it drops every
cached analysis and projection too, so the next open of each drawing is a cold one.

## Testing

### Rust tests (on the computer)

```sh
cd rust
RUST_MIN_STACK=268435456 cargo test --release --lib
```

- `--release` because the geometry code is too slow unoptimised, and `RUST_MIN_STACK`
  because rustc overflows its stack compiling one dependency.
- The tests in `lib.rs` share the engine thread and the library directory, so they take the
  `ONE` lock and start from a clean library (`remove_entry` for every entry) when they depend
  on it. The library root is set once per process: a test that needs a fresh library removes
  the entries instead of calling `init` with a new directory.
- Fixtures: `tests/fixtures/assembly.step` (a plate with two pins: an assembly, holes,
  turned steps), `tests/fixtures/GRM-02_shaft.step` (a turned shaft), `assets/plate.step`.

Ignored tests are tools, run on demand with `-- --ignored --nocapture`:

| Test | What it does |
|---|---|
| `bench_edits` | Opens a model and times the usual edits through the app's own calls (`DW_STEP`, `DW_PIN`, `DW_BATCH`, `DW_LIB`, `DW_STOP`) |
| `bench_open_specs` | What building a session costs with an automatic sheet against a given paper (`DW_ONLY`) |
| `print_side_hits` | The views and face tap targets of a model |

The same test binary runs on an Android device or emulator: build it with
`cargo ndk -t x86_64 -P 26 test --release --lib --no-run`, push the file it names to
`/data/local/tmp` and run it there with `bench_edits --ignored --nocapture`.

### On a device or emulator

```sh
cd android
./gradlew :app:assembleRelease -Pabi=x86_64 -PdebugSigned   # for an x86_64 emulator
adb install -r app/build/outputs/apk/release/app-release.apk
adb logcat -s Draftwright
```

The log shows every open and edit with its timing, for example
`edit: 1437 ms (plan 1400, drawing 2, svg 10, …); A4 at 1:2, …; core plans 4, sheet runs 7`,
the call across the boundary (`ui: "…" answered in … ms`) and the page's paint
(`page: painted … ms after the drawing arrived`). `plan` is the core's own work; everything
else is the app's.

What to try by hand before a release (the 2026-11-13 pass in the README went through all
of it): opening a part and an assembly, the part picker, paper sizes and Automatic, hidden
lines, a problem answered with a value and one with a ready fix, a tolerance, removing and
restoring a dimension, undo, datums by the menu and by tapping a face, the title block,
Settings (each switch), a PDF, reopening after a restart, the dark theme, rotation, and one
large model (`vent_lid` class: a few hundred faces).

On Android 16 an emulator cannot be sent a file with "Open with" from adb (the shell may not
grant the file's permission); use the app's own "Open STEP file" there.

## Traps already found

- **Cold opens are slow by nature.** The core analyses the model and plans the sheet; for a
  part with a few hundred faces that is tens of seconds on a phone. Most of an edit's time is
  the core choosing the paper again, which "Keep paper and scale" avoids.
  [performance.md](performance.md) has the numbers and the options measured.
- **A label's glyphs are outlines.** In a finished drawing (`draftwright::output::Drawing`)
  the visible text is filled faces in `layers`; the `text` runs are only an invisible copy
  for searching. To change what a label shows, edit both (see `engine::uncredit`).
- **Sheets cover the snackbar.** A message shown while a bottom sheet is open is hidden
  under it; `run(...)` closes the open sheets when a call fails.
- **Bottom sheets with fields** open at full height (`skipPartiallyExpanded = true`), or an
  opened row's field lies below the screen.
- **The core keeps what it planned** for a part within a process, so the second time the same
  drawing is planned it is nearly free. Benchmarks must compare states the core has not seen.
- **Windows builds:** run Gradle from PowerShell or a Git Bash shell without
  `MSYS_NO_PATHCONV=1` (set for adb in some scripts), or UniFFI's generator gets `/c/...` paths
  it cannot open.

## Conventions

- Comments say what a piece of code is for and why, in full sentences; no history ("was",
  "used to").
- A setting is stored in `library::Settings` (`library/settings.json`), exposed by a pair of
  functions in `lib.rs` (`<name>()` and `set_<name>(...)`), and mirrored in `AppViewModel`.
- Strings shown to the user are plain English, short, and say what to do next when something
  is refused.
- Version: `versionName` and `versionCode` in `android/app/build.gradle.kts`; every release
  gets a tag `v<versionName>` and a `fastlane/metadata/android/en-US/changelogs/<versionCode>.txt`
  (under 500 characters).

## Publishing a release

1. Raise `versionCode` and `versionName` in `android/app/build.gradle.kts` and add
   `fastlane/metadata/android/en-US/changelogs/<versionCode>.txt` (under 500 characters).
2. Commit, tag `v<versionName>` and push the tag.
3. For F-Droid: `docs/fdroid/app.pocketdraft.yml` goes to fdroiddata as
   `metadata/app.pocketdraft.yml`, with the tag's commit. After the first release, F-Droid
   picks up new tags by itself (`UpdateCheckMode: Tags`). The store listing comes from
   `fastlane/` in this repository.
4. F-Droid signs the APK itself. Any other channel needs a release key: keep it and its
   passwords out of the repository (`.gitignore` excludes keystores).

The application id is `app.pocketdraft` (`appId` in `android/gradle.properties`); it cannot
change once published.

## Reference

### Layout

| Path | What |
|---|---|
| `rust/` | Crate `draftwright-ffi` (cdylib): the UniFFI interface in `src/lib.rs` and the app's layer over the core (`engine`, `cache`, `library`, `fixes`, `viewer`, `log`) |
| `rust/uniffi-bindgen/` | The bindings generator, a separate host tool |
| `android/` | Gradle project: `app/src/main/java/app/pocketdraft/` (Activity, ViewModel, Compose screens); the bindings are generated into `app/build/rust/` |
| `build.sh` | A shortcut for the Gradle build |
| `fastlane/` | The store listing F-Droid reads |
| `docs/` | The developer guide, performance measurements and the F-Droid recipe |
| `CONTRIBUTING.md` | What a change needs |

The engine runs the core's session on one thread with a large stack; its functions are
synchronous and Kotlin calls them on `Dispatchers.Default`. The library lives in the app's
private files (`init(filesDir)`).

draftwright is the released core, unmodified: the `v0.2.15` tag of
`pzfreo/draftwright-rust`, with no `[patch]`. Everything here is built on its public
interface.

### The interface

All calls are synchronous; each waits for the engine thread:

| Group | Calls |
|---|---|
| Set-up and library | `init(filesDir)`, `libraryEntries()`, `removeEntry(id)`, `exactPaper()`, `setExactPaper(b)` |
| Opening | `looksLikeStep`, `parts`, `openFile(name, bytes, partIndex, partName)`, `openSaved(id)` |
| The open drawing | `showSheet`, `setPaper(size, exact)`, `setHidden`, `setDatum(side, letter)`, `setOrigin(axis, end)`, `setUncitedDatums`, `applyOps(fix.opsJson)`, `answerProblem(key, value)`, `editItem(item, …)`, `removeDimension(id)`, `restoreDimension(id)`, `setTitles(changes)`, `undo`, `pdf`, `model` |
| Editor fields | `titleFields`, `itemFields(kind)`, `unsupportedText` |
| Viewer page | `viewerHtml(background)`, `viewerShow(fit, svg)` |

Records: `Doc`, `Problem`, `Fix`, `Field`, `Item`, `TitleField`, `Entry`, `PartRow`, `ModelFile`,
`TitleChange`. Failures are `DwException.Failed(reason)` or `NothingToUndo`.

The viewer is an HTML page in a WebView (`rust/src/viewer.rs`). Its
`window.bridge.postMessage` is bound to a `@JavascriptInterface`; the app answers with
`evaluateJavascript`. The drawing screen stays composed and the library is drawn over it,
so the page loads once.

### Status

Works:
- **Library:** list with isometric thumbnails (AndroidSVG) and summaries; remove; opening a
  STEP through the system picker.
- **Assemblies:** the part picker, a bottom sheet (`ui/PartPicker.kt`): the whole assembly first, then
  the parts, with a search field above 8 parts.
- **Theme:** the system palette on Android 12+ (light and dark, both tried); the window
  before the first frame, the thumbnails and the list rows follow it too.
- **Drawing screen:**
  - a cold open and the sheet snapshot (the drawing is shown at once; the session is
    planned behind it, and edits wait for it);
  - top app bar (back, name, undo, and a menu: datums and origins, hidden lines, removed
    dimensions) and a bottom app bar: problems with a count badge, paper
    size menu with Exact size, title block, and Save PDF as its FAB;
  - a floating sheet switcher for drawings of several sheets (composed, not tried);
  - the problems sheet with ready fixes and typed answers;
  - the dimension editor with suggestion chips;
  - undo;
  - Save PDF (system "Save as");
  - the title-block editor (composed, not tried on the emulator);
  - datums and origins (`ui/Datums.kt`, the anchor in the top bar): a datum letter on
    each side of the part (`faces/<side>/datum`), the end of the part each axis's
    dimensions start from (`origin/<axis>`), and whether datums nothing refers to are
    drawn (`sheet/uncited_datums`). All are ordinary Spec edits.
  - Settings (`ui/SettingsScreen.kt`; the gear in the library, "Settings" in the drawing's
    menu): "Keep paper and scale", "Exact size", "Credit draftwright in Drawn by", and the
    title-block values new drawings start with (`titleDefaults`, `setTitleDefaults`);
  - "Credit draftwright in Drawn by" off: the core always writes `draftwright` (or
    `<author> / draftwright`) into the "drawn by" cell and has no setting for it, so the Rust
    layer edits the finished drawing (`engine::uncredit`): it removes the credit's glyph
    outlines (a label is one face of many loops: the loops after the author go) and its
    invisible text, and the cell's link, in the SVG and the PDF. Tried on a phone with and
    without an author;
  - removing dimensions: "Remove" in a dimension's editor (`dimensions/<id>/omit`), with Undo
    in the snackbar; the top bar's menu lists the removed ones, each with "Restore";
  - datums by tap: a face of the part in a principal view, or a drawn datum symbol, opens
    that side's letter chooser (`SideSheet`). The engine adds the tap targets from the
    faces themselves (`engine::side_hits`, IDs `side:<side>@<view>#<face>`): a strip along a
    face seen edge-on, the face's box where it is turned to the viewer. The isometric,
    detail views and a section's cut give none; a label near a face wins the tap. Only the
    six sides the core names can be datums here (planar faces at the ends of the part's box).

Not there yet:
- "Update model from a newer file";
- sharing the PDF or STEP (share sheet);
- the built-in sample.

Tried on a Pixel 9 phone (Android 16) and on emulators; "Share" from other apps is not
verified.

### Test pass on an emulator (2026-11-13)

AVD `Pixel_5a_API_36` (the Pixel 5 profile at 1080×2400, 420 dpi; Android 16, the only system
image installed), the x86_64 build, driven over adb. 56 checks in 12 groups passed: library
and picker, the part picker, paper and the kept sheet, problems (an answer and a ready fix),
dimensions (tolerance, remove, undo, restore), datums and origins, datum by tap, title block,
Settings and the credit, PDF, restart and defaults, dark theme, a 267-face model, rotation.

Found and fixed on the way:
- the problems sheet opened half-height, hiding an opened problem's field and fixes;
- after one of the core's "only this item" fixes (`sheet/keep_layout`), removing a dimension
  was refused, and the refusal lay hidden under the open sheet: `engine::edit` now applies
  such an edit with the guard released, and a refusal closes the sheet so its message shows;
- the kept sheet was not refreshed after the credit setting changed.

Left as they are: a full-height sheet reaches under the status bar; after a rotation the
sheet keeps its zoom instead of fitting the new screen; "Open with" could not be driven from
adb on Android 16 (it worked on a Pixel 9 phone).

### Performance

"Keep paper and scale" (the paper menu, on by default) is option 1 of performance.md:
the first edit of a drawing whose paper or scale the core chose also gives the core that paper
and scale, so later edits do not choose them again; Automatic lays the drawing out again.

`performance.md` has the measurements for a large model (267 faces) and the options
that follow from them. In short: an edit's time is the core choosing the paper and scale
again (16 to 60 sheet plans); with the paper and scale given the same edits are about 11
times faster. The app's own share is under 0.3 s an edit.

To see where time goes on a device: `adb logcat -s Draftwright`. The Rust layer notes every
open and edit with its laps (`edit: 1234 ms (plan 1200, svg 20, …)`), and the app notes the
call across the boundary (`ui: …`) and the page's paint (`page: …`). The benchmarks are
ignored tests (`bench_edits`, `bench_open_specs`); `ABI=x86_64 ./build.sh release` builds the
app for the x86_64 emulator.
