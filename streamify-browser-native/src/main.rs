#![windows_subsystem = "windows"]

mod constants;
mod database;
mod tabs;
mod url_util;
mod webview;
mod window;

use std::path::PathBuf;
use std::process::ExitCode;

use windows::core::{w, Error, Result, HSTRING};
use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED};
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, GetMessageW, MessageBoxW, TranslateMessage, MB_ICONERROR, MB_OK, MSG,
};

use crate::constants::DB_FILENAME;
use crate::database::Database;

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            let message =
                HSTRING::from(format!("No se pudo iniciar Streamify Browser.\n\n{error}"));
            unsafe {
                MessageBoxW(
                    None,
                    &message,
                    w!("Streamify Browser"),
                    MB_OK | MB_ICONERROR,
                );
            }
            ExitCode::FAILURE
        }
    }
}

struct ComApartment;

impl ComApartment {
    fn initialize() -> Result<Self> {
        unsafe {
            CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok()?;
        }
        Ok(Self)
    }
}

impl Drop for ComApartment {
    fn drop(&mut self) {
        unsafe {
            CoUninitialize();
        }
    }
}

fn run() -> std::result::Result<(), Box<dyn std::error::Error>> {
    let _com = ComApartment::initialize()?;
    let data_dir = PathBuf::from(std::env::var_os("LOCALAPPDATA").ok_or("Falta LOCALAPPDATA")?)
        .join("Streamify Browser");
    std::fs::create_dir_all(&data_dir)?;
    // Keep an existing portable database available to users of the original layout.
    let legacy_db = std::env::current_exe()?.with_file_name(DB_FILENAME);
    let db_path = if legacy_db.is_file() {
        legacy_db
    } else {
        data_dir.join(DB_FILENAME)
    };
    let db = Database::open(&db_path)?;
    let hwnd = window::create_main_window(db)?;
    window::init_webview(hwnd, &data_dir.join("WebView2"));

    let mut msg = MSG::default();
    unsafe {
        loop {
            match GetMessageW(&mut msg, None, 0, 0).0 {
                -1 => return Err(Error::from_thread().into()),
                0 => return Ok(()),
                _ => {
                    let _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
            }
        }
    }
}
