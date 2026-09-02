use std::fs;
use std::path::{Component, Path, PathBuf};
use tauri::{AppHandle, Manager};

fn err(e: impl ToString) -> String {
    e.to_string()
}

/// Notes live in ~/nyanmd. ponytail: fixed vault dir, add a picker when someone needs two vaults.
fn vault(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app.path().home_dir().map_err(err)?.join("nyanmd");
    fs::create_dir_all(&dir).map_err(err)?;
    Ok(dir)
}

/// Resolve a note name relative to the vault, refusing anything that escapes it.
fn resolve(app: &AppHandle, rel: &str) -> Result<PathBuf, String> {
    let p = Path::new(rel);
    let escapes = p.is_absolute()
        || p.components().any(|c| !matches!(c, Component::Normal(_) | Component::CurDir));
    if rel.trim().is_empty() || escapes {
        return Err(format!("bad note path: {rel}"));
    }
    let mut p = vault(app)?.join(p);
    if p.extension().is_none() {
        p.set_extension("md");
    }
    Ok(p)
}

fn walk(root: &Path, dir: &Path, out: &mut Vec<String>) -> Result<(), String> {
    for entry in fs::read_dir(dir).map_err(err)? {
        let path = entry.map_err(err)?.path();
        if path.is_dir() {
            walk(root, &path, out)?;
        } else if path.extension().is_some_and(|e| e == "md") {
            let rel = path.strip_prefix(root).map_err(err)?;
            out.push(rel.to_string_lossy().into_owned());
        }
    }
    Ok(())
}

#[tauri::command]
fn list_notes(app: AppHandle) -> Result<Vec<String>, String> {
    let root = vault(&app)?;
    let mut out = Vec::new();
    walk(&root, &root, &mut out)?;
    out.sort();
    Ok(out)
}

/// Missing notes read as empty so `:e newname` opens a fresh buffer, like vim.
#[tauri::command]
fn read_note(app: AppHandle, path: String) -> Result<String, String> {
    match fs::read_to_string(resolve(&app, &path)?) {
        Ok(s) => Ok(s),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(e) => Err(err(e)),
    }
}

#[tauri::command]
fn write_note(app: AppHandle, path: String, content: String) -> Result<(), String> {
    let p = resolve(&app, &path)?;
    if let Some(parent) = p.parent() {
        fs::create_dir_all(parent).map_err(err)?;
    }
    fs::write(p, content).map_err(err)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![list_notes, read_note, write_note])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
