use std::os::windows::ffi::OsStrExt;
use std::path::Path;

use webview2_com::{
    take_pwstr, AcceleratorKeyPressedEventHandler, CoreWebView2EnvironmentOptions,
    CreateCoreWebView2ControllerCompletedHandler, CreateCoreWebView2EnvironmentCompletedHandler,
    DocumentTitleChangedEventHandler, HistoryChangedEventHandler,
    LaunchingExternalUriSchemeEventHandler, Microsoft::Web::WebView2::Win32::*,
    NavigationCompletedEventHandler, NavigationStartingEventHandler,
    NewWindowRequestedEventHandler, SourceChangedEventHandler,
};
use windows::core::*;
use windows::Win32::Foundation::{E_POINTER, HWND, LPARAM, RECT, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{GetClientRect, PostMessageW};

use crate::constants::*;
use crate::{url_util, window};

pub fn create_environment(
    profile: &Path,
    completed: impl FnOnce(Result<ICoreWebView2Environment>) + 'static,
) -> Result<()> {
    let profile: Vec<u16> = profile.as_os_str().encode_wide().chain([0]).collect();
    let options: ICoreWebView2EnvironmentOptions = CoreWebView2EnvironmentOptions::default().into();
    let handler =
        CreateCoreWebView2EnvironmentCompletedHandler::create(Box::new(move |status, env| {
            completed(status.and_then(|()| env.ok_or_else(|| E_POINTER.into())));
            Ok(())
        }));
    unsafe {
        options.SetAdditionalBrowserArguments(w!("--disable-gpu"))?;
        CreateCoreWebView2EnvironmentWithOptions(
            PCWSTR::null(),
            PCWSTR(profile.as_ptr()),
            &options,
            &handler,
        )
    }
}

pub fn create_controller(
    env: &ICoreWebView2Environment,
    hwnd: HWND,
    completed: impl FnOnce(Result<ICoreWebView2Controller>) + 'static,
) -> Result<()> {
    let handler = CreateCoreWebView2ControllerCompletedHandler::create(Box::new(
        move |status, controller| {
            completed(status.and_then(|()| controller.ok_or_else(|| E_POINTER.into())));
            Ok(())
        },
    ));
    unsafe { env.CreateCoreWebView2Controller(hwnd, &handler) }
}

pub fn configure(
    controller: &ICoreWebView2Controller,
    hwnd: HWND,
    generation: usize,
) -> Result<()> {
    unsafe {
        let view = controller.CoreWebView2()?;
        let settings = view.Settings()?;
        settings.SetAreHostObjectsAllowed(false)?;
        settings.SetIsWebMessageEnabled(false)?;
        settings.SetIsStatusBarEnabled(false)?;
        settings.SetAreDevToolsEnabled(cfg!(debug_assertions))?;
        if let Ok(controller) = controller.cast::<ICoreWebView2Controller2>() {
            let _ = controller.SetDefaultBackgroundColor(COREWEBVIEW2_COLOR {
                A: 255,
                R: 13,
                G: 13,
                B: 13,
            });
        }

        let mut token = 0;
        if let Ok(view_with_external_scheme) = view.cast::<ICoreWebView2_18>() {
            view_with_external_scheme.add_LaunchingExternalUriScheme(
                &LaunchingExternalUriSchemeEventHandler::create(Box::new(|_, args| {
                    if let Some(args) = args {
                        args.SetCancel(true)?;
                    }
                    Ok(())
                })),
                &mut token,
            )?;
        }

        view.add_NavigationStarting(
            &NavigationStartingEventHandler::create(Box::new(|_, args| {
                if let Some(args) = args {
                    let mut uri = PWSTR::null();
                    args.Uri(&mut uri)?;
                    if !url_util::is_navigable(&take_pwstr(uri)) {
                        args.SetCancel(true)?;
                    }
                }
                Ok(())
            })),
            &mut token,
        )?;
        view.add_NewWindowRequested(
            &NewWindowRequestedEventHandler::create(Box::new(|_, args| {
                if let Some(args) = args {
                    let mut uri = PWSTR::null();
                    let mut user_initiated = BOOL::default();
                    args.Uri(&mut uri)?;
                    let uri = take_pwstr(uri);
                    args.IsUserInitiated(&mut user_initiated)?;
                    if !user_initiated.as_bool() || !url_util::is_navigable(&uri) {
                        args.SetHandled(true)?;
                    }
                }
                Ok(())
            })),
            &mut token,
        )?;
        view.add_NavigationCompleted(
            &NavigationCompletedEventHandler::create(Box::new(move |_, args| {
                let mut success = BOOL::default();
                if let Some(args) = args {
                    args.IsSuccess(&mut success)?;
                }
                let _ = PostMessageW(
                    Some(hwnd),
                    WM_WEBVIEW_NAV_COMPLETE,
                    WPARAM(generation),
                    LPARAM(success.0 as isize),
                );
                Ok(())
            })),
            &mut token,
        )?;
        view.add_SourceChanged(
            &SourceChangedEventHandler::create(Box::new(move |_, _| {
                let _ = PostMessageW(
                    Some(hwnd),
                    WM_WEBVIEW_SOURCE_CHANGED,
                    WPARAM(generation),
                    LPARAM(0),
                );
                Ok(())
            })),
            &mut token,
        )?;
        view.add_DocumentTitleChanged(
            &DocumentTitleChangedEventHandler::create(Box::new(move |_, _| {
                let _ = PostMessageW(
                    Some(hwnd),
                    WM_WEBVIEW_TITLE_CHANGED,
                    WPARAM(generation),
                    LPARAM(0),
                );
                Ok(())
            })),
            &mut token,
        )?;
        view.add_HistoryChanged(
            &HistoryChangedEventHandler::create(Box::new(move |_, _| {
                let _ = PostMessageW(
                    Some(hwnd),
                    WM_WEBVIEW_HISTORY_CHANGED,
                    WPARAM(generation),
                    LPARAM(0),
                );
                Ok(())
            })),
            &mut token,
        )?;
        controller.add_AcceleratorKeyPressed(
            &AcceleratorKeyPressedEventHandler::create(Box::new(move |_, args| {
                if let Some(args) = args {
                    let mut key = 0;
                    let mut kind = COREWEBVIEW2_KEY_EVENT_KIND::default();
                    args.VirtualKey(&mut key)?;
                    args.KeyEventKind(&mut kind)?;
                    if kind == COREWEBVIEW2_KEY_EVENT_KIND_KEY_DOWN {
                        if let Some(command) = window::shortcut(key) {
                            args.SetHandled(true)?;
                            let _ = PostMessageW(
                                Some(hwnd),
                                WM_BROWSER_SHORTCUT,
                                WPARAM(command),
                                LPARAM(0),
                            );
                        }
                    }
                }
                Ok(())
            })),
            &mut token,
        )?;
    }
    resize_controller(controller, hwnd);
    Ok(())
}

pub fn resize_controller(controller: &ICoreWebView2Controller, parent: HWND) {
    let mut rect = RECT::default();
    unsafe {
        if GetClientRect(parent, &mut rect).is_ok() {
            rect.top = TOOLBAR_H.min(rect.bottom);
            let _ = controller.SetBounds(rect);
        }
    }
}

pub fn navigate(controller: &ICoreWebView2Controller, url: &str) -> Result<()> {
    unsafe { controller.CoreWebView2()?.Navigate(&HSTRING::from(url)) }
}

pub fn source(controller: &ICoreWebView2Controller) -> Result<String> {
    unsafe {
        let mut uri = PWSTR::null();
        controller.CoreWebView2()?.Source(&mut uri)?;
        Ok(take_pwstr(uri))
    }
}

pub fn title(controller: &ICoreWebView2Controller) -> Result<String> {
    unsafe {
        let mut title = PWSTR::null();
        controller.CoreWebView2()?.DocumentTitle(&mut title)?;
        Ok(take_pwstr(title))
    }
}

pub fn history_buttons(controller: &ICoreWebView2Controller) -> (bool, bool) {
    let (mut back, mut forward) = (BOOL::default(), BOOL::default());
    unsafe {
        if let Ok(view) = controller.CoreWebView2() {
            let _ = view.CanGoBack(&mut back);
            let _ = view.CanGoForward(&mut forward);
        }
    }
    (back.as_bool(), forward.as_bool())
}

/// Low memory mode leaves scripts and audio active in an inactive controller.
pub fn set_memory_target(controller: &ICoreWebView2Controller, foreground: bool) {
    unsafe {
        if let Ok(view) = controller
            .CoreWebView2()
            .and_then(|view| view.cast::<ICoreWebView2_19>())
        {
            let level = if foreground {
                COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_NORMAL
            } else {
                COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW
            };
            let _ = view.SetMemoryUsageTargetLevel(level);
        }
    }
}

pub fn set_visible(
    controller: &ICoreWebView2Controller,
    visible: bool,
    foreground: bool,
) -> Result<()> {
    unsafe {
        controller.SetIsVisible(visible)?;
    }
    set_memory_target(controller, visible && foreground);
    Ok(())
}
