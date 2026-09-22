use crate::db;
use crate::models::{MarketplaceInstallResult, MarketplacePluginView};
use crate::services::{data_service, marketplace_service};
use tauri::{AppHandle, Emitter};

/// 拉取插件市场清单，并补充每个插件的「已安装」状态。
#[tauri::command]
pub fn get_marketplace(app: AppHandle) -> Result<Vec<MarketplacePluginView>, String> {
    let manifest = marketplace_service::fetch_manifest_auto()?;
    let base = crate::utils::app_base_dir().ok_or_else(|| "程序根目录未确定".to_string())?;

    // 应用被删除（含删分组级联、清空/导入配置）后，对应标记应作废，
    // 市场据此恢复为「未安装」，用户可以再次下载安装。
    let conn = db::get_connection(&app)?;
    let live: std::collections::HashSet<String> = data_service::get_all_apps(&conn)?
        .into_iter()
        .map(|item| item.id)
        .collect();
    marketplace_service::prune_stale_markers(base, &live);

    let markers = marketplace_service::read_all_markers(base);

    let views = manifest
        .plugins
        .into_iter()
        .map(|p| {
            let auto_add = marketplace_service::should_auto_add(&p);
            let kind = marketplace_service::kind_to_str(p.kind).to_string();
            let marker = markers.get(&p.id);
            MarketplacePluginView {
                id: p.id,
                name: p.name,
                description: p.description,
                version: p.version,
                author: p.author,
                kind,
                download_url: p.download_url,
                pip: p.pip,
                icon_url: p.icon_url,
                entry: p.entry,
                launch_kind: p.launch_kind,
                interpreter: p.interpreter,
                auto_add,
                installed: marker.is_some(),
                installed_version: marker.and_then(|m| m.version.clone()),
            }
        })
        .collect();
    Ok(views)
}

/// 下载并安装一个插件（进度通过 `marketplace://progress` 事件推给前端）。
#[tauri::command]
pub fn install_marketplace_plugin(app: AppHandle, plugin_id: String) -> Result<MarketplaceInstallResult, String> {
    let manifest = marketplace_service::fetch_manifest_auto()?;
    let plugin = manifest
        .plugins
        .into_iter()
        .find(|p| p.id == plugin_id)
        .ok_or_else(|| format!("插件不存在: {}", plugin_id))?;

    let base = crate::utils::app_base_dir().ok_or_else(|| "程序根目录未确定".to_string())?;
    marketplace_service::install_plugin(&app, base, &plugin, &mut |progress| {
        let _ = app.emit("marketplace://progress", &progress);
    })
}
