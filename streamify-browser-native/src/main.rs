// Streamify Browser v0.2.0 – Native Win32 + WebView2
// Entry point: COM init (STA), create main window, message loop.
// No Tauri, no Svelte, no Node.js — pure Rust + Win32.

#![windows_subsystem = "windows"]

mod constants;
mod database;
mod performance;
mod tabs;
mod url_util;
mod webview;
mod window;

use std::path::PathBuf;
use windows::core::*;
use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};
use windows::Win32::UI::HiDpi;
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, GetMessageW, TranslateMessage, MSG,
};

use crate::constants::DB_FILENAME;
use crate::database::Database;

fn main() -> Result<()> {
    // Initialize COM as STA (required for WebView2)
    unsafe {
        CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok()?;
    }

    // Set DPI awareness
    unsafe {
        let _ = HiDpi::SetProcessDpiAwareness(HiDpi::PROCESS_PER_MONITOR_DPI_AWARE);
    }

    // Open database (next to executable)
    let db_path = get_db_path();
    let db = Database::open(&db_path).unwrap_or_else(|e| {
        eprintln!("Failed to open database at {}: {e}", db_path.display());
        std::process::exit(1);
    });

    // Create the main window
    let hwnd = window::create_main_window()?;

    // Initialize app data (tab state, fonts, address bar control)
    window::init_app_data(hwnd, db);

    // Show the window
    window::show_window(hwnd);

    // Initialize WebView2 environment and create first tab
    // This is done after showing the window so the user sees the UI immediately.
    window::init_webview(hwnd);

    // Win32 message loop (event-driven, no polling)
    let mut msg = MSG::default();
    unsafe {
        loop {
            let result = GetMessageW(&mut msg, None, 0, 0).0;
            match result {
                -1 => break Err(windows::core::Error::from_thread()),
                0 => break Ok(()),
                _ => {
                    let _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
            }
        }
    }
}

/// Returns the path to the database file, next to the executable.
fn get_db_path() -> PathBuf {
    std::env::current_exe()
        .unwrap_or_else(|_| PathBuf::from("."))
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."))
        .join(DB_FILENAME)
}
