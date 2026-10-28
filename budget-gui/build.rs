use std::{
    env, fs, io,
    path::{Path, PathBuf},
    process::Command,
};

fn main() {
    let manifest_dir =
        PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("Cargo manifest directory"));
    let root = manifest_dir.parent().expect("workspace directory");
    let files = source_inputs(root).expect("read build inputs");

    // File watches catch edits, while directory watches also catch additions and removals.
    for directory in ["budget-gui", "budget-core", "vendor/webbrowser", "scripts"] {
        println!("cargo:rerun-if-changed={}", root.join(directory).display());
    }
    for file in &files {
        println!("cargo:rerun-if-changed={}", root.join(file).display());
    }

    let rustc = env::var_os("RUSTC").unwrap_or_else(|| "rustc".into());
    let compiler = Command::new(rustc)
        .arg("--version")
        .output()
        .expect("read compiler version");
    assert!(compiler.status.success(), "rustc --version failed");
    let compiler = String::from_utf8(compiler.stdout).expect("compiler version is UTF-8");
    let target = env::var("TARGET").expect("Cargo build target");
    let profile = env::var("PROFILE").expect("Cargo build profile");
    let mut metadata = vec![("rustc".to_string(), compiler.trim().to_string())];
    for key in [
        "RUSTC",
        "RUSTFLAGS",
        "CARGO_ENCODED_RUSTFLAGS",
        "TARGET",
        "PROFILE",
        "OPT_LEVEL",
        "DEBUG",
    ] {
        println!("cargo:rerun-if-env-changed={key}");
        if key != "RUSTC" {
            metadata.push((key.to_string(), env::var(key).unwrap_or_default()));
        }
    }
    for (key, value) in env::vars().filter(|(key, _)| key.starts_with("CARGO_FEATURE_")) {
        println!("cargo:rerun-if-env-changed={key}");
        metadata.push((key, value));
    }
    let metadata = metadata
        .iter()
        .map(|(key, value)| (key.as_str(), value.as_str()))
        .collect::<Vec<_>>();
    let build_id = fingerprint(root, &files, &metadata).expect("fingerprint build inputs");
    let manifest = build_manifest(root, &files, &build_id, &target, &profile, compiler.trim())
        .expect("record build inputs");
    let output = PathBuf::from(env::var_os("OUT_DIR").expect("Cargo output directory"));
    fs::write(
        output.join(format!("build-inputs-{build_id}.json")),
        manifest,
    )
    .expect("write build input manifest");
    println!("cargo:rustc-env=BUDGET_BUILD_ID={build_id}");
    println!("cargo:rustc-env=BUDGET_BUILD_TARGET={target}");
    println!("cargo:rustc-env=BUDGET_BUILD_PROFILE={profile}");
}

fn source_inputs(root: &Path) -> io::Result<Vec<PathBuf>> {
    // Keep this allowlist aligned with scripts/archive-release.ps1. Local budgets and
    // generated/progress files must never affect a build or enter its source archive.
    let mut files = [
        "Cargo.toml",
        "Cargo.lock",
        "rust-toolchain.toml",
        ".gitattributes",
        ".gitignore",
        "SPEC.md",
        "CHANGELOG.md",
        "AGENTS.md",
        "budget-gui/Cargo.toml",
        "budget-gui/build.rs",
        "budget-core/Cargo.toml",
        "vendor/webbrowser/Cargo.toml",
        "docs/ux.md",
    ]
    .into_iter()
    .map(PathBuf::from)
    .collect::<Vec<_>>();
    for (directory, extensions) in [
        ("budget-gui/src", &["rs"][..]),
        ("budget-gui/assets", &["ico", "png", "svg"][..]),
        ("budget-core/src", &["rs"][..]),
        ("budget-core/tests", &["rs"][..]),
        ("vendor/webbrowser/src", &["rs"][..]),
        ("scripts", &["ps1"][..]),
    ] {
        collect_inputs(root, Path::new(directory), extensions, &mut files)?;
    }
    for name in [
        "LICENSE",
        "LICENSE-MIT",
        "LICENSE-APACHE",
        "LICENSE.txt",
        "README.md",
    ] {
        let path = Path::new("vendor/webbrowser").join(name);
        if root.join(&path).is_file() {
            files.push(path);
        }
    }
    files.sort_by_key(|path| relative_name(path));
    files.dedup();
    Ok(files)
}

