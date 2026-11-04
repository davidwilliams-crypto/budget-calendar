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
    for directory in [
        "budget-gui",
        "budget-core",
        "budget-engine",
        "budget-mobile/public",
        "budget-mobile/server",
        "budget-mobile/tests",
        "vendor/webbrowser",
        "scripts",
        "installer",
    ] {
        println!("cargo:rerun-if-changed={}", root.join(directory).display());
    }
    for file in &files {
        println!("cargo:rerun-if-changed={}", root.join(file).display());
    }
    // Every input is read exactly once. The build ID and the manifest are both derived from
    // these snapshots, so an edit made during the build cannot make them disagree.
    let inputs = read_inputs(root, &files).expect("read build inputs");

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
    let build_id = fingerprint(&inputs, &metadata);
    let manifest = build_manifest(&inputs, &build_id, &target, &profile, compiler.trim());
    let output = PathBuf::from(env::var_os("OUT_DIR").expect("Cargo output directory"));
    fs::write(
        output.join(format!("build-inputs-{build_id}.json")),
        manifest,
    )
    .expect("write build input manifest");
    if target.contains("-windows-") {
        // Use the same snapshot as the fingerprint so an edit during the build cannot silently
        // embed pixels from a different source revision. The PNG still supplies the live window icon.
        let icon = inputs
            .iter()
            .find(|input| relative_name(&input.path) == "budget-gui/assets/budget-calendar.ico")
            .expect("application icon in build input inventory");
        embed_windows_icon(&icon.identity, &output, &target)
            .expect("embed Windows application icon");
    }
    println!("cargo:rustc-env=BUDGET_BUILD_ID={build_id}");
    println!("cargo:rustc-env=BUDGET_BUILD_TARGET={target}");
    println!("cargo:rustc-env=BUDGET_BUILD_PROFILE={profile}");
}

/// Produce a standard Windows .res file without depending on an SDK resource compiler. Each ICO
/// image becomes RT_ICON; RT_GROUP_ICON keeps the size metadata and references those image IDs.
/// MSVC accepts this directly. MinGW's linker needs the same resource converted to a COFF object.
fn icon_resources(ico: &[u8]) -> io::Result<Vec<u8>> {
    let invalid = || {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid application ICO directory or image bounds",
        )
    };
    if ico.len() < 6 || ico[..4] != [0, 0, 1, 0] {
        return Err(invalid());
    }
    let count = u16::from_le_bytes([ico[4], ico[5]]);
    let directory_end = 6 + usize::from(count) * 16;
    if count == 0 || directory_end > ico.len() {
        return Err(invalid());
    }
    let mut resource = Vec::new();
    append_resource(&mut resource, 0, 0, &[]); // Required null resource marks the 32-bit .res format.
    let mut group = ico[..6].to_vec();
    for index in 0..count {
        let start = 6 + usize::from(index) * 16;
        let entry = &ico[start..start + 16];
        let size = u32::from_le_bytes(entry[8..12].try_into().unwrap()) as usize;
        let offset = u32::from_le_bytes(entry[12..16].try_into().unwrap()) as usize;
        let end = offset.checked_add(size).ok_or_else(invalid)?;
        if size == 0 || offset < directory_end || end > ico.len() {
            return Err(invalid());
        }
        let id = index + 1;
        append_resource(&mut resource, 3, id, &ico[offset..end]);
        group.extend_from_slice(&entry[..12]);
        group.extend_from_slice(&id.to_le_bytes());
    }
    append_resource(&mut resource, 14, 1, &group);
    Ok(resource)
}

