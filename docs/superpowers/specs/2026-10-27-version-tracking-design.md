# Visible and recorded app versions

The user needs to identify the app version they are using and understand changes between builds. Begin with v0.3.0 as the first tracked local release; earlier local changes shared the upstream v0.2.0 label and must not be retroactively assigned invented releases.

Show v0.3.0 in the native title and header. Clicking the version or choosing About/version history from View opens a readable, bounded dialog with the current version, a build identifier, dated change history and a copy-version-info button. Version history comes from the same repository CHANGELOG.md used to track changes. Keep all budget editing, layout, data and safe saves unchanged.

Use budget-gui/Cargo.toml as the semantic version source, reflected in Cargo.lock. A dependency-free build script generates a deterministic identifier from app/core/vendor source, assets, manifests, changelog and compiler/target/profile so different source builds can be distinguished. Expose --version and --version-json without opening or loading a budget.

Archive delivered builds under local-releases/vVERSION with executable, icon, changelog and a manifest with version/build ID/SHA256. An archive command refuses to overwrite a different build under an existing version; recording the identical build is idempotent. Preserve matching application source in a source archive. Add root AGENTS.md guidance requiring a version bump, dated changelog entry, tests/build and archival for each future delivered app change. No new dependencies; never write Dave's budget as test data. Do not close an open draft to replace a running executable.
