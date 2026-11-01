Cut a release.

1. Bump `workspace.package.version` in Cargo.toml and `versionName` in apps/android/app/build.gradle.kts.
2. Run `/test`.
3. `cargo build --release -p perch-daemon -p perch-cli --features full-linux` and list the binaries + sizes.
4. Update `.claude/memory/changelog.md` and the Status table in README.md.
5. `git tag -a vX.Y.Z -m "..."`; do not push unless asked.
