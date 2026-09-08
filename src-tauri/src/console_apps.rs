//! Installed application metadata. Discovery never launches applications.
use crate::work_console::WorkConsole;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstalledApp {
    name: String,
    bundle_id: String,
    path: String,
    icon: Option<String>,
}

#[cfg(target_os = "macos")]
unsafe extern "C" {
    fn repose_console_list_apps(configured: *const std::ffi::c_char) -> *mut std::ffi::c_char;
    fn repose_console_pick_app() -> *mut std::ffi::c_char;
    fn repose_console_free_json(value: *mut std::ffi::c_char);
}

#[cfg(target_os = "macos")]
fn decode<T: serde::de::DeserializeOwned>(value: *mut std::ffi::c_char) -> Result<T, String> {
    if value.is_null() {
        return Err("无法读取应用程序信息，请重试".into());
    }
    let result = unsafe { serde_json::from_slice(std::ffi::CStr::from_ptr(value).to_bytes()) };
    unsafe {
        repose_console_free_json(value);
    }
    result.map_err(|_| "应用程序信息格式无效".into())
}

#[tauri::command]
pub async fn console_list_apps(
    service: tauri::State<'_, Arc<WorkConsole>>,
) -> Result<Vec<InstalledApp>, String> {
    let apps = service.status().config.apps;
    tauri::async_runtime::spawn_blocking(move || {
        #[cfg(target_os = "macos")]
        {
            let json = serde_json::to_string(&apps).map_err(|_| "无法读取配置")?;
            let input = std::ffi::CString::new(json).map_err(|_| "配置无效")?;
            decode(unsafe { repose_console_list_apps(input.as_ptr()) })
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = apps;
            Err("仅支持 macOS".into())
        }
    })
    .await
    .map_err(|_| "无法读取已安装 App".to_string())?
}

#[tauri::command]
pub async fn console_pick_app() -> Result<Option<InstalledApp>, String> {
    tauri::async_runtime::spawn_blocking(|| {
        #[cfg(target_os = "macos")]
        {
            #[derive(Deserialize)]
            struct Selection {
                app: Option<InstalledApp>,
                error: Option<String>,
            }
            let selection: Selection = decode(unsafe { repose_console_pick_app() })?;
            if let Some(error) = selection.error {
                return Err(error);
            }
            Ok(selection.app)
        }
        #[cfg(not(target_os = "macos"))]
        {
            Err("仅支持 macOS".into())
        }
    })
    .await
    .map_err(|_| "无法打开 App 选择窗口".to_string())?
}