fn collect_inputs(
    root: &Path,
    relative: &Path,
    extensions: &[&str],
    files: &mut Vec<PathBuf>,
) -> io::Result<()> {
    if !root.join(relative).exists() {
        return Ok(());
    }
    for entry in fs::read_dir(root.join(relative))? {
        let entry = entry?;
        let name = entry.file_name();
        if [
            "target",
            "local-releases",
            ".git",
            "docs",
            "reference",
            "analysis",
        ]
        .iter()
        .any(|ignored| name.to_string_lossy().eq_ignore_ascii_case(ignored))
        {
            continue;
        }
        let path = relative.join(name);
        let kind = entry.file_type()?;
        if kind.is_dir() {
            collect_inputs(root, &path, extensions, files)?;
        } else if kind.is_file()
            && path
                .extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| {
                    extensions
                        .iter()
                        .any(|allowed| extension.eq_ignore_ascii_case(allowed))
                })
        {
            files.push(path);
        }
    }
    Ok(())
}

fn relative_name(path: &Path) -> String {
    path.to_str()
        .expect("source paths are UTF-8")
        .replace('\\', "/")
}

fn fingerprint(root: &Path, files: &[PathBuf], metadata: &[(&str, &str)]) -> io::Result<String> {
    let mut hash = 0xcbf29ce484222325u64;
    hash_field(&mut hash, b"budget-calendar-build-id-v1");
    let mut files = files.to_vec();
    files.sort_by_key(|path| relative_name(path));
    files.dedup();
    for file in files {
        hash_field(&mut hash, relative_name(&file).as_bytes());
        hash_field(&mut hash, &fs::read(root.join(file))?);
    }
    let mut metadata = metadata.to_vec();
    metadata.sort_unstable();
    for (key, value) in metadata {
        hash_field(&mut hash, key.as_bytes());
        hash_field(&mut hash, value.as_bytes());
    }
    Ok(format!("{hash:016x}"))
}

fn hash_field(hash: &mut u64, bytes: &[u8]) {
    for byte in (bytes.len() as u64).to_le_bytes().iter().chain(bytes) {
        *hash ^= u64::from(*byte);
        *hash = hash.wrapping_mul(0x100000001b3);
    }
}

fn build_manifest(
    root: &Path,
    files: &[PathBuf],
    build_id: &str,
    target: &str,
    profile: &str,
    compiler: &str,
) -> io::Result<String> {
    let mut files = files.to_vec();
    files.sort_by_key(|path| relative_name(path));
    files.dedup();
    let inputs = files
        .iter()
        .map(|path| {
            let digest = content_hash(&fs::read(root.join(path))?);
            Ok(format!(
                "{{\"path\":{},\"fnv1a\":\"{digest}\"}}",
                json_string(&relative_name(path))
            ))
        })
        .collect::<io::Result<Vec<_>>>()?;
    Ok(format!(
        "{{\"build_id\":{},\"target\":{},\"profile\":{},\"compiler\":{},\"inputs\":[{}]}}",
        json_string(build_id),
        json_string(target),
        json_string(profile),
        json_string(compiler),
        inputs.join(",")
    ))
}

fn content_hash(bytes: &[u8]) -> String {
    let hash = bytes.iter().fold(0xcbf29ce484222325u64, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
    });
    format!("{hash:016x}")
}

