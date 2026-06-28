use std::sync::Mutex;
use std::collections::HashMap;
use tauri::{AppHandle, Manager, State, Emitter};
use sysinfo::System;
use serde::{Deserialize, Serialize};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri_plugin_opener::OpenerExt;
use windows_sys::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};
use rusqlite::Connection;

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
struct JournalWrapper(Mutex<Connection>);

// 3.5 Native Helpers
fn get_foreground_pid() -> u32 {
    unsafe {
        let hwnd = GetForegroundWindow();
        let mut pid = 0;
        if hwnd != std::ptr::null_mut() {
            GetWindowThreadProcessId(hwnd, &mut pid);
        }
        pid
    }
}

fn init_journal() -> Connection {
    let app_data = std::env::var("APPDATA").unwrap_or_else(|_| ".".into());
    let dir = std::path::PathBuf::from(app_data).join("NexTune");
    let _ = std::fs::create_dir_all(&dir);
    let db_path = dir.join("usage.db");
    
    let conn = Connection::open(&db_path).unwrap();
    conn.execute(
        "CREATE TABLE IF NOT EXISTS ram_history (
            timestamp INTEGER PRIMARY KEY,
            ram_used_mb INTEGER
        )",
        [],
    ).unwrap();
    conn
}

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
    
    const CORE_WINDOWS: &[&str] = &[
        "svchost.exe", "explorer.exe", "csrss.exe", "smss.exe", "wininit.exe", 
        "services.exe", "lsass.exe", "winlogon.exe", "fontdrvhost.exe", "dwm.exe",
        "spoolsv.exe", "sihost.exe", "taskhostw.exe", "RuntimeBroker.exe", "SearchIndexer.exe",
        "SecurityHealthService.exe", "MsMpEng.exe", "NisSrv.exe", "conhost.exe", "WmiPrvSE.exe",
        "System", "Registry", "Memory Compression", "TextInputHost.exe", "ctfmon.exe"
    ];

    for (pid, process) in sys.processes() {
        let exe = process.name().to_string_lossy().to_string();
        
        if seen.contains(&exe) || CORE_WINDOWS.contains(&exe.as_str()) {
            continue;
        }

        let mem_mb = process.memory() / 1024 / 1024;

        if let Some(entry) = db_lock.get(&exe) {
            seen.insert(exe.clone());
            
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
        } else if mem_mb > 150 {
            // PHASE 3: Unknown Heavy Process Detection
            seen.insert(exe.clone());
            results.push(ProcessInfo {
                exe: exe.clone(),
                name: exe.clone(),
                vendor: "Unknown".to_string(),
                memMB: mem_mb,
                ramRange: "".to_string(),
                description: "Heavy background process not in our database. You can report it to the community.".to_string(),
                impact: "Unknown".to_string(),
                safe_to_kill: false, // Too risky to kill unknown apps
                category: "Unknown".to_string(),
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
    
    let fg_pid = get_foreground_pid();
    
    for (pid, process) in sys.processes() {
        let exe = process.name().to_string_lossy().to_string();
        let pid_u32 = pid.as_u32();
        
        if let Some(entry) = db_lock.get(&exe) {
            // Check Safe Mode conditions
            if safe_mode && (entry.category == "Communication" || entry.category == "Media") {
                continue; // Skip killing Discord, Spotify, Teams, etc. in safe mode
            }
            
            // SMART KILL ENGINE: Never kill the foreground active window!
            if pid_u32 == fg_pid {
                println!("Smart Kill Engine: Skipped {} because it is the active foreground window.", exe);
                continue;
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

#[derive(Serialize)]
struct RamDataPoint {
    timestamp: u64,
    ram_used_mb: u64,
}

#[tauri::command]
fn get_ram_history(journal: State<JournalWrapper>) -> Vec<RamDataPoint> {
    let conn = journal.0.lock().unwrap();
    let mut stmt = conn.prepare("SELECT timestamp, ram_used_mb FROM ram_history ORDER BY timestamp DESC LIMIT 60").unwrap();
    let iter = stmt.query_map([], |row| {
        Ok(RamDataPoint {
            timestamp: row.get::<_, i64>(0)? as u64,
            ram_used_mb: row.get::<_, i64>(1)? as u64,
        })
    }).unwrap();
    
    let mut data = Vec::new();
    for row in iter {
        if let Ok(p) = row {
            data.push(p);
        }
    }
    data.reverse(); // oldest first
    data
}

fn start_monitoring(app: AppHandle) {
    std::thread::spawn(move || {
        let mut sys = System::new_all();
        let mut ticks = 0;
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
            
            // Log memory usage to journal every ~60 seconds
            ticks += 1;
            if ticks >= 40 {
                ticks = 0;
                let state: State<JournalWrapper> = app.state();
                let conn = state.0.lock().unwrap();
                let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
                let _ = conn.execute("INSERT INTO ram_history (timestamp, ram_used_mb) VALUES (?1, ?2)", rusqlite::params![now as i64, used_ram as i64]);
            }
            
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
        .manage(JournalWrapper(Mutex::new(init_journal())))
        .invoke_handler(tauri::generate_handler![
            get_state, get_settings, save_settings, get_history, get_version,
            window_minimize, window_maximize, window_close,
            scan_processes, get_startup_items, scan_junk, get_services,
            kill_process, kill_all_bloat, toggle_startup, toggle_service, clean_junk,
            stream_on, stream_off, install_autobot, uninstall_autobot, create_restore_point,
            undo_action, open_external, install_update, get_ram_history
        ])
        .setup(|app| {
            start_monitoring(app.handle().clone());
            fetch_database(app.handle().clone());
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
