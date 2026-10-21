use crate::Result;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::HashSet,
    fs::{self, File},
    io::{Read, Write},
    path::{Component, Path, PathBuf},
};
pub const SCHEMA: &str = "launcher-mod-package-v1";
const MAX_TOTAL: u64 = 128 * 1024 * 1024;
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Resource {
    pub path: String,
    pub bytes: u64,
    pub sha256: String,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Manifest {
    pub schema: String,
    pub id: String,
    pub name: String,
    pub author: String,
    pub version: String,
    pub protocol: String,
    pub entrypoint: String,
    pub capabilities: Vec<String>,
    pub files: Vec<Resource>,
    pub config_schema: serde_json::Value,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Approval {
    pub schema: String,
    pub manifest_sha256: String,
    pub policy: String,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Preview {
    pub manifest: Manifest,
    pub manifest_sha256: String,
    pub total_bytes: u64,
    pub trust: String,
    pub installed_version: Option<String>,
}
fn bad() -> String {
    "Mod 包结构、摘要或版本校验失败，未执行程序。".into()
}
pub fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
        && !id.starts_with('-')
}
pub fn version(text: &str) -> Result<[u32; 3]> {
    let parts = text
        .split('.')
        .map(str::parse::<u32>)
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|_| bad())?;
    parts.try_into().map_err(|_| bad())
}
pub fn relative(text: &str) -> Result<PathBuf> {
    if text.is_empty() || text.len() > 240 || text.contains(['\\', ':']) {
        return Err(bad());
    }
    let path = Path::new(text);
    if path.is_absolute() {
        return Err(bad());
    }
    for component in path.components() {
        let Component::Normal(part) = component else {
            return Err(bad());
        };
        let part = part.to_str().ok_or_else(bad)?;
        if part.ends_with(['.', ' '])
            || part.chars().any(|c| c.is_control())
            || part.starts_with('.')
        {
            return Err(bad());
        }
        let stem = part.split('.').next().unwrap().to_ascii_uppercase();
        if ["CON", "PRN", "AUX", "NUL"].contains(&stem.as_str())
            || ((stem.starts_with("COM") || stem.starts_with("LPT"))
                && stem.len() == 4
                && matches!(stem.as_bytes()[3], b'1'..=b'9'))
        {
            return Err(bad());
        }
    }
    Ok(path.into())
}
fn regular(path: &Path) -> Result<fs::Metadata> {
    let meta = fs::symlink_metadata(path).map_err(|_| bad())?;
    if meta.file_type().is_symlink() {
        return Err(bad());
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if meta.file_attributes() & 0x400 != 0 {
            return Err(bad());
        }
    }
    Ok(meta)
}
fn bounded(path: &Path, limit: u64) -> Result<Vec<u8>> {
    if !regular(path)?.is_file() {
        return Err(bad());
    }
    let mut bytes = Vec::new();
    File::open(path)
        .map_err(|_| bad())?
        .take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| bad())?;
    if bytes.len() as u64 > limit {
        return Err(bad());
    }
    Ok(bytes)
}
pub fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
pub fn inspect(source: &Path) -> Result<Preview> {
    if !regular(source)?.is_dir() {
        return Err(bad());
    }
    let raw = bounded(&source.join("launcher-mod.json"), 65536)?;
    let m: Manifest = serde_json::from_slice(&raw).map_err(|_| bad())?;
    if m.schema != SCHEMA
        || !valid_id(&m.id)
        || m.protocol != "launcher-mod-http-v1"
        || m.name.is_empty()
        || m.name.len() > 128
        || m.author.is_empty()
        || m.author.len() > 128
        || m.files.is_empty()
        || m.files.len() > 256
        || !m.config_schema.is_object()
    {
        return Err(bad());
    }
    version(&m.version)?;
    relative(&m.entrypoint)?;
    if !m.entrypoint.ends_with(".exe") {
        return Err(bad());
    }
    if m.capabilities.is_empty()
        || !m.capabilities.iter().any(|c| c == "settings.http")
        || m.capabilities
            .iter()
            .any(|c| !["settings.http", "native-game-child"].contains(&c.as_str()))
    {
        return Err(bad());
    }
    let mut names = HashSet::new();
    let mut total = 0u64;
    let canonical = source.canonicalize().map_err(|_| bad())?;
    for file in &m.files {
        let path = relative(&file.path)?;
        if file.path == "launcher-mod.json"
            || !names.insert(file.path.to_ascii_lowercase())
            || file.sha256.len() != 64
            || !file.sha256.bytes().all(|b| b.is_ascii_hexdigit())
        {
            return Err(bad());
        }
        total = total
            .checked_add(file.bytes)
            .filter(|n| *n <= MAX_TOTAL)
            .ok_or_else(bad)?;
        let full = source.join(path);
        if !full
            .canonicalize()
            .map_err(|_| bad())?
            .starts_with(&canonical)
        {
            return Err(bad());
        }
        let data = bounded(&full, MAX_TOTAL)?;
        if data.len() as u64 != file.bytes || digest(&data) != file.sha256.to_ascii_lowercase() {
            return Err(bad());
        }
    }
    if !names.contains(&m.entrypoint.to_ascii_lowercase()) {
        return Err(bad());
    }
    let mut directories = vec![source.to_path_buf()];
    let mut observed = 0usize;
    while let Some(dir) = directories.pop() {
        for entry in fs::read_dir(&dir).map_err(|_| bad())? {
            let path = entry.map_err(|_| bad())?.path();
            let metadata = regular(&path)?;
            let rel = path
                .strip_prefix(source)
                .map_err(|_| bad())?
                .to_str()
                .ok_or_else(bad)?
                .replace('\\', "/");
            if metadata.is_dir() {
                if !m
                    .files
                    .iter()
                    .any(|f| f.path.starts_with(&(rel.clone() + "/")))
                {
                    return Err(bad());
                }
                directories.push(path)
            } else {
                observed += 1;
                if observed > 258
                    || (!names.contains(&rel.to_ascii_lowercase())
                        && rel != "launcher-mod.json"
                        && rel != "receipt.json")
                {
                    return Err(bad());
                }
            }
        }
    }
    Ok(Preview {
        manifest: m,
        manifest_sha256: digest(&raw),
        total_bytes: total,
        trust: "unsigned-manual-digest-approval".into(),
        installed_version: None,
    })
}
pub fn package_root(root: &Path, id: &str) -> Result<PathBuf> {
    if !valid_id(id) {
        return Err(bad());
    }
    for path in [
        root.to_path_buf(),
        root.join("packages"),
        root.join("packages").join(id),
    ] {
        if path.exists() && !regular(&path)?.is_dir() {
            return Err(bad());
        }
    }
    Ok(root.join("packages").join(id))
}
pub fn current(root: &Path, id: &str) -> Result<PathBuf> {
    Ok(package_root(root, id)?.join("current"))
}
pub fn approved(path: &Path) -> Result<Preview> {
    let p = inspect(path)?;
    let receipt: Approval =
        serde_json::from_slice(&bounded(&path.join("receipt.json"), 4096)?).map_err(|_| bad())?;
    if receipt.schema != "launcher-mod-approval-v1"
        || receipt.policy != "manual-digest-approval"
        || receipt.manifest_sha256 != p.manifest_sha256
    {
        return Err(bad());
    }
    Ok(p)
}
/// A UI listing reads the approved manifest only; execution rechecks every byte.
pub fn approved_metadata(path: &Path) -> Result<Preview> {
    let raw = bounded(&path.join("launcher-mod.json"), 65536)?;
    let m: Manifest = serde_json::from_slice(&raw).map_err(|_| bad())?;
    let receipt: Approval =
        serde_json::from_slice(&bounded(&path.join("receipt.json"), 4096)?).map_err(|_| bad())?;
    if m.schema != SCHEMA
        || !valid_id(&m.id)
        || receipt.schema != "launcher-mod-approval-v1"
        || receipt.policy != "manual-digest-approval"
        || receipt.manifest_sha256 != digest(&raw)
    {
        return Err(bad());
    }
    version(&m.version)?;
    let total = m.files.iter().try_fold(0u64, |n, f| {
        n.checked_add(f.bytes)
            .filter(|n| *n <= MAX_TOTAL)
            .ok_or_else(bad)
    })?;
    Ok(Preview {
        manifest: m,
        manifest_sha256: receipt.manifest_sha256,
        total_bytes: total,
        trust: "unsigned-manual-digest-approval".into(),
        installed_version: None,
    })
}
fn write_new(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut f = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|_| bad())?;
    f.write_all(bytes)
        .and_then(|_| f.sync_all())
        .map_err(|_| bad())
}
fn remove_owned(parent: &Path, path: &Path) -> Result<()> {
    let parent = parent.canonicalize().map_err(|_| bad())?;
    let actual = path.canonicalize().map_err(|_| bad())?;
    if !actual.starts_with(&parent) || actual == parent || !regular(path)?.is_dir() {
        return Err(bad());
    }
    fs::remove_dir_all(path).map_err(|_| bad())
}
pub fn recover(root: &Path, id: &str) -> Result<()> {
    let dir = package_root(root, id)?;
    if !dir.exists() {
        return Ok(());
    }
    let now = dir.join("current");
    let prev = dir.join("previous");
    let rollback = dir.join("rollback-staging");
    if rollback.exists() {
        approved(&rollback)?;
        if !now.exists() {
            if prev.exists() {
                fs::rename(&prev, &now).map_err(|_| bad())?;
                fs::rename(&rollback, &prev).map_err(|_| bad())?
            } else {
                fs::rename(&rollback, &now).map_err(|_| bad())?
            }
        } else if !prev.exists() {
            fs::rename(&rollback, &prev).map_err(|_| bad())?
        } else {
            return Err("回滚目录状态冲突，未删除任何版本。".into());
        }
    }
    if !now.exists() && prev.exists() {
        approved(&prev)?;
        fs::rename(&prev, &now).map_err(|_| bad())?;
    }
    let retired = dir.join("retired");
    if retired.exists() {
        approved(&retired)?;
        if !prev.exists() {
            fs::rename(&retired, &prev).map_err(|_| bad())?
        } else {
            approved(&now)?;
            approved(&prev)?;
            remove_owned(&dir, &retired)?
        }
    }
    let stage = dir.join("staging");
    let marker = dir.join("staging-owner.json");
    if marker.exists() {
        let owner: serde_json::Value =
            serde_json::from_slice(&bounded(&marker, 4096)?).map_err(|_| bad())?;
        if owner["schema"] != "launcher-mod-stage-v1" || owner["id"] != id {
            return Err(bad());
        }
        if stage.exists() {
            remove_owned(&dir, &stage)?;
        }
        fs::remove_file(marker).map_err(|_| bad())?;
    } else if stage.exists() {
        approved(&stage)?;
        if now.exists() {
            remove_owned(&dir, &stage)?;
        } else {
            fs::rename(&stage, &now).map_err(|_| bad())?;
        }
    }
    Ok(())
}
pub fn install(root: &Path, source: &Path, expected: &str) -> Result<Preview> {
    install_inner(root, source, expected, false)
}
fn install_inner(
    root: &Path,
    source: &Path,
    expected: &str,
    inject_failure: bool,
) -> Result<Preview> {
    let preview = inspect(source)?;
    if preview.manifest_sha256 != expected {
        return Err("Mod 包在确认后发生变化，请重新导入并确认摘要。".into());
    }
    let id = &preview.manifest.id;
    let dir = package_root(root, id)?;
    fs::create_dir_all(&dir).map_err(|_| bad())?;
    recover(root, id)?;
    let now = dir.join("current");
    let prev = dir.join("previous");
    let stage = dir.join("staging");
    if now.exists()
        && version(&approved(&now)?.manifest.version)? >= version(&preview.manifest.version)?
    {
        return Err("更新版本必须高于当前版本；降级请使用回滚。".into());
    }
    if stage.exists() {
        return Err("上次暂存目录仍在，请先检查导入中断记录，未覆盖任何版本。".into());
    }
    let marker = dir.join("staging-owner.json");
    write_new(
        &marker,
        &serde_json::to_vec(&serde_json::json!({"schema":"launcher-mod-stage-v1","id":id}))
            .map_err(|_| bad())?,
    )?;
    fs::create_dir(&stage).map_err(|_| bad())?;
    let staged = (|| {
        for file in &preview.manifest.files {
            let path = relative(&file.path)?;
            let bytes = bounded(&source.join(&path), MAX_TOTAL)?;
            if digest(&bytes) != file.sha256.to_ascii_lowercase() {
                return Err(bad());
            }
            let dest = stage.join(path);
            fs::create_dir_all(dest.parent().unwrap()).map_err(|_| bad())?;
            write_new(&dest, &bytes)?;
        }
        let raw = bounded(&source.join("launcher-mod.json"), 65536)?;
        if digest(&raw) != expected {
            return Err(bad());
        }
        write_new(&stage.join("launcher-mod.json"), &raw)?;
        write_new(
            &stage.join("receipt.json"),
            &serde_json::to_vec(&Approval {
                schema: "launcher-mod-approval-v1".into(),
                manifest_sha256: expected.into(),
                policy: "manual-digest-approval".into(),
            })
            .map_err(|_| bad())?,
        )?;
        approved(&stage)?;
        Ok(())
    })();
    if let Err(e) = staged {
        let _ = remove_owned(&dir, &stage);
        let _ = fs::remove_file(&marker);
        return Err(e);
    }
    let retired = dir.join("retired");
    if prev.exists() {
        approved(&prev)?;
        fs::rename(&prev, &retired).map_err(|_| bad())?;
    }
    if now.exists() {
        fs::rename(&now, &prev).map_err(|_| bad())?
    }
    if inject_failure {
        if prev.exists() {
            fs::rename(&prev, &now).map_err(|_| bad())?;
        }
        if retired.exists() {
            fs::rename(&retired, &prev).map_err(|_| bad())?;
        }
        let _ = remove_owned(&dir, &stage);
        let _ = fs::remove_file(&marker);
        return Err("模拟替换失败，已恢复原版本。".into());
    }
    if fs::rename(&stage, &now).is_err() {
        if prev.exists() {
            let _ = fs::rename(&prev, &now);
        }
        if retired.exists() {
            let _ = fs::rename(&retired, &prev);
        }
        let _ = remove_owned(&dir, &stage);
        let _ = fs::remove_file(&marker);
        return Err("更新未提交，已尝试恢复原版本。".into());
    }
    let verified = approved(&now)?;
    if retired.exists() {
        remove_owned(&dir, &retired)?;
    }
    fs::remove_file(marker).map_err(|_| bad())?;
    Ok(verified)
}
pub fn rollback(root: &Path, id: &str) -> Result<Preview> {
    recover(root, id)?;
    let dir = package_root(root, id)?;
    let now = dir.join("current");
    let prev = dir.join("previous");
    approved(&now)?;
    approved(&prev)?;
    let temp = dir.join("rollback-staging");
    if temp.exists() {
        return Err(bad());
    }
    fs::rename(&now, &temp).map_err(|_| bad())?;
    if fs::rename(&prev, &now).is_err() {
        let _ = fs::rename(&temp, &now);
        return Err(bad());
    }
    fs::rename(&temp, &prev).map_err(|_| bad())?;
    approved(&now)
}
pub fn uninstall(root: &Path, id: &str) -> Result<()> {
    recover(root, id)?;
    let dir = package_root(root, id)?;
    for name in ["current", "previous"] {
        let path = dir.join(name);
        if path.exists() {
            approved(&path)?;
            remove_owned(&dir, &path)?
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn failed_third_update_preserves_both_approved_versions() {
        let dir = temp();
        let store = dir.join("store");
        let a = fixture(&dir, "1.0.0", b"first");
        let b = fixture(&dir, "1.1.0", b"second");
        let c = fixture(&dir, "1.2.0", b"third");
        for source in [&a, &b] {
            let p = inspect(source).unwrap();
            install(&store, source, &p.manifest_sha256).unwrap();
        }
        let p = inspect(&c).unwrap();
        assert!(install_inner(&store, &c, &p.manifest_sha256, true).is_err());
        let package = package_root(&store, "sample-mod").unwrap();
        assert_eq!(
            approved(&package.join("current")).unwrap().manifest.version,
            "1.1.0"
        );
        assert_eq!(
            approved(&package.join("previous"))
                .unwrap()
                .manifest
                .version,
            "1.0.0"
        );
        install(&store, &c, &p.manifest_sha256).unwrap();
        assert_eq!(
            approved(&package.join("current")).unwrap().manifest.version,
            "1.2.0"
        );
        assert_eq!(
            approved(&package.join("previous"))
                .unwrap()
                .manifest
                .version,
            "1.1.0"
        );
        assert!(!package.join("retired").exists());
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn crash_recovery_restores_cutover_and_removes_only_owned_staging() {
        let dir = temp();
        let store = dir.join("store");
        let source = fixture(&dir, "1.0.0", b"first");
        let p = inspect(&source).unwrap();
        install(&store, &source, &p.manifest_sha256).unwrap();
        let package = package_root(&store, "sample-mod").unwrap();
        fs::rename(package.join("current"), package.join("previous")).unwrap();
        fs::create_dir(package.join("staging")).unwrap();
        fs::write(package.join("staging/partial.bin"), b"partial").unwrap();
        fs::write(
            package.join("staging-owner.json"),
            br#"{"schema":"launcher-mod-stage-v1","id":"sample-mod"}"#,
        )
        .unwrap();
        recover(&store, "sample-mod").unwrap();
        assert_eq!(
            approved(&package.join("current")).unwrap().manifest.version,
            "1.0.0"
        );
        assert!(!package.join("staging").exists());
        assert!(!package.join("staging-owner.json").exists());
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn unlisted_dlls_and_unsupported_signatures_are_rejected() {
        let dir = temp();
        let source = fixture(&dir, "1.0.0", b"first");
        fs::write(source.join("unlisted.dll"), b"unexpected").unwrap();
        assert!(inspect(&source).is_err());
        fs::remove_file(source.join("unlisted.dll")).unwrap();
        let manifest = source.join("launcher-mod.json");
        let mut value: serde_json::Value =
            serde_json::from_slice(&fs::read(&manifest).unwrap()).unwrap();
        value["signature"] = serde_json::json!({"algorithm":"unknown","value":"unverified"});
        fs::write(manifest, serde_json::to_vec(&value).unwrap()).unwrap();
        assert!(inspect(&source).is_err());
        fs::remove_dir_all(dir).unwrap();
    }
    fn fixture(root: &Path, version: &str, bytes: &[u8]) -> PathBuf {
        let source = root.join(format!("source-{version}"));
        fs::create_dir_all(&source).unwrap();
        fs::write(source.join("sample.exe"), bytes).unwrap();
        let m = Manifest {
            schema: SCHEMA.into(),
            id: "sample-mod".into(),
            name: "Sample".into(),
            author: "test".into(),
            version: version.into(),
            protocol: "launcher-mod-http-v1".into(),
            entrypoint: "sample.exe".into(),
            capabilities: vec!["settings.http".into()],
            files: vec![Resource {
                path: "sample.exe".into(),
                bytes: bytes.len() as u64,
                sha256: digest(bytes),
            }],
            config_schema: serde_json::json!({"type":"object"}),
        };
        fs::write(
            source.join("launcher-mod.json"),
            serde_json::to_vec(&m).unwrap(),
        )
        .unwrap();
        source
    }
    fn temp() -> PathBuf {
        let p = std::env::temp_dir().join(format!(
            "launcher-mod-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&p).unwrap();
        p
    }
    #[test]
    fn rejects_traversal_ads_devices_and_aliases() {
        for name in [
            "../a.exe", "/a.exe", "a:stream", "a\\b.exe", "a./b.exe", "CON.exe", "com1.bin",
            ".hidden",
        ] {
            assert!(relative(name).is_err(), "{name}")
        }
    }
    #[test]
    fn update_failure_rolls_back_and_config_is_not_overwritten() {
        let dir = temp();
        let store = dir.join("store");
        let a = fixture(&dir, "1.0.0", b"first");
        let b = fixture(&dir, "1.1.0", b"second");
        let p = inspect(&a).unwrap();
        install(&store, &a, &p.manifest_sha256).unwrap();
        let data = store.join("data/sample-mod");
        fs::create_dir_all(&data).unwrap();
        fs::write(data.join("config.json"), b"private-config").unwrap();
        let p = inspect(&b).unwrap();
        assert!(install_inner(&store, &b, &p.manifest_sha256, true).is_err());
        assert_eq!(
            approved(&current(&store, "sample-mod").unwrap())
                .unwrap()
                .manifest
                .version,
            "1.0.0"
        );
        install(&store, &b, &p.manifest_sha256).unwrap();
        assert_eq!(
            rollback(&store, "sample-mod").unwrap().manifest.version,
            "1.0.0"
        );
        assert_eq!(
            fs::read(data.join("config.json")).unwrap(),
            b"private-config"
        );
        uninstall(&store, "sample-mod").unwrap();
        assert!(data.join("config.json").exists());
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn altered_payload_and_approval_digest_are_rejected() {
        let dir = temp();
        let source = fixture(&dir, "1.0.0", b"first");
        let p = inspect(&source).unwrap();
        assert!(install(&dir.join("store"), &source, "wrong").is_err());
        fs::write(source.join("sample.exe"), b"other").unwrap();
        assert!(inspect(&source).is_err());
        assert!(!p.manifest_sha256.is_empty());
        fs::remove_dir_all(dir).unwrap();
    }
}