fn append_resource(output: &mut Vec<u8>, kind: u16, id: u16, data: &[u8]) {
    output.extend_from_slice(&(data.len() as u32).to_le_bytes());
    output.extend_from_slice(&32u32.to_le_bytes()); // Header size, including DataSize/HeaderSize.
    for value in [0xffff, kind, 0xffff, id] {
        output.extend_from_slice(&value.to_le_bytes());
    }
    output.extend_from_slice(&0u32.to_le_bytes()); // DataVersion
    output.extend_from_slice(&(if kind == 0 { 0u16 } else { 0x1030u16 }).to_le_bytes()); // moveable, pure, discardable
    output.extend_from_slice(&0u16.to_le_bytes()); // Neutral language
    output.extend_from_slice(&0u32.to_le_bytes()); // Version
    output.extend_from_slice(&0u32.to_le_bytes()); // Characteristics
    output.extend_from_slice(data);
    while output.len() % 4 != 0 {
        output.push(0);
    }
}

fn embed_windows_icon(ico: &[u8], output: &Path, target: &str) -> io::Result<()> {
    let resource = output.join("budget-calendar.res");
    fs::write(&resource, icon_resources(ico)?)?;
    let link_input = if target.ends_with("-gnu") || target.ends_with("-gnullvm") {
        println!("cargo:rerun-if-env-changed=WINDRES");
        let object = output.join("budget-calendar-icon.o");
        let candidates = if let Some(configured) = env::var_os("WINDRES") {
            vec![configured]
        } else {
            let architecture = target.split('-').next().unwrap_or("x86_64");
            vec![
                format!("{architecture}-w64-mingw32-windres").into(),
                "windres".into(),
            ]
        };
        let mut found = false;
        for compiler in candidates {
            let result = Command::new(&compiler)
                .args(["--input-format=res", "--output-format=coff", "--input"])
                .arg(&resource)
                .arg("--output")
                .arg(&object)
                .output();
            match result {
                Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
                Err(error) => return Err(error),
                Ok(result) if !result.status.success() => {
                    return Err(io::Error::other(format!(
                        "{} failed to convert the application icon: {}",
                        compiler.to_string_lossy(),
                        String::from_utf8_lossy(&result.stderr)
                    )))
                }
                Ok(_) => {
                    found = true;
                    break;
                }
            }
        }
        if !found {
            return Err(io::Error::new(io::ErrorKind::NotFound, "Windows GNU builds require MinGW windres on PATH or the WINDRES environment variable"));
        }
        object
    } else {
        resource
    };
    println!(
        "cargo:rustc-link-arg-bin=budget-gui={}",
        link_input.display()
    );
    Ok(())
}

