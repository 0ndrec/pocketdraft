# Performance with large models

Measured on 2026-11-12 with `vent_lid.STEP` (690 KB, 267 faces, 82 dimension labels; the core
plans it on A2 at 1:2). The question: why does every edit of a large model take so long, and
what can the app do about it without changing the drawing core.

The core is used as released (`draftwright` v0.2.15, unmodified). Everything measured here is
either the app's own code or a setting the core already offers.

## Summary

- An edit is slow because of one thing: with an automatic sheet, the core chooses the paper
  and scale again on every edit, by planning the whole sheet 16 to 60 times over. That is 99.9%
  of an edit's time.
- The app's own share is small: about 35 ms in the Rust layer, 20–45 ms across the
  Kotlin/Rust boundary and 0.1–0.3 s to show the drawing in the WebView.
- With the paper and scale given (`sheet/size`, `sheet/scale`), the same edits are 10 to 50
  times faster, and the drawing is the same.

## How it was measured

| Tool | What it does |
|---|---|
| `bench_edits` (ignored test, `rust/src/lib.rs`) | opens a model and makes the usual edits through the app's own calls; prints what the caller waits for each |
| `bench_open_specs` (ignored test, `rust/src/engine.rs`) | what a session costs under different sheet settings |
| the engine's notes (`edit: … ms (plan …, svg …)`) | where an edit's time went inside the Rust layer; in the Android log under the tag `Draftwright` |
| `ui:` and `page:` log lines (Kotlin) | the call across the boundary, and the page's paint |
| `ABI=x86_64 ./build.sh release` | the app for the x86_64 emulator, native rather than translated |

```sh
cd rust
export RUST_MIN_STACK=268435456        # rustc needs it for one dependency
DW_STEP=part.step cargo test --release --lib bench_edits -- --ignored --nocapture
DW_STEP=part.step DW_PIN=scale cargo test --release --lib bench_edits -- --ignored --nocapture
DW_STEP=part.step DW_ONLY="paper A2" cargo test --release --lib bench_open_specs -- --ignored --nocapture
adb logcat -s Draftwright               # the notes of the app on a device
```

The same test binary runs on the emulator: build it with
`cargo ndk -t x86_64 -P 24 test --release --lib --no-run`, push the file from
`target/x86_64-linux-android/release/deps/` to `/data/local/tmp` and run it there with
`bench_edits --ignored --nocapture`.

Machines: the development PC (Windows, x86_64) and the emulator `Pixel_9_API_36` (x86_64
image, 4 cores, 4 GB), running x86_64 code natively. No phone was measured; a phone's cores
are slower than the PC's, by a factor that depends on the phone.

A caution for anyone repeating this: within one process the core remembers what it planned
for a part, so a drawing it has seen before comes back in a fraction of a second. Compare
only states it has not planned yet, one process per case where it matters.

## Where an edit's time goes

Emulator, the standard build, each edit on a drawing the core has not planned before:

| Edit | Automatic sheet | Paper and scale given | Faster by |
|---|---|---|---|
| Tolerance on a dimension | 13.5 s | 8.6 s | 1.6 |
| Tolerance on another | 43.4 s | 1.4 s | 31 |
| Remove a dimension | 75.0 s | 1.3 s | 57 |
| Put it back | 23.9 s | 0.4 s | 55 |
| A title-block field | 66.5 s | 6.2 s | 11 |
| Datum A on the bottom | 78.1 s | 7.9 s | 10 |
| Hidden lines off | 62.8 s | 6.8 s | 9 |
| **These seven** | **363 s** | **33 s** | **11** |
| Undo | 0.2 s | 1.2 s | |

