// Streamify Browser v0.2.0 – WebView2 controller management
// Single active WebView2 controller at a time. Uses --disable-gpu (software rendering).
// Content WebView2 is positioned below the 106px toolbar.

use std::sync::mpsc;

use webview2_com::{
    CreateCoreWebView2ControllerCompletedHandler,
    CreateCoreWebView2EnvironmentCompletedHandler,
    Microsoft::Web::WebView2::Win32::*,
    NavigationCompletedEventHandler,
    SourceChangedEventHandler,
};

use windows::core::*;
use windows::Win32::Foundation::{E_POINTER, HWND, RECT};
use windows::Win32::UI::WindowsAndMessaging;

use crate::constants::{TOOLBAR_H, WEBVIEW_ARGS, WM_WEBVIEW_NAV_COMPLETE,
    WM_WEBVIEW_SOURCE_CHANGED, WM_WEBVIEW_TITLE_CHANGED};

// ── WebView2 environment (created once, reused) ────────────────────

/// Creates the WebView2 environment with --disable-gpu browser args.
/// This is called once at startup. The environment is used to create controllers.
pub fn create_environment() -> Result<ICoreWebView2Environment> {
    let (tx, rx) = mpsc::channel();

    CreateCoreWebView2EnvironmentCompletedHandler::wait_for_async_operation(
        Box::new(|handler| unsafe {
            // Pass --disable-gpu via CreateCoreWebView2EnvironmentWithOptions
            let options: ICoreWebView2EnvironmentOptions =
                CoreWebView2EnvironmentOptions::default().into();
            let args = HSTRING::from(WEBVIEW_ARGS);
            options.SetAdditionalBrowserArguments(&args)?;

            CreateCoreWebView2EnvironmentWithOptions(None, None, &options, &handler)
                .map_err(webview2_com::Error::WindowsError)
        }),
        Box::new(move |error_code, environment| {
            error_code?;
            tx.send(environment.ok_or_else(|| windows::core::Error::from(E_POINTER)))
                .expect("send environment over channel");
            Ok(())
        }),
    )?;

    rx.recv()
        .map_err(|_| windows::core::Error::new(E_POINTER, HSTRING::new()))?
}

// ── WebView2 controller (one per active tab) ───────────────────────

/// Creates a WebView2 controller parented to the given HWND.
/// The controller's bounds are set to fill the area below the toolbar (106px).
pub fn create_controller(
    env: &ICoreWebView2Environment,
    parent: HWND,
) -> Result<ICoreWebView2Controller> {
    let (tx, rx) = mpsc::channel();
    let env_clone = env.clone();

    CreateCoreWebView2ControllerCompletedHandler::wait_for_async_operation(
        Box::new(move |handler| unsafe {
            env_clone
                .CreateCoreWebView2Controller(parent, &handler)
                .map_err(webview2_com::Error::WindowsError)
        }),
        Box::new(move |error_code, controller| {
            error_code?;
            tx.send(controller.ok_or_else(|| windows::core::Error::from(E_POINTER)))
                .expect("send controller over channel");
            Ok(())
        }),
    )?;

    let controller = rx
        .recv()
        .map_err(|_| windows::core::Error::new(E_POINTER, HSTRING::new()))??;

    // Position below toolbar, fill remaining client area
    let mut rect = RECT::default();
    unsafe {
        let _ = WindowsAndMessaging::GetClientRect(parent, &mut rect);
        controller.SetBounds(RECT {
            left: 0,
            top: TOOLBAR_H,
            right: rect.right,
            bottom: rect.bottom,
        })?;
        controller.SetIsVisible(true)?;
    }

    // Configure settings: no host objects for remote content, dev tools only in debug
    unsafe {
        let webview = controller.CoreWebView2()?;
        let settings = webview.Settings()?;

        // Security: remote content must not have elevated permissions
        settings.SetAreDefaultContextMenusEnabled(true)?;
        settings.SetIsStatusBarEnabled(false)?;
        settings.SetIsZoomControlEnabled(true)?;

        // Disable dev tools in release, enable in debug
        #[cfg(debug_assertions)]
        settings.SetAreDevToolsEnabled(true)?;
        #[cfg(not(debug_assertions))]
        settings.SetAreDevToolsEnabled(false)?;
    }

    Ok(controller)
}

/// Resizes the WebView2 controller to fill the client area below the toolbar.
pub fn resize_controller(controller: &ICoreWebView2Controller, parent: HWND) {
    let mut rect = RECT::default();
    unsafe {
        let _ = WindowsAndMessaging::GetClientRect(parent, &mut rect);
        let _ = controller.SetBounds(RECT {
            left: 0,
            top: TOOLBAR_H,
            right: rect.right,
            bottom: rect.bottom,
        });
    }
}