fn source_inputs(root: &Path) -> io::Result<Vec<PathBuf>> {
    // Keep this allowlist aligned with scripts/archive-release.ps1. Local budgets and
    // generated/progress files must never affect a build or enter its source archive.
    let mut files = [
        "Cargo.toml",
        "Cargo.lock",
        "rust-toolchain.toml",
        ".cargo/config.toml",
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
        "README.md",
        "budget-engine/Cargo.toml",
        "budget-mobile/package.json",
        "budget-mobile/README.md",
        "budget-mobile/ENGINE-CONTRACT.md",
        "budget-mobile/SERVER-CONTRACT.md",
        "budget-mobile/server/DEPLOYMENT.md",
        "budget-mobile/Dockerfile",
        "budget-mobile/.dockerignore",
    ]
    .into_iter()
    .map(PathBuf::from)
    .collect::<Vec<_>>();
    for (directory, extensions) in [
        ("budget-gui/src", &["rs"][..]),
        ("budget-gui/assets", &["ico", "png", "svg"][..]),
        ("budget-core/src", &["rs"][..]),
        ("budget-core/tests", &["rs"][..]),
        ("budget-engine/src", &["rs"][..]),
        ("budget-engine/tests", &["mjs"][..]),
        (
            "budget-mobile/public",
            &["mjs", "js", "html", "css", "webmanifest", "png", "wasm"][..],
        ),
        ("budget-mobile/server", &["mjs"][..]),
        ("budget-mobile/tests", &["mjs"][..]),
        ("vendor/webbrowser/src", &["rs"][..]),
        ("scripts", &["ps1"][..]),
        ("installer", &["nsi", "ps1"][..]),
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
            "node_modules",
            "data",
            "private",
            "secrets",
            "credentials",
            "backups",
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

/// One source input as this build read it. `identity` holds the bytes that the build ID and the
/// manifest describe: text is line-ending normalised, binary files are exactly as stored.
struct Input {
    path: PathBuf,
    identity: Vec<u8>,
}

fn read_inputs(root: &Path, files: &[PathBuf]) -> io::Result<Vec<Input>> {
    let mut paths = files.to_vec();
    paths.sort_by_key(|path| relative_name(path));
    paths.dedup_by(|a, b| relative_name(a) == relative_name(b));
    paths
        .into_iter()
        .map(|path| {
            let bytes = fs::read(root.join(&path))?;
            Ok(Input {
                path,
                identity: identity_bytes(bytes),
            })
        })
        .collect()
}

/// Git stores text under `text=auto` with LF line endings, and a Windows checkout turns them into
/// CRLF. Hashing the LF form makes one commit give one build ID on every checkout. A NUL byte marks
/// a binary file, which is hashed exactly as stored, matching git's own text/binary split.
fn identity_bytes(bytes: Vec<u8>) -> Vec<u8> {
    if bytes.contains(&0) {
        return bytes;
    }
    let mut identity = Vec::with_capacity(bytes.len());
    for (index, byte) in bytes.iter().enumerate() {
        if *byte == b'\r' && bytes.get(index + 1) == Some(&b'\n') {
            continue;
        }
        identity.push(*byte);
    }
    identity
}

fn fingerprint(inputs: &[Input], metadata: &[(&str, &str)]) -> String {
    let mut hash = 0xcbf29ce484222325u64;
    hash_field(&mut hash, b"budget-calendar-build-id-v1");
    let mut inputs = inputs.iter().collect::<Vec<_>>();
    inputs.sort_by_key(|input| relative_name(&input.path));
    for input in inputs {
        hash_field(&mut hash, relative_name(&input.path).as_bytes());
        hash_field(&mut hash, &input.identity);
    }
    let mut metadata = metadata.to_vec();
    metadata.sort_unstable();
    for (key, value) in metadata {
        hash_field(&mut hash, key.as_bytes());
        hash_field(&mut hash, value.as_bytes());
    }
    format!("{hash:016x}")
}

fn hash_field(hash: &mut u64, bytes: &[u8]) {
    for byte in (bytes.len() as u64).to_le_bytes().iter().chain(bytes) {
        *hash ^= u64::from(*byte);
        *hash = hash.wrapping_mul(0x100000001b3);
    }
}

fn build_manifest(
    inputs: &[Input],
    build_id: &str,
    target: &str,
    profile: &str,
    compiler: &str,
) -> String {
    let mut inputs = inputs.iter().collect::<Vec<_>>();
    inputs.sort_by_key(|input| relative_name(&input.path));
    let entries = inputs
        .iter()
        .map(|input| {
            format!(
                "{{\"path\":{},\"fnv1a\":\"{}\"}}",
                json_string(&relative_name(&input.path)),
                content_hash(&input.identity)
            )
        })
        .collect::<Vec<_>>();
    format!(
        "{{\"build_id\":{},\"target\":{},\"profile\":{},\"compiler\":{},\"inputs\":[{}]}}",
        json_string(build_id),
        json_string(target),
        json_string(profile),
        json_string(compiler),
        entries.join(",")
    )
}

/// FNV-1a is kept for input digests. SHA-256 would need a sha2 dependency, which is in neither
/// Cargo.lock nor the offline Cargo cache; the release archive records SHA-256 for every file it keeps.
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

    fn icon_fixture() -> Vec<u8> {
        // Two different-sized payloads deliberately require different amounts of resource padding.
        // The encoder preserves PNG/DIB bytes; Windows is responsible for decoding their pixels.
        let mut ico = vec![0, 0, 1, 0, 2, 0];
        for (size, length, offset) in [(16u8, 5u32, 38u32), (0, 7, 43)] {
            ico.extend_from_slice(&[size, size, 0, 0, 1, 0, 32, 0]);
            ico.extend_from_slice(&length.to_le_bytes());
            ico.extend_from_slice(&offset.to_le_bytes());
        }
        ico.extend_from_slice(b"smallLARGE!!");
        ico
    }

    #[test]
    fn icon_resources_preserve_every_frame_and_group_reference_with_dword_alignment() {
        let ico = icon_fixture();
        let res = icon_resources(&ico).unwrap();
        let mut offset = 0;
        let mut entries = Vec::new();
        while offset < res.len() {
            assert_eq!(offset % 4, 0, "resource header must be DWORD-aligned");
            let data_size =
                u32::from_le_bytes(res[offset..offset + 4].try_into().unwrap()) as usize;
            let header_size =
                u32::from_le_bytes(res[offset + 4..offset + 8].try_into().unwrap()) as usize;
            assert_eq!(header_size, 32);
            assert_eq!(&res[offset + 8..offset + 10], &[0xff, 0xff]);
            assert_eq!(&res[offset + 12..offset + 14], &[0xff, 0xff]);
            let kind = u16::from_le_bytes(res[offset + 10..offset + 12].try_into().unwrap());
            let id = u16::from_le_bytes(res[offset + 14..offset + 16].try_into().unwrap());
            let start = offset + header_size;
            let end = start + data_size;
            entries.push((kind, id, &res[start..end]));
            offset = (end + 3) & !3;
            assert!(res[end..offset].iter().all(|byte| *byte == 0));
        }
        assert_eq!(offset, res.len());
        assert_eq!(entries.len(), 4);
        assert_eq!(entries[0], (0, 0, &[][..]), "Windows .res signature");
        assert_eq!(entries[1], (3, 1, &b"small"[..]));
        assert_eq!(entries[2], (3, 2, &b"LARGE!!"[..]));
        let (kind, id, group) = entries[3];
        assert_eq!((kind, id), (14, 1));
        assert_eq!(&group[..6], &ico[..6]);
        assert_eq!(group.len(), 6 + 2 * 14);
        for frame in 0..2 {
            let source = 6 + frame * 16;
            let destination = 6 + frame * 14;
            assert_eq!(
                &group[destination..destination + 12],
                &ico[source..source + 12]
            );
            assert_eq!(
                u16::from_le_bytes(
                    group[destination + 12..destination + 14]
                        .try_into()
                        .unwrap()
                ),
                frame as u16 + 1
            );
        }
    }

    #[test]
    fn icon_resources_reject_invalid_directory_and_out_of_bounds_frames() {
        let valid = icon_fixture();
        for length in 0..38 {
            assert!(
                icon_resources(&valid[..length]).is_err(),
                "accepted truncated directory of {length} bytes"
            );
        }
        for (position, bytes) in [
            (0, vec![1]),           // reserved header
            (2, vec![2]),           // a cursor is not an icon
            (4, vec![0, 0]),        // empty icon group
            (14, vec![0, 0, 0, 0]), // zero-length first payload
            (18, vec![0, 0, 0, 0]), // payload points into directory
            (18, u32::MAX.to_le_bytes().to_vec()),
            (14, u32::MAX.to_le_bytes().to_vec()),
        ] {
            let mut malformed = valid.clone();
            malformed[position..position + bytes.len()].copy_from_slice(&bytes);
            assert!(
                icon_resources(&malformed).is_err(),
                "accepted malformed entry at {position}"
            );
        }
        assert!(icon_resources(&valid[..valid.len() - 1]).is_err());
    }

    static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

    /// Fixtures live beside this test executable (target/ per AGENTS.md), never in the system temp directory.
    fn fixture_base() -> PathBuf {
        env::current_exe()
            .expect("test executable path")
            .parent()
            .expect("test executable directory")
            .join("build-id-fixtures")
    }

    /// Text inputs have several lines so line-ending tests mean something; icons hold NUL bytes like real images.
    fn fixture_bytes(path: &str) -> Vec<u8> {
        if path.ends_with(".png") || path.ends_with(".ico") {
            b"\x89PNG\r\n\x1a\n\0fixture binary".to_vec()
        } else {
            format!("{path}\nfixture line\n").into_bytes()
        }
    }

    struct Fixture(PathBuf);

    impl Fixture {
        fn new() -> Self {
            let id = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
            let root = fixture_base().join(format!("budget-build-id-{}-{id}", std::process::id()));
            fs::create_dir_all(&root).unwrap();
            let fixture = Self(root);
            for path in [
                "Cargo.toml",
                "Cargo.lock",
                "rust-toolchain.toml",
                ".cargo/config.toml",
                ".gitattributes",
                ".gitignore",
                "SPEC.md",
                "CHANGELOG.md",
                "AGENTS.md",
                "docs/ux.md",
                "README.md",
                "budget-engine/Cargo.toml",
                "budget-mobile/package.json",
                "budget-mobile/README.md",
                "budget-mobile/ENGINE-CONTRACT.md",
                "budget-mobile/SERVER-CONTRACT.md",
                "budget-mobile/server/DEPLOYMENT.md",
                "budget-mobile/Dockerfile",
                "budget-mobile/.dockerignore",
                "budget-engine/src/lib.rs",
                "budget-engine/tests/parity.mjs",
                "budget-mobile/public/app.mjs",
                "budget-mobile/public/sw.js",
                "budget-mobile/public/index.html",
                "budget-mobile/public/styles.css",
                "budget-mobile/public/manifest.webmanifest",
                "budget-mobile/public/icon-192.png",
                "budget-mobile/public/budget_engine.wasm",
                "budget-mobile/server/server.mjs",
                "budget-mobile/tests/server.test.mjs",
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
                fixture.write(path, &fixture_bytes(path));
            }
            fixture
        }

        fn write(&self, path: &str, bytes: &[u8]) {
            let path = self.0.join(path);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, bytes).unwrap();
        }

        fn id(&self, metadata: &[(&str, &str)]) -> String {
            fingerprint(&self.inputs(), metadata)
        }

        fn inputs(&self) -> Vec<Input> {
            read_inputs(&self.0, &source_inputs(&self.0).unwrap()).unwrap()
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let root = self.0.canonicalize().unwrap();
            let base = fixture_base().canonicalize().unwrap();
            assert!(root.starts_with(base));
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
        fixture.write("installer/budget.nsi", b"installer script");
        fixture.write("installer/build-installer.ps1", b"installer wrapper");
        for path in [
            "target/generated.rs",
            "budget-gui/target/generated.rs",
            "local-releases/v0.3.0/build.rs",
            ".git/config",
            "docs/superpowers/plans/notes.rs",
            "docs/screenshot.png",
            "scripts/private.bgt",
            "budget-gui/assets/private.bgt",
            "budget-mobile/public/cached-budget.json",
            "budget-mobile/public/private.sqlite",
            "budget-mobile/public/.env",
            "budget-mobile/public/local.bgt",
            "budget-mobile/server/credentials.json",
            "budget-engine/tests/private.json",
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
            ".cargo/config.toml",
            ".gitattributes",
            ".gitignore",
            "SPEC.md",
            "CHANGELOG.md",
            "AGENTS.md",
            "docs/ux.md",
            "README.md",
            "budget-engine/Cargo.toml",
            "budget-mobile/package.json",
            "budget-mobile/README.md",
            "budget-mobile/ENGINE-CONTRACT.md",
            "budget-mobile/SERVER-CONTRACT.md",
            "budget-mobile/server/DEPLOYMENT.md",
            "budget-mobile/Dockerfile",
            "budget-mobile/.dockerignore",
            "budget-engine/src/lib.rs",
            "budget-engine/tests/parity.mjs",
            "budget-mobile/public/app.mjs",
            "budget-mobile/public/sw.js",
            "budget-mobile/public/index.html",
            "budget-mobile/public/styles.css",
            "budget-mobile/public/manifest.webmanifest",
            "budget-mobile/public/icon-192.png",
            "budget-mobile/public/budget_engine.wasm",
            "budget-mobile/server/server.mjs",
            "budget-mobile/tests/server.test.mjs",
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
            "installer/budget.nsi",
            "installer/build-installer.ps1",
        ] {
            assert!(
                paths.iter().any(|path| path == expected),
                "missing input: {expected}"
            );
        }
        assert_eq!(
            paths.len(),
            43,
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
            "Node_Modules",
            "Data",
            "Private",
            "Secrets",
            "Credentials",
            "Backups",
        ] {
            fixture.write(
                &format!("budget-core/src/{directory}/generated.rs"),
                b"generated",
            );
            fixture.write(&format!("scripts/{directory}/generated.ps1"), b"generated");
            fixture.write(
                &format!("budget-mobile/public/{directory}/private.mjs"),
                b"private",
            );
            fixture.write(
                &format!("budget-engine/tests/{directory}/generated.mjs"),
                b"generated",
            );
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
        let mut inputs = first.inputs();
        let original = fingerprint(&inputs, &metadata);
        inputs.reverse();
        assert_eq!(original, fingerprint(&inputs, &metadata));
        first.write(
            "budget-core/src/lib.rs",
            &fixture_bytes("budget-core/src/lib.rs"),
        );
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
            "README.md",
            "budget-engine/Cargo.toml",
            "budget-mobile/package.json",
            "budget-mobile/README.md",
            "budget-mobile/ENGINE-CONTRACT.md",
            "budget-mobile/SERVER-CONTRACT.md",
            "budget-mobile/server/DEPLOYMENT.md",
            "budget-mobile/Dockerfile",
            "budget-mobile/.dockerignore",
            "budget-engine/src/lib.rs",
            "budget-engine/tests/parity.mjs",
            "budget-mobile/public/app.mjs",
            "budget-mobile/public/sw.js",
            "budget-mobile/public/index.html",
            "budget-mobile/public/styles.css",
            "budget-mobile/public/manifest.webmanifest",
            "budget-mobile/public/icon-192.png",
            "budget-mobile/public/budget_engine.wasm",
            "budget-mobile/server/server.mjs",
            "budget-mobile/tests/server.test.mjs",
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
        let cargo_toml = read_inputs(&fixture.0, &[PathBuf::from("Cargo.toml")]).unwrap();
        let cargo_lock = read_inputs(&fixture.0, &[PathBuf::from("Cargo.lock")]).unwrap();
        assert_ne!(fingerprint(&cargo_toml, &[]), fingerprint(&cargo_lock, &[]));
    }

    /// Rewrites every text input the way a core.autocrlf=true Windows checkout stores it.
    fn checkout_with_crlf(fixture: &Fixture) {
        for file in source_inputs(&fixture.0).unwrap() {
            let path = fixture.0.join(&file);
            let bytes = fs::read(&path).unwrap();
            if bytes.contains(&0) {
                continue;
            }
            let text = String::from_utf8(bytes).unwrap();
            fs::write(&path, text.replace('\n', "\r\n")).unwrap();
        }
    }

    #[test]
    fn crlf_and_lf_checkouts_of_the_same_source_have_one_build_id() {
        let lf = Fixture::new();
        let crlf = Fixture::new();
        checkout_with_crlf(&crlf);
        assert_ne!(
            fs::read(lf.0.join("budget-core/src/lib.rs")).unwrap(),
            fs::read(crlf.0.join("budget-core/src/lib.rs")).unwrap(),
            "fixture did not get CRLF line endings"
        );
        assert_eq!(
            lf.id(&[]),
            crlf.id(&[]),
            "line endings changed the build identity"
        );
        assert_eq!(
            build_manifest(&lf.inputs(), "one-id", "t", "release", "rustc"),
            build_manifest(&crlf.inputs(), "one-id", "t", "release", "rustc"),
            "line endings changed the manifest"
        );
    }

    #[test]
    fn text_line_endings_are_normalised_but_binary_and_lone_carriage_returns_are_not() {
        assert_eq!(identity_bytes(b"a\r\nb\r\n".to_vec()), b"a\nb\n");
        assert_eq!(identity_bytes(b"a\rb\n".to_vec()), b"a\rb\n");
        assert_eq!(identity_bytes(b"\0\r\n".to_vec()), b"\0\r\n");
        // Known answer shared with scripts/tests/archive-release.Tests.ps1, which must digest the same text alike.
        assert_eq!(
            content_hash(&identity_bytes(b"a\r\nb\r\n".to_vec())),
            "78ed6781f136a14e"
        );
        assert_eq!(
            content_hash(&identity_bytes(b"\0\r\n".to_vec())),
            content_hash(b"\0\r\n")
        );
    }

    #[test]
    fn binary_inputs_are_hashed_exactly_so_their_bytes_still_count() {
        let original = Fixture::new();
        let changed = Fixture::new();
        changed.write(
            "budget-gui/assets/icon.png",
            b"\x89PNG\n\x1a\n\0fixture binary",
        );
        assert_ne!(original.id(&[]), changed.id(&[]));
    }

    #[test]
    fn fixtures_are_written_beside_the_test_executable_not_in_the_system_temp_directory() {
        let fixture = Fixture::new();
        let executable_directory = env::current_exe()
            .unwrap()
            .parent()
            .unwrap()
            .canonicalize()
            .unwrap();
        let root = fixture.0.canonicalize().unwrap();
        assert!(
            root.starts_with(&executable_directory),
            "fixture {} is outside {}",
            root.display(),
            executable_directory.display()
        );
        assert!(!root.starts_with(env::temp_dir().canonicalize().unwrap()));
    }

    #[test]
    fn build_inputs_are_read_once_and_the_manifest_uses_that_snapshot() {
        let fixture = Fixture::new();
        let inputs = fixture.inputs();
        let build_id = fingerprint(&inputs, &[]);
        // Once the snapshot exists, neither the build ID nor the manifest may touch the disk again.
        fs::remove_dir_all(&fixture.0).unwrap();
        let manifest = build_manifest(&inputs, &build_id, "t", "release", "rustc");
        let expected = content_hash(&fixture_bytes("budget-core/src/lib.rs"));
        assert!(
            manifest.contains(&format!(
                "\"path\":\"budget-core/src/lib.rs\",\"fnv1a\":\"{expected}\""
            )),
            "manifest was not built from the snapshot: {manifest}"
        );
        assert_eq!(fingerprint(&inputs, &[]), build_id);
        fs::create_dir_all(&fixture.0).unwrap();
    }

    #[test]
    fn build_manifest_records_identity_and_verifiable_source_content() {
        let fixture = Fixture::new();
        fixture.write("budget-core/src/lib.rs", b"foobar");
        let inputs = read_inputs(&fixture.0, &[PathBuf::from("budget-core/src/lib.rs")]).unwrap();
        let manifest = build_manifest(
            &inputs,
            "0123456789abcdef",
            "fixture-target",
            "release",
            "rustc fixture",
        );
        assert_eq!(manifest, "{\"build_id\":\"0123456789abcdef\",\"target\":\"fixture-target\",\"profile\":\"release\",\"compiler\":\"rustc fixture\",\"inputs\":[{\"path\":\"budget-core/src/lib.rs\",\"fnv1a\":\"85944171f73967e8\"}]}");
        assert_eq!(content_hash(b""), "cbf29ce484222325");
        assert_eq!(content_hash(b"a"), "af63dc4c8601ec8c");
    }
}
