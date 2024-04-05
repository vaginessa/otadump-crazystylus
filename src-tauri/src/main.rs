// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod core;

use core::{Options, Reporter};
use std::{path::PathBuf, sync::Arc};

use serde::{Deserialize, Serialize};
use tauri::Window;

#[derive(Clone, Deserialize, Serialize)]
#[serde(tag = "kind")]
enum Message {
    Progress { value: f64 },
    Error { message: String },
    Completed,
}

struct MyReporter;

impl Reporter for MyReporter {
    fn report_complete(&self) {}
    fn report_error(&self, error: Box<dyn std::error::Error>) {}
    fn report_progress(&self, progress: f64) {}
}

// Learn more about Tauri commands at https://tauri.app/v1/guides/features/command
#[tauri::command]
fn extract(window: Window, payload_file: PathBuf, output_dir: PathBuf) {
    // const EVENT_NAME: &str = "otadump";
    // window
    //     .emit(EVENT_NAME, Some(Message::Progress { value: 0.5 }))
    //     .unwrap();

    let options = Options {
        payload_file,
        output_dir,
    };
    core::extract(options, Arc::new(MyReporter));
}

fn main() {
    // let options = Options {
    //     payload_file: "/home/ajeet/ws/otadump-payloads/bluejay-ota-sd2a.220601.001.a1-bacd4108.zip"
    //         .into(),
    //     output_dir: "/tmp/foobar".into(),
    // };
    // core::extract(options, Arc::new(MyReporter));

    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![extract])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