/// Navigates the WebView2 to the given URL.
pub fn navigate(controller: &ICoreWebView2Controller, url: &str) -> Result<()> {
    unsafe {
        let webview = controller.CoreWebView2()?;
        let url = HSTRING::from(url);
        webview.Navigate(&url)?;
    }
    Ok(())
}

/// Gets the current source URL from the WebView2.
pub fn get_source(controller: &ICoreWebView2Controller) -> Result<String> {
    unsafe {
        let webview = controller.CoreWebView2()?;
        let mut uri = PWSTR::null();
        webview.Source(&mut uri)?;
        Ok(uri.to_string()?)
    }
}

/// Gets the document title from the WebView2.
pub fn get_title(controller: &ICoreWebView2Controller) -> Result<String> {
    unsafe {
        let webview = controller.CoreWebView2()?;
        let mut title = PWSTR::null();
        webview.DocumentTitle(&mut title)?;
        Ok(title.to_string()?)
    }
}

/// Registers event handlers for navigation completed, source changed, and title changed.
/// Posts WM_APP custom messages to the parent window so WndProc can update UI.
pub fn register_events(
    controller: &ICoreWebView2Controller,
    parent: HWND,
) -> Result<(i64, i64, i64)> {
    unsafe {
        let webview = controller.CoreWebView2()?;

        // Navigation completed
        let mut nav_token: i64 = 0;
        let parent_nav = parent;
        webview.add_NavigationCompleted(
            &NavigationCompletedEventHandler::create(Box::new(move |_sender, _args| {
                let _ = WindowsAndMessaging::PostMessageW(
                    parent_nav,
                    WM_WEBVIEW_NAV_COMPLETE,
                    windows::Win32::Foundation::WPARAM(0),
                    windows::Win32::Foundation::LPARAM(0),
                );
                Ok(())
            })),
            &mut nav_token,
        )?;

        // Source changed
        let mut source_token: i64 = 0;
        let parent_src = parent;
        webview.add_SourceChanged(
            &SourceChangedEventHandler::create(Box::new(move |_sender, _args| {
                let _ = WindowsAndMessaging::PostMessageW(
                    parent_src,
                    WM_WEBVIEW_SOURCE_CHANGED,
                    windows::Win32::Foundation::WPARAM(0),
                    windows::Win32::Foundation::LPARAM(0),
                );
                Ok(())
            })),
            &mut source_token,
        )?;

        // Document title changed
        let mut title_token: i64 = 0;
        let parent_title = parent;
        webview.add_DocumentTitleChanged(
            &webview2_com::DocumentTitleChangedEventHandler::create(Box::new(
                move |_sender, _args| {
                    let _ = WindowsAndMessaging::PostMessageW(
                        parent_title,
                        WM_WEBVIEW_TITLE_CHANGED,
                        windows::Win32::Foundation::WPARAM(0),
                        windows::Win32::Foundation::LPARAM(0),
                    );
                    Ok(())
                },
            )),
            &mut title_token,
        )?;

        Ok((nav_token, source_token, title_token))
    }
}

/// Closes the WebView2 controller, releasing resources.
pub fn close_controller(controller: &ICoreWebView2Controller) {
    unsafe {
        let _ = controller.Close();
    }
}

/// Executes a script in the WebView2 (fire-and-forget).
pub fn execute_script(controller: &ICoreWebView2Controller, js: &str) {
    unsafe {
        if let Ok(webview) = controller.CoreWebView2() {
            let js = HSTRING::from(js);
            let handler = webview2_com::ExecuteScriptCompletedHandler::create(Box::new(
                |_error_code, _result| Ok(()),
            ));
            let _ = webview.ExecuteScript(&js, &handler);
        }
    }
}

/// Reloads the current page.
pub fn reload(controller: &ICoreWebView2Controller) {
    unsafe {
        if let Ok(webview) = controller.CoreWebView2() {
            let _ = webview.Reload();
        }
    }
}

/// Navigates back.
pub fn go_back(controller: &ICoreWebView2Controller) {
    unsafe {
        if let Ok(webview) = controller.CoreWebView2() {
            let _ = webview.GoBack();
        }
    }
}

/// Navigates forward.
pub fn go_forward(controller: &ICoreWebView2Controller) {
    unsafe {
        if let Ok(webview) = controller.CoreWebView2() {
            let _ = webview.GoForward();
        }
    }
}

/// Checks if the webview can go back.
pub fn can_go_back(controller: &ICoreWebView2Controller) -> bool {
    unsafe {
        controller
            .CoreWebView2()
            .and_then(|wv| wv.CanGoBack())
            .map(|b| b.as_bool())
            .unwrap_or(false)
    }
}

/// Checks if the webview can go forward.
pub fn can_go_forward(controller: &ICoreWebView2Controller) -> bool {
    unsafe {
        controller
            .CoreWebView2()
            .and_then(|wv| wv.CanGoForward())
            .map(|b| b.as_bool())
            .unwrap_or(false)
    }
}
