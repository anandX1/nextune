use std::sync::Mutex;
use tauri::{AppHandle, Manager, State, Emitter};
use sysinfo::System;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tauri_plugin_opener::OpenerExt;

#[derive(Serialize, Clone)]
struct SystemStats {
    ramTotal: f32,
    ramUsed: f32,
    ramFree: f32,
    ramPct: u8,
    cpuPct: u8,
    gpuPct: u8,
    cpuTemp: u8,
    diskRead: f32,
    diskWrite: f32,
}

#[derive(Serialize, Deserialize, Clone)]
struct AppSettings {
    autoCleanInterval: u32,
    protectedApps: Vec<String>,
    startMinimized: bool,
    trayOnClose: bool,
    theme: String,
    notifications: bool,
    safeMode: bool,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            autoCleanInterval: 30,
            protectedApps: vec![],
            startMinimized: false,
            trayOnClose: true,
            theme: "dark".into(),
            notifications: true,
            safeMode: true,
        }
    }
}

#[derive(Serialize, Deserialize, Clone)]
struct AppStateStruct {
    streamMode: bool,
    autobotInstalled: bool,
    killedProcesses: Vec<String>,
    cleanedBytes: u64,
}

impl Default for AppStateStruct {
    fn default() -> Self {
        Self {
            streamMode: false,
            autobotInstalled: false,
            killedProcesses: vec![],
            cleanedBytes: 0,
        }
    }
}

// State wrappers
struct AppStateWrapper(Mutex<AppStateStruct>);
struct SettingsWrapper(Mutex<AppSettings>);

// Dummy history and version
#[tauri::command]
fn get_history() -> Vec<String> { vec![] }
#[tauri::command]
fn get_version() -> String { "1.0.0-tauri".into() }

#[tauri::command]
fn get_state(state: State<AppStateWrapper>) -> AppStateStruct {
    state.0.lock().unwrap().clone()
}

#[tauri::command]
fn get_settings(settings: State<SettingsWrapper>) -> AppSettings {
    settings.0.lock().unwrap().clone()
}

#[tauri::command]
fn save_settings(s: AppSettings, settings: State<SettingsWrapper>) -> bool {
    let mut st = settings.0.lock().unwrap();
    *st = s;
    true
}

// Window commands
#[tauri::command]
fn window_minimize(app: AppHandle) {
    if let Some(win) = app.get_webview_window("main") {
        win.minimize().unwrap();
    }
}
#[tauri::command]
fn window_maximize(app: AppHandle) {
    if let Some(win) = app.get_webview_window("main") {
        if win.is_maximized().unwrap_or(false) {
            win.unmaximize().unwrap();
        } else {
            win.maximize().unwrap();
        }
    }
}
#[tauri::command]
fn window_close(app: AppHandle) {
    app.exit(0);
}

#[tauri::command]
fn scan_processes() -> Vec<String> { vec![] }
#[tauri::command]
fn get_startup_items() -> Vec<String> { vec![] }
#[tauri::command]
fn scan_junk() -> serde_json::Value { serde_json::json!({ "items": [], "totalBytes": 0, "totalMB": 0 }) }
#[tauri::command]
fn get_services() -> String { "".into() }
#[tauri::command]
fn kill_process(_data: serde_json::Value) -> serde_json::Value { serde_json::json!({ "ok": true }) }
#[tauri::command]
fn kill_all_bloat() -> serde_json::Value { serde_json::json!({ "ok": true, "count": 0 }) }
#[tauri::command]
fn toggle_startup(_data: serde_json::Value) -> serde_json::Value { serde_json::json!({ "ok": true }) }
#[tauri::command]
fn toggle_service(_data: serde_json::Value) -> serde_json::Value { serde_json::json!({ "ok": true }) }
#[tauri::command]
fn clean_junk(_data: serde_json::Value) -> serde_json::Value { serde_json::json!({ "ok": true }) }
#[tauri::command]
fn stream_on() -> serde_json::Value { serde_json::json!({ "ok": true }) }
#[tauri::command]
fn stream_off() -> serde_json::Value { serde_json::json!({ "ok": true }) }
#[tauri::command]
fn install_autobot() -> serde_json::Value { serde_json::json!({ "ok": true }) }
#[tauri::command]
fn uninstall_autobot() -> serde_json::Value { serde_json::json!({ "ok": true }) }
#[tauri::command]
fn create_restore_point(_data: serde_json::Value) -> serde_json::Value { serde_json::json!({ "ok": true }) }
#[tauri::command]
fn undo_action(_data: serde_json::Value) -> serde_json::Value { serde_json::json!({ "ok": true }) }
#[tauri::command]
fn open_external(url: String, app: AppHandle) {
    let _ = app.opener().open_url(&url, None::<String>);
}
#[tauri::command]
fn install_update() {}

fn start_monitoring(app: AppHandle) {
    std::thread::spawn(move || {
        let mut sys = System::new_all();
        loop {
            sys.refresh_all();
            let total_ram = sys.total_memory() as f32 / 1024.0 / 1024.0;
            let used_ram = sys.used_memory() as f32 / 1024.0 / 1024.0;
            let free_ram = total_ram - used_ram;
            let ram_pct = ((used_ram / total_ram) * 100.0) as u8;
            
            let cpu_pct = sys.global_cpu_usage() as u8;

            let stats = SystemStats {
                ramTotal: total_ram,
                ramUsed: used_ram,
                ramFree: free_ram,
                ramPct: ram_pct,
                cpuPct: cpu_pct,
                gpuPct: 0,
                cpuTemp: 45,
                diskRead: 0.0,
                diskWrite: 0.0,
            };
            
            let _ = app.emit("stats-update", stats);
            std::thread::sleep(Duration::from_millis(1500));
        }
    });
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(AppStateWrapper(Mutex::new(AppStateStruct::default())))
        .manage(SettingsWrapper(Mutex::new(AppSettings::default())))
        .invoke_handler(tauri::generate_handler![
            get_state, get_settings, save_settings, get_history, get_version,
            window_minimize, window_maximize, window_close,
            scan_processes, get_startup_items, scan_junk, get_services,
            kill_process, kill_all_bloat, toggle_startup, toggle_service, clean_junk,
            stream_on, stream_off, install_autobot, uninstall_autobot, create_restore_point,
            undo_action, open_external, install_update
        ])
        .setup(|app| {
            start_monitoring(app.handle().clone());
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
