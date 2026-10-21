//! Launcher-only adapter for generic packages. No MeowLink/account implementation.
use crate::{
    commands::{ActionRecord, AppState},
    session::Phase,
};
use launcher_mod_runtime::{package, process};
use serde::Serialize;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{LazyLock, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};
use tauri::AppHandle;
use tauri_plugin_dialog::{DialogExt, FilePath};
static PREVIEWS: LazyLock<Mutex<HashMap<String, (PathBuf, String, String)>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportPreview {
    ticket: String,
    preview: package::Preview,
}
fn main(window: &tauri::WebviewWindow) -> Result<(), String> {
    if window.label() != "main" {
        Err("仅主窗口可操作启动器 Mod。".into())
    } else {
        Ok(())
    }
}
pub fn root(root: &crate::paths::Root) -> PathBuf {
    root.path.join("launcher-mods")
}
pub fn selected(root: &crate::paths::Root) -> bool {
    root.path.join("launcher-mods/selection.json").exists()
}
pub fn stock_updater_compatible() -> bool {
    false
}
#[tauri::command]
pub async fn launcher_mod_local(
    window: tauri::WebviewWindow,
    app: AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<ActionRecord, String> {
    main(&window)?;
    let state = state.inner().clone();
    background(move || {
        let mut manager = process::lock()?;
        if manager.any_active()? {
            return Err("请先停止 Mod 实例再切回本地模式。".into());
        }
        let marker = root(&state.root).join("selection.json");
        if marker.exists() {
            std::fs::remove_file(marker).map_err(|_| "无法清除 Mod 模式选择。".to_string())?;
        }
        crate::prepare_environment(app, state.session, state.autofire);
        Ok(ActionRecord {
            message: "已显式切回本地模式。".into(),
        })
    })
    .await
}
fn select(root: &Path, id: &str) -> Result<(), String> {
    std::fs::create_dir_all(root).map_err(|_| "无法写入 Mod 选择。".to_string())?;
    std::fs::write(
        root.join("selection.json"),
        serde_json::to_vec(&serde_json::json!({"id":id})).unwrap(),
    )
    .map_err(|_| "无法保存 Mod 选择。".to_string())
}
fn local_idle(state: &AppState) -> Result<(), String> {
    if matches!(
        state.session.status().phase,
        Phase::Starting | Phase::Running | Phase::Stopping
    ) {
        Err("请先停止原本地实例再打开启动器 Mod。".into())
    } else {
        Ok(())
    }
}
async fn background<T: Send + 'static>(
    f: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|_| "Mod 后台任务异常。".to_string())?
}
#[tauri::command]
pub async fn launcher_mod_list(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, AppState>,
) -> Result<Vec<process::View>, String> {
    main(&window)?;
    let root = root(&state.root);
    background(move || process::lock()?.list(&root)).await
}
#[tauri::command]
pub async fn launcher_mod_pick(
    window: tauri::WebviewWindow,
    app: AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<Option<ImportPreview>, String> {
    main(&window)?;
    let root = root(&state.root);
    background(move || {
        let Some(picked) = app.dialog().file().blocking_pick_folder() else {
            return Ok(None);
        };
        let FilePath::Path(path) = picked else {
            return Err("只能导入本地 Mod 目录。".into());
        };
        let mut preview = package::inspect(&path)?;
        let mut manager = process::lock()?;
        manager.idle(&root, &preview.manifest.id)?;
        preview.installed_version =
            package::approved(&package::current(&root, &preview.manifest.id)?)
                .ok()
                .map(|p| p.manifest.version);
        let ticket = format!(
            "{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(|_| "系统时间无效。".to_string())?
                .as_nanos()
        );
        let mut previews = PREVIEWS.lock().map_err(|_| "导入状态异常。".to_string())?;
        if previews.len() >= 8 {
            previews.clear()
        }
        previews.insert(
            ticket.clone(),
            (
                path,
                preview.manifest_sha256.clone(),
                preview.manifest.id.clone(),
            ),
        );
        Ok(Some(ImportPreview { ticket, preview }))
    })
    .await
}
#[tauri::command]
pub async fn launcher_mod_approve(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, AppState>,
    ticket: String,
    manifest_sha256: String,
) -> Result<ActionRecord, String> {
    main(&window)?;
    let root = root(&state.root);
    background(move || {
        let (path, expected, id) = PREVIEWS
            .lock()
            .map_err(|_| "导入状态异常。".to_string())?
            .remove(&ticket)
            .ok_or_else(|| "导入确认已失效，请重新选择包。".to_string())?;
        if expected != manifest_sha256 {
            return Err("确认摘要不一致。".into());
        }
        let mut manager = process::lock()?;
        manager.idle(&root, &id)?;
        let installed = package::install(&root, &path, &expected)?;
        Ok(ActionRecord {
            message: format!(
                "已导入 {} {}；配置数据独立保存，尚未执行 Mod。",
                installed.manifest.name, installed.manifest.version
            ),
        })
    })
    .await
}
#[tauri::command]
pub async fn launcher_mod_open(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, AppState>,
    id: String,
) -> Result<String, String> {
    main(&window)?;
    let state = state.inner().clone();
    background(move || {
        let mut manager = process::lock()?;
        local_idle(&state)?;
        let root = root(&state.root);
        let url = manager.open(&root, &id)?;
        select(&root, &id)?;
        Ok(url)
    })
    .await
}
#[tauri::command]
pub async fn launcher_mod_stop(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, AppState>,
    id: String,
) -> Result<ActionRecord, String> {
    main(&window)?;
    let root = root(&state.root);
    background(move || {
        process::lock()?.stop(&root, &id)?;
        Ok(ActionRecord {
            message: "Mod 实例已停止。".into(),
        })
    })
    .await
}
#[tauri::command]
pub async fn launcher_mod_rollback(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, AppState>,
    id: String,
) -> Result<ActionRecord, String> {
    main(&window)?;
    let root = root(&state.root);
    background(move || {
        let mut manager = process::lock()?;
        manager.idle(&root, &id)?;
        let p = package::rollback(&root, &id)?;
        Ok(ActionRecord {
            message: format!("已回滚到 {}，配置数据保留。", p.manifest.version),
        })
    })
    .await
}
#[tauri::command]
pub async fn launcher_mod_uninstall(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, AppState>,
    id: String,
) -> Result<ActionRecord, String> {
    main(&window)?;
    let root = root(&state.root);
    background(move || {
        let mut manager = process::lock()?;
        manager.idle(&root, &id)?;
        package::uninstall(&root, &id)?;
        Ok(ActionRecord {
            message: "Mod 程序已卸载，配置数据保留。".into(),
        })
    })
    .await
}