fn json_string(value: &str) -> String {
    let mut output = String::from("\"");
    for character in value.chars() {
        match character {
            '"' => output.push_str("\\\""),
            '\\' => output.push_str("\\\\"),
            '\n' => output.push_str("\\n"),
            '\r' => output.push_str("\\r"),
            '\t' => output.push_str("\\t"),
            character if character < ' ' => {
                output.push_str(&format!("\\u{:04x}", u32::from(character)))
            }
            character => output.push(character),
        }
    }
    output.push('"');
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

    struct Fixture(PathBuf);

    impl Fixture {
        fn new() -> Self {
            let id = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
            let root =
                std::env::temp_dir().join(format!("budget-build-id-{}-{id}", std::process::id()));
            fs::create_dir_all(&root).unwrap();
            let fixture = Self(root);
            for path in [
                "Cargo.toml",
                "Cargo.lock",
                "rust-toolchain.toml",
                ".gitattributes",
                ".gitignore",
                "SPEC.md",
                "CHANGELOG.md",
                "AGENTS.md",
                "docs/ux.md",
                "scripts/archive-release.ps1",
                "budget-gui/Cargo.toml",
                "budget-core/Cargo.toml",
                "budget-gui/build.rs",
                "budget-gui/src/main.rs",
                "budget-core/src/lib.rs",
                "budget-gui/assets/icon.png",
                "vendor/webbrowser/Cargo.toml",
                "vendor/webbrowser/src/lib.rs",
            ] {
                fixture.write(path, path.as_bytes());
            }
            fixture
        }

        fn write(&self, path: &str, bytes: &[u8]) {
            let path = self.0.join(path);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, bytes).unwrap();
        }

        fn id(&self, metadata: &[(&str, &str)]) -> String {
            fingerprint(&self.0, &source_inputs(&self.0).unwrap(), metadata).unwrap()
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let root = self.0.canonicalize().unwrap();
            let temp = std::env::temp_dir().canonicalize().unwrap();
            assert!(root.starts_with(temp));
            assert!(root
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("budget-build-id-"));
            fs::remove_dir_all(root).unwrap();
        }
    }

    #[test]
    fn source_inventory_includes_inputs_and_new_source_but_excludes_build_outputs() {
        let fixture = Fixture::new();
        fixture.write("budget-core/tests/new_contract.rs", b"test source");
        fixture.write("budget-gui/assets/new/icon.svg", b"asset");
        for path in [
            "target/generated.rs",
            "budget-gui/target/generated.rs",
            "local-releases/v0.3.0/build.rs",
            ".git/config",
            "docs/superpowers/plans/notes.rs",
            "docs/screenshot.png",
            "scripts/private.bgt",
            "budget-gui/assets/private.bgt",
        ] {
            fixture.write(path, b"not a source input");
        }
        let paths = source_inputs(&fixture.0)
            .unwrap()
            .iter()
            .map(|path| path.to_string_lossy().replace('\\', "/"))
            .collect::<Vec<_>>();
        for expected in [
            "Cargo.toml",
            "Cargo.lock",
            "rust-toolchain.toml",
            ".gitattributes",
            ".gitignore",
            "SPEC.md",
            "CHANGELOG.md",
            "AGENTS.md",
            "docs/ux.md",
            "scripts/archive-release.ps1",
            "budget-gui/Cargo.toml",
            "budget-core/Cargo.toml",
            "budget-gui/build.rs",
            "budget-gui/src/main.rs",
            "budget-core/src/lib.rs",
            "budget-core/tests/new_contract.rs",
            "budget-gui/assets/icon.png",
            "budget-gui/assets/new/icon.svg",
            "vendor/webbrowser/Cargo.toml",
            "vendor/webbrowser/src/lib.rs",
        ] {
            assert!(
                paths.iter().any(|path| path == expected),
                "missing input: {expected}"
            );
        }
        assert_eq!(
            paths.len(),
            20,
            "unexpected generated or local input: {paths:?}"
        );
        assert!(paths.windows(2).all(|pair| pair[0] < pair[1]));
    }

    #[test]
    fn nested_generated_reference_and_analysis_directories_are_excluded_case_insensitively() {
        let fixture = Fixture::new();
        let original = source_inputs(&fixture.0).unwrap();
        for directory in [
            "Target",
            "Local-Releases",
            ".Git",
            "Docs",
            "Reference",
            "Analysis",
        ] {
            fixture.write(
                &format!("budget-core/src/{directory}/generated.rs"),
                b"generated",
            );
            fixture.write(&format!("scripts/{directory}/generated.ps1"), b"generated");
        }
        assert_eq!(source_inputs(&fixture.0).unwrap(), original);
    }

    #[test]
    fn fingerprint_is_stable_across_location_inventory_order_and_timestamp_changes() {
        let first = Fixture::new();
        let second = Fixture::new();
        let metadata = [
            ("rustc", "rustc fixture"),
            ("target", "example-target"),
            ("profile", "release"),
        ];
        let mut files = source_inputs(&first.0).unwrap();
        let original = fingerprint(&first.0, &files, &metadata).unwrap();
        files.reverse();
        assert_eq!(original, fingerprint(&first.0, &files, &metadata).unwrap());
        first.write("budget-core/src/lib.rs", b"budget-core/src/lib.rs");
        assert_eq!(original, first.id(&metadata));
        assert_eq!(original, second.id(&metadata));
        assert_eq!(original.len(), 16);
        assert!(original
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase()));
    }

    #[test]
    fn fingerprint_changes_for_source_assets_manifest_changelog_and_new_files() {
        for path in [
            "budget-gui/src/main.rs",
            "budget-core/src/lib.rs",
            "vendor/webbrowser/src/lib.rs",
            "budget-gui/assets/icon.png",
            "Cargo.toml",
            "Cargo.lock",
            "budget-gui/Cargo.toml",
            "CHANGELOG.md",
            "budget-gui/build.rs",
            "budget-core/src/new.rs",
            "AGENTS.md",
            "SPEC.md",
            "docs/ux.md",
            "scripts/new-script.ps1",
        ] {
            let fixture = Fixture::new();
            let before = fixture.id(&[]);
            fixture.write(path, b"changed content");
            assert_ne!(before, fixture.id(&[]), "build ID ignored {path}");
        }
    }

    #[test]
    fn fingerprint_changes_for_compiler_target_profile_and_flags() {
        let fixture = Fixture::new();
        let metadata = [
            ("rustc", "rustc fixture"),
            ("target", "example-target"),
            ("profile", "release"),
            ("flags", "-C opt-level=3"),
        ];
        let original = fixture.id(&metadata);
        for index in 0..metadata.len() {
            let mut changed = metadata;
            changed[index].1 = "different";
            assert_ne!(
                original,
                fixture.id(&changed),
                "metadata ignored: {}",
                metadata[index].0
            );
        }
        assert_ne!(
            fingerprint(&fixture.0, &[PathBuf::from("Cargo.toml")], &[]).unwrap(),
            fingerprint(&fixture.0, &[PathBuf::from("Cargo.lock")], &[]).unwrap()
        );
    }

    #[test]
    fn build_manifest_records_identity_and_verifiable_source_content() {
        let fixture = Fixture::new();
        fixture.write("budget-core/src/lib.rs", b"foobar");
        let manifest = build_manifest(
            &fixture.0,
            &[PathBuf::from("budget-core/src/lib.rs")],
            "0123456789abcdef",
            "fixture-target",
            "release",
            "rustc fixture",
        )
        .unwrap();
        assert_eq!(manifest, "{\"build_id\":\"0123456789abcdef\",\"target\":\"fixture-target\",\"profile\":\"release\",\"compiler\":\"rustc fixture\",\"inputs\":[{\"path\":\"budget-core/src/lib.rs\",\"fnv1a\":\"85944171f73967e8\"}]}");
        assert_eq!(content_hash(b""), "cbf29ce484222325");
        assert_eq!(content_hash(b"a"), "af63dc4c8601ec8c");
    }
}
