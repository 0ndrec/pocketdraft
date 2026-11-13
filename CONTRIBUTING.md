# Contributing to Pocketdraft

Thank you for helping. Before a change:

- Read [docs/DEVELOPMENT.md](docs/DEVELOPMENT.md): how the app is built, how a feature goes
  from the drawing core to the screen, and the traps already found.
- The draftwright core is used as released and is never modified here. Build on its public
  interface; ask its author upstream for anything that needs a change in it.
- English only, in the UI, code and documentation. Material Design 3 for the UI.
- No network access, permissions, analytics or non-free dependencies: the app is published
  on F-Droid.

With a change:

- Rust tests pass: `cd rust && RUST_MIN_STACK=268435456 cargo test --release --lib`, with a
  test for the new behaviour through the exported functions in `rust/src/lib.rs`.
- The app builds: `cd android && ./gradlew :app:assembleRelease`.
- What it changes on screen is tried on a device or emulator (`adb logcat -s Draftwright`
  shows the timings).
- If it changes what a saved drawing looks like, `FORMAT` in `rust/src/cache.rs` is bumped.
- User-facing changes get a line in the next `fastlane/metadata/android/en-US/changelogs/`.

By contributing you agree that your work is licensed under the AGPL-3.0, as the project.
