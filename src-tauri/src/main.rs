// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::PathBuf;
use std::thread;

use anyhow::{Context as _, Result};
use otadump::core::ExtractOptions;
use otadump::gui;
use tauri::AppHandle;

#[tauri::command]
fn extract(app: AppHandle, payload_file: PathBuf, output_dir: PathBuf) {
    thread::spawn(move || {
        let options = ExtractOptions { payload_file, output_dir };
        gui::extract(app, options);
    });
}

fn main() -> Result<()> {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![extract])
        .run(tauri::generate_context!())
        .context("error running application")
}
