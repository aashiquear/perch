Cut a release.

1. Bump `workspace.package.version` in Cargo.toml, `versionName`/`versionCode` in apps/android/app/build.gradle.kts and `CFBundleShortVersionString` in apps/ipad/Info.plist.
2. Run `/test`.
3. `cargo build --release -p perch-daemon -p perch-cli --features full-linux` and list the binaries + sizes.
4. Update `.claude/memory/changelog.md` and the Status table in README.md.
5. `git tag -a vX.Y.Z -m "..."`; do not push unless asked. Pushing the tag makes `.github/workflows/release.yml` attach the Linux tarball, the Android debug APK and the unsigned iPad IPA. To add the IPA to an older release: `gh workflow run release -f tag=vX.Y.Z`.
