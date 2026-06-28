use std::sync::Mutex;
use std::collections::HashMap;
use tauri::{AppHandle, Manager, State, Emitter};
use sysinfo::System;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tauri_plugin_opener::OpenerExt;

// 1. Data Structures
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
#[allow(non_snake_case)]
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
#[allow(non_snake_case)]
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

// 2. Database Models
#[derive(Serialize, Deserialize, Clone)]
struct BloatEntry {
    name: String,
    vendor: Option<String>,
    category: String,
    impact: String,
    #[allow(non_snake_case)]
    ramRange: Option<String>,
    description: String,
    safe_to_kill: bool,
}

#[derive(Serialize, Clone)]
#[allow(non_snake_case)]
struct ProcessInfo {
    exe: String,
    name: String,
    vendor: String,
    memMB: u64,
    ramRange: String,
    description: String,
    impact: String,
    safe_to_kill: bool,
    category: String,
}

// 3. Wrappers
struct AppStateWrapper(Mutex<AppStateStruct>);
struct SettingsWrapper(Mutex<AppSettings>);
struct DatabaseWrapper(Mutex<HashMap<String, BloatEntry>>);

// 4. API Endpoints
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
fn scan_processes(db: State<DatabaseWrapper>) -> Vec<ProcessInfo> {
    let mut sys = System::new_all();
    sys.refresh_all();
    
    let db_lock = db.0.lock().unwrap();
    let mut results = Vec::new();
    let mut seen = std::collections::HashSet::new();

    for (_pid, process) in sys.processes() {
        let exe = process.name().to_string_lossy().to_string();
        
        if seen.contains(&exe) {
            continue;
        }

        if let Some(entry) = db_lock.get(&exe) {
            seen.insert(exe.clone());
            let mem_mb = process.memory() / 1024 / 1024;
            
            results.push(ProcessInfo {
                exe: exe.clone(),
                name: entry.name.clone(),
                vendor: entry.vendor.clone().unwrap_or_default(),
                memMB: mem_mb,
                ramRange: entry.ramRange.clone().unwrap_or_default(),
                description: entry.description.clone(),
                impact: entry.impact.clone(),
                safe_to_kill: entry.safe_to_kill,
                category: entry.category.clone(),
            });
        }
    }
    
    results
}

#[tauri::command]
fn kill_process(data: serde_json::Value, state: State<AppStateWrapper>) -> serde_json::Value {
    let target_exe = data.get("exe").and_then(|v| v.as_str()).unwrap_or("");
    
    let mut sys = System::new_all();
    sys.refresh_all();
    
    let mut freed_bytes: u64 = 0;
    let mut killed = false;
    
    for (_pid, process) in sys.processes() {
        let exe = process.name().to_string_lossy().to_string();
        if exe == target_exe {
            freed_bytes += process.memory();
            process.kill();
            killed = true;
        }
    }
    
    if killed {
        let mut st = state.0.lock().unwrap();
        st.cleanedBytes += freed_bytes;
        st.killedProcesses.push(target_exe.to_string());
        serde_json::json!({ "ok": true, "freedMB": freed_bytes / 1024 / 1024 })
    } else {
        serde_json::json!({ "ok": false })
    }
}

#[tauri::command]
fn kill_all_bloat(db: State<DatabaseWrapper>, settings: State<SettingsWrapper>, state: State<AppStateWrapper>) -> serde_json::Value {
    let mut sys = System::new_all();
    sys.refresh_all();
    
    let db_lock = db.0.lock().unwrap();
    let safe_mode = settings.0.lock().unwrap().safeMode;
    
    let mut freed_bytes: u64 = 0;
    let mut count = 0;
    
    for (_pid, process) in sys.processes() {
        let exe = process.name().to_string_lossy().to_string();
        
        if let Some(entry) = db_lock.get(&exe) {
            // Check Safe Mode conditions
            if safe_mode && (entry.category == "Communication" || entry.category == "Media") {
                continue; // Skip killing Discord, Spotify, Teams, etc. in safe mode
            }
            
            if entry.safe_to_kill {
                freed_bytes += process.memory();
                if process.kill() {
                    count += 1;
                    state.0.lock().unwrap().killedProcesses.push(exe.clone());
                }
            }
        }
    }
    
    if count > 0 {
        state.0.lock().unwrap().cleanedBytes += freed_bytes;
    }
    
    serde_json::json!({ "ok": true, "count": count, "freedMB": freed_bytes / 1024 / 1024 })
}

#[tauri::command]
fn get_startup_items() -> Vec<String> { vec![] }
#[tauri::command]
fn scan_junk() -> serde_json::Value { serde_json::json!({ "items": [], "totalBytes": 0, "totalMB": 0 }) }
#[tauri::command]
fn get_services() -> String { "".into() }
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

fn fetch_database(app: AppHandle) {
    std::thread::spawn(move || {
        let db_url = "https://raw.githubusercontent.com/anandX1/nextune/master/data/bloat-database.json";
        
        let mut parsed_db: Option<HashMap<String, BloatEntry>> = None;
        
        if let Ok(resp) = reqwest::blocking::get(db_url) {
            if let Ok(json) = resp.json::<HashMap<String, BloatEntry>>() {
                println!("Successfully fetched dynamic database from GitHub!");
                parsed_db = Some(json);
            }
        }
        
        if parsed_db.is_none() {
            println!("Falling back to bundled bloat-database.json");
            if let Ok(content) = std::fs::read_to_string("data/bloat-database.json") {
                if let Ok(json) = serde_json::from_str::<HashMap<String, BloatEntry>>(&content) {
                    parsed_db = Some(json);
                }
            }
        }
        
        if let Some(db) = parsed_db {
            let state: State<DatabaseWrapper> = app.state();
            let mut lock = state.0.lock().unwrap();
            *lock = db;
        }
    });
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(AppStateWrapper(Mutex::new(AppStateStruct::default())))
        .manage(SettingsWrapper(Mutex::new(AppSettings::default())))
        .manage(DatabaseWrapper(Mutex::new(HashMap::new())))
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
            fetch_database(app.handle().clone());
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