Inside one automatic edit (the engine's note): `plan 43317, svg 26, save 0` and nothing else
above 1 ms. The core's own count of sheet plans per edit ("sheet runs") was 16 to 60 with an
automatic sheet and 1 to 5 with the paper and scale given.

The PC gives the same picture about 15% faster (automatic: 12 to 67 s an edit; given: 0.4 to
7.5 s).

In the app itself, on the emulator with nothing else running:

| Step | Time |
|---|---|
| The call across the Kotlin/Rust boundary, with a 650 KB SVG | 20–45 ms |
| Building the page's message and handing it to the WebView | 30–50 ms |
| The page parsing and painting the SVG | 80–230 ms |

## Opening

| | Time (PC) |
|---|---|
| First open: analysis | 1.4 s |
| First open: session with an automatic sheet | 16.9 s (8 sheet runs) |
| Session with the paper given (A2; the scale chosen by the core) | 5.8 s (3 runs), the same drawing |
| Session with the paper and scale given (A2 at 1:2) | 5.5 s |
| Session on other papers given: A3, A1 | 6.3 s, 6.6 s |

On the emulator the first open took 20.5 s (analysis 1.5 s, session 19.0 s).

Opening again after a restart shows the kept sheet at once (0.3 s from the tap in the app)
and builds the session behind it. Until the session is ready no edit can start. It takes as
long as the first time with an automatic sheet (16.8 s on the PC, 19.7 s on the emulator), and
5.4 s on the PC when the saved Spec gives the paper and scale. The cached projections make
no visible difference: the time is the planning.

## Build settings

Emulator, paper and scale given, the eleven edits of `bench_edits` after opening:

| Build of the app's library | Open | The edits | |
|---|---|---|---|
| Standard (`opt-level = 3`, 16 codegen units, no LTO) | 20.4–22.6 s | 49.7 s | |
| The same with mimalloc as the allocator (feature `mimalloc`) | 17.9 s | 38.4 s | 23% less |
| Fat LTO, one codegen unit | 19.8 s | 48.4 s | 3% less |

A repeat of the standard build's run gave the same edit times within 1–2%; opening varies by
about 10%.

- mimalloc replaces Android's allocator (scudo) for the whole library: one `#[global_allocator]`
  in the app's crate. The core's source is untouched, but its allocations go through it.
  Measured on x86_64 only.
- Fat LTO takes 23 minutes to build instead of 6 and gains 3%: not worth it.

## Several edits at once

A plan costs the same for one operation or many. PC, paper and scale given:

| | Time |
|---|---|
| Five dimensions removed, one edit each | 18.7 s |
| Five other dimensions removed in one edit | 2.7 s |

With an automatic sheet three removals one by one took 92 s.

## The core's "keep the layout" mode

`sheet/keep_layout` (with `sheet/size`, `sheet/scale` and `sheets/max = 1`) plans without
choosing paper or refitting. On the PC it was no faster than the paper and scale alone
(tolerance 1.3 s, title field 3.0 s, datum 5.7 s), and it refuses an edit that changes what is
drawn: removing a dimension answers "This edit cannot keep the retained sheet layout".

## Options

1. **Give the paper and scale after the first automatic layout** (done: "Keep paper and scale",
   on by default; on the emulator hidden-lines edits of vent_lid went from 63 s to 4.7–5.6 s). The app writes the paper and
   scale the core chose into the Spec, so later edits plan on that sheet. Measured: edits 11
   times faster over the usual mix, reopening to the first edit 3 times faster. The cost: the
   sheet no longer grows or shrinks by itself after an edit, so the app needs a "fit the sheet
   again" action and a hint when the drawing stops fitting (the core reports that as
   problems). The Spec fields are the core's own.
2. **mimalloc.** 23% off the edits, 12–21% off opening, for one line and one dependency in the
   app's crate. It changes how the library is built, not the core's source. To confirm on an
   arm64 phone.
3. **Several edits, one plan.** Either an explicit "apply" for collected changes, or edits
   made while a plan is running joining the next one. Five removals: 18.7 s down to 2.7 s.
   The drawing shows the changes only after the plan, so pending changes need marking.
4. **A default paper for new drawings.** A session with the paper given costs about 6 s
   instead of 17 s, whatever the paper. The first open would be 3 times faster, but on a paper
   that may not suit the part until the sheet is fitted again.
5. **Ask upstream.** A session that takes a stored plan or plans through the core's plan
   cache (reopening in under a second, as an earlier patched build showed); an edit of text
   alone (a title-block field) that does not choose the paper again; the paper candidates
   planned in parallel.

Measured and not worth doing: fat LTO (3%), the keep-layout mode (no faster, refuses
removals), and work on the WebView or the boundary (0.3 s at most per edit).
