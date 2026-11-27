// Streamify Browser v0.2.0 – Main window, WndProc, GDI painting
// Custom-drawn titlebar (32px), tab bar (36px), address bar (38px).
// Dark theme, no animations, no transparency.

use std::mem;
use std::ptr;

use webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2Controller;
use windows::core::*;
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Controls::WM_MOUSELEAVE;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::WindowsAndMessaging::*;

use crate::constants::*;
use crate::database::Database;
use crate::tabs::{TabId, TabInfo, TabState};
use crate::url_util;
use crate::webview;

// ── GDI text helper ────────────────────────────────────────────────

/// Draws text using GDI DrawTextW. Handles &str → wide string conversion.
unsafe fn draw_text(hdc: HDC, text: &str, rect: &mut RECT, flags: DRAW_TEXT_FORMAT) {
    let mut wide: Vec<u16> = text.encode_utf16().collect();
    if wide.is_empty() {
        return;
    }
    DrawTextW(hdc, &mut wide, rect, flags);
}

// ── Application state (stored in GWLP_USERDATA) ───────────────────

pub struct AppData {
    pub tabs: TabState,
    pub db: Database,
    pub wv_env: Option<webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2Environment>,
    pub wv_controller: Option<ICoreWebView2Controller>,
    pub hwnd: HWND,
    // Address bar child HWND
    pub hwnd_address: HWND,
    // GDI resources
    pub font: HFONT,
    pub font_small: HFONT,
    pub font_bold: HFONT,
    // Tracking
    pub hover_tab: Option<TabId>,
    pub hover_btn: HoverButton,
    pub is_maximized: bool,
    pub tracking_mouse: bool,
    // Sidebar
    pub sidebar_visible: bool,
    pub sidebar_mode: SidebarMode,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum HoverButton {
    None,
    Minimize,
    Maximize,
    Close,
    NewTab,
    Back,
    Forward,
    Reload,
    BookmarkStar,
    MenuBtn,
    TabClose(TabId),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SidebarMode {
    Bookmarks,
    History,
    Performance,
}

// ── Window creation ────────────────────────────────────────────────

/// Creates and shows the main application window.
pub fn create_main_window() -> Result<HWND> {
    let class_name = HSTRING::from(WINDOW_CLASS);
    let title = HSTRING::from(APP_TITLE);

    unsafe {
        let hinstance = GetModuleHandleW(None)
            .ok()
            .map(|h| HINSTANCE(h.0))
            .unwrap_or_default();

        let wc = WNDCLASSEXW {
            cbSize: mem::size_of::<WNDCLASSEXW>() as u32,
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(wnd_proc),
            hInstance: hinstance,
            hCursor: LoadCursorW(None, IDC_ARROW)?,
            hbrBackground: HBRUSH(ptr::null_mut()),
            lpszClassName: PCWSTR(class_name.as_ptr()),
            ..Default::default()
        };

        RegisterClassExW(&wc);

        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            PCWSTR(class_name.as_ptr()),
            PCWSTR(title.as_ptr()),
            WS_OVERLAPPEDWINDOW | WS_CLIPCHILDREN,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            DEFAULT_W,
            DEFAULT_H,
            None,
            None,
            Some(hinstance),
            None,
        )?;

        // Set minimum window size tracking via MINMAXINFO in WndProc

        Ok(hwnd)
    }
}

/// Initializes the app data and stores it in GWLP_USERDATA.
pub fn init_app_data(hwnd: HWND, db: Database) {
    unsafe {
        // Create fonts
        let font = CreateFontW(
            -FONT_SIZE,
            0, 0, 0,
            FW_NORMAL.0 as i32,
            0, 0, 0,
            DEFAULT_CHARSET.0 as u32,
            OUT_DEFAULT_PRECIS.0 as u32,
            CLIP_DEFAULT_PRECIS.0 as u32,
            CLEARTYPE_QUALITY.0 as u32,
            (FF_SWISS.0 | VARIABLE_PITCH.0) as u32,
            &HSTRING::from(FONT_NAME),
        );

        let font_small = CreateFontW(
            -FONT_SIZE_SMALL,
            0, 0, 0,
            FW_NORMAL.0 as i32,
            0, 0, 0,
            DEFAULT_CHARSET.0 as u32,
            OUT_DEFAULT_PRECIS.0 as u32,
            CLIP_DEFAULT_PRECIS.0 as u32,
            CLEARTYPE_QUALITY.0 as u32,
            (FF_SWISS.0 | VARIABLE_PITCH.0) as u32,
            &HSTRING::from(FONT_NAME),
        );

        let font_bold = CreateFontW(
            -FONT_SIZE,
            0, 0, 0,
            FW_BOLD.0 as i32,
            0, 0, 0,
            DEFAULT_CHARSET.0 as u32,
            OUT_DEFAULT_PRECIS.0 as u32,
            CLIP_DEFAULT_PRECIS.0 as u32,
            CLEARTYPE_QUALITY.0 as u32,
            (FF_SWISS.0 | VARIABLE_PITCH.0) as u32,
            &HSTRING::from(FONT_NAME),
        );

        // Create address bar EDIT control
        let addr_class = HSTRING::from("EDIT");
        let hinstance = GetModuleHandleW(None)
            .ok()
            .map(|h| HINSTANCE(h.0))
            .unwrap_or_default();

        let hwnd_address = CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            PCWSTR(addr_class.as_ptr()),
            PCWSTR::null(),
            WINDOW_STYLE(
                WS_CHILD.0 | WS_VISIBLE.0 | ES_AUTOHSCROLL as u32 | ES_LEFT as u32,
            ),
            ADDR_INPUT_LEFT,
            TITLEBAR_H + TABBAR_H + 6, // 6px top padding within address bar
            400, // will be resized in WM_SIZE
            ADDR_INPUT_H,
            Some(hwnd),
            HMENU(IDC_ADDRESS as *mut _),
            Some(hinstance),
            None,
        )
        .unwrap_or_default();

        // Style the EDIT control (dark background, light text)
        // We'll handle WM_CTLCOLOREDIT in WndProc

        let _ = SendMessageW(hwnd_address, WM_SETFONT, WPARAM(font.0 as usize), LPARAM(1));

        let data = Box::new(AppData {
            tabs: TabState::new(),
            db,
            wv_env: None,
            wv_controller: None,
            hwnd,
            hwnd_address,
            font,
            font_small,
            font_bold,
            hover_tab: None,
            hover_btn: HoverButton::None,
            is_maximized: false,
            tracking_mouse: false,
            sidebar_visible: false,
            sidebar_mode: SidebarMode::Bookmarks,
        });

        SetWindowLongPtrW(hwnd, GWLP_USERDATA, Box::into_raw(data) as isize);
    }
}

/// Shows the window and triggers the first paint.
pub fn show_window(hwnd: HWND) {
    unsafe {
        let _ = ShowWindow(hwnd, SW_SHOW);
        let _ = UpdateWindow(hwnd);
        let _ = SetFocus(Some(hwnd));
    }
}

// ── GWLP_USERDATA access ──────────────────────────────────────────

fn get_app_data(hwnd: HWND) -> Option<&'static mut AppData> {
    unsafe {
        let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut AppData;
        if ptr.is_null() {
            None
        } else {
            Some(&mut *ptr)
        }
    }
}

// ── WndProc ────────────────────────────────────────────────────────

pub unsafe extern "system" fn wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_CREATE => {
            LRESULT(0)
        }

        WM_SIZE => {
            if let Some(data) = get_app_data(hwnd) {
                // Resize WebView2 controller
                if let Some(ref controller) = data.wv_controller {
                    webview::resize_controller(controller, hwnd);
                }

                // Resize address bar EDIT control
                let mut rect = RECT::default();
                let _ = GetClientRect(hwnd, &mut rect);
                let addr_w = rect.right - ADDR_INPUT_LEFT - ADDR_INPUT_RIGHT;
                let _ = SetWindowPos(
                    data.hwnd_address,
                    None,
                    ADDR_INPUT_LEFT,
                    TITLEBAR_H + TABBAR_H + 6,
                    addr_w.max(100),
                    ADDR_INPUT_H,
                    SWP_NOZORDER | SWP_NOACTIVATE,
                );

                // Track maximized state
                data.is_maximized = IsZoomed(hwnd).as_bool();
            }
            let _ = InvalidateRect(Some(hwnd), None, false);
            LRESULT(0)
        }

        WM_GETMINMAXINFO => {
            let info = &mut *(lparam.0 as *mut MINMAXINFO);
            info.ptMinTrackSize.x = MIN_W;
            info.ptMinTrackSize.y = MIN_H;
            LRESULT(0)
        }

        WM_ERASEBKGND => {
            // We handle all painting in WM_PAINT
            LRESULT(1)
        }

        WM_PAINT => {
            paint_window(hwnd);
            LRESULT(0)
        }

        WM_CTLCOLOREDIT => {
            // Dark theme for the address bar EDIT control
            let hdc = HDC(wparam.0 as *mut _);
            SetTextColor(hdc, CLR_TEXT);
            SetBkColor(hdc, CLR_INPUT_BG);
            let brush = CreateSolidBrush(CLR_INPUT_BG);
            return LRESULT(brush.0 as isize);
        }

        WM_LBUTTONDOWN => {
            let x = (lparam.0 & 0xFFFF) as i16 as i32;
            let y = ((lparam.0 >> 16) & 0xFFFF) as i16 as i32;
            handle_click(hwnd, x, y);
            LRESULT(0)
        }

        WM_MOUSEMOVE => {
            let x = (lparam.0 & 0xFFFF) as i16 as i32;
            let y = ((lparam.0 >> 16) & 0xFFFF) as i16 as i32;
            handle_mouse_move(hwnd, x, y);
            LRESULT(0)
        }

        WM_MOUSELEAVE => {
            if let Some(data) = get_app_data(hwnd) {
                data.hover_tab = None;
                data.hover_btn = HoverButton::None;
                data.tracking_mouse = false;
                let _ = InvalidateRect(Some(hwnd), None, false);
            }
            LRESULT(0)
        }

        WM_NCHITTEST => {
            let x = (lparam.0 & 0xFFFF) as i16 as i32;
            let y = ((lparam.0 >> 16) & 0xFFFF) as i16 as i32;
            return nchittest(hwnd, x, y);
        }

        WM_KEYDOWN => {
            handle_keydown(hwnd, wparam.0 as u32);
            LRESULT(0)
        }

        WM_COMMAND => {
            let control_id = (wparam.0 & 0xFFFF) as u16;
            let notification = ((wparam.0 >> 16) & 0xFFFF) as u16;
            if control_id == IDC_ADDRESS && notification == EN_SETFOCUS as u16 {
                // Select all text when address bar gets focus
                let _ = SendMessageW(
                    HWND(lparam.0 as *mut _),
                    EM_SETSEL,
                    WPARAM(0),
                    LPARAM(-1),
                );
            }
            // Handle Enter key in address bar (via subclassing or WM_KEYDOWN route)
            LRESULT(0)
        }

        // Custom messages from WebView2 events
        WM_WEBVIEW_NAV_COMPLETE => {
            on_navigation_complete(hwnd);
            LRESULT(0)
        }

        WM_WEBVIEW_TITLE_CHANGED => {
            on_title_changed(hwnd);
            LRESULT(0)
        }

        WM_WEBVIEW_SOURCE_CHANGED => {
            on_source_changed(hwnd);
            LRESULT(0)
        }

        WM_CLOSE => {
            // Save session before closing
            if let Some(data) = get_app_data(hwnd) {
                let session = data.tabs.get_session_data();
                if let Ok(json) = serde_json::to_string(&session) {
                    let _ = data.db.save_session(&json);
                }
                // Close WebView2 controller
                if let Some(ref controller) = data.wv_controller {
                    webview::close_controller(controller);
                }
            }
            let _ = DestroyWindow(hwnd);
            LRESULT(0)
        }

        WM_DESTROY => {
            // Clean up app data
            let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut AppData;
            if !ptr.is_null() {
                let data = Box::from_raw(ptr);
                // Delete GDI objects
                let _ = DeleteObject(data.font);
                let _ = DeleteObject(data.font_small);
                let _ = DeleteObject(data.font_bold);
            }
            PostQuitMessage(0);
            LRESULT(0)
        }

        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

// ── Painting ───────────────────────────────────────────────────────

fn paint_window(hwnd: HWND) {
    unsafe {
        let mut ps = PAINTSTRUCT::default();
        let hdc = BeginPaint(hwnd, &mut ps);
        let mut rect = RECT::default();
        let _ = GetClientRect(hwnd, &mut rect);

        // Double-buffered paint
        let mem_dc = CreateCompatibleDC(Some(hdc));
        let bmp = CreateCompatibleBitmap(hdc, rect.right, rect.bottom);
        let old_bmp = SelectObject(mem_dc, bmp);

        if let Some(data) = get_app_data(hwnd) {
            // Fill entire background
            let bg_brush = CreateSolidBrush(CLR_BG);
            FillRect(mem_dc, &rect, bg_brush);
            let _ = DeleteObject(bg_brush);

            // Draw titlebar
            paint_titlebar(mem_dc, &rect, data);

            // Draw tab bar
            paint_tabbar(mem_dc, &rect, data);

            // Draw address bar background
            paint_addressbar(mem_dc, &rect, data);

            // Draw separator line below toolbar
            let sep_brush = CreateSolidBrush(CLR_BORDER);
            let sep_rect = RECT {
                left: 0,
                top: TOOLBAR_H - 1,
                right: rect.right,
                bottom: TOOLBAR_H,
            };
            FillRect(mem_dc, &sep_rect, sep_brush);
            let _ = DeleteObject(sep_brush);
        }

        // Blit to screen
        let _ = BitBlt(hdc, 0, 0, rect.right, rect.bottom, Some(mem_dc), 0, 0, SRCCOPY);
        let _ = SelectObject(mem_dc, old_bmp);
        let _ = DeleteObject(bmp);
        let _ = DeleteDC(mem_dc);
        let _ = EndPaint(hwnd, &ps);
    }
}

fn paint_titlebar(hdc: HDC, client: &RECT, data: &AppData) {
    unsafe {
        let tb_rect = RECT {
            left: 0,
            top: 0,
            right: client.right,
            bottom: TITLEBAR_H,
        };

        // Titlebar background
        let brush = CreateSolidBrush(CLR_BG);
        FillRect(hdc, &tb_rect, brush);
        let _ = DeleteObject(brush);

        // App icon (◈) + title
        let old_font = SelectObject(hdc, data.font_bold);
        SetBkMode(hdc, TRANSPARENT);
        SetTextColor(hdc, CLR_ACCENT);

        let icon_rect = &mut RECT {
            left: 8,
            top: 0,
            right: 28,
            bottom: TITLEBAR_H,
        };
        draw_text(hdc, APP_ICON_GLYPH, icon_rect, DT_SINGLELINE | DT_VCENTER | DT_LEFT);

        // App name
        SetTextColor(hdc, CLR_TEXT);
        let title_rect = &mut RECT {
            left: 30,
            top: 0,
            right: client.right - WIN_BTN_W * 3,
            bottom: TITLEBAR_H,
        };
        let _ = SelectObject(hdc, data.font);
        draw_text(hdc, APP_TITLE, title_rect, DT_SINGLELINE | DT_VCENTER | DT_LEFT);

        let _ = SelectObject(hdc, old_font);

        // Window control buttons: minimize, maximize, close
        let btn_top = 0;
        let btn_h = TITLEBAR_H;
        let close_left = client.right - WIN_BTN_W;
        let max_left = close_left - WIN_BTN_W;
        let min_left = max_left - WIN_BTN_W;

        // Minimize button
        draw_window_button(hdc, min_left, btn_top, WIN_BTN_W, btn_h,
            data.hover_btn == HoverButton::Minimize, false, "─");
        // Maximize button
        let max_glyph = if data.is_maximized { "❐" } else { "□" };
        draw_window_button(hdc, max_left, btn_top, WIN_BTN_W, btn_h,
            data.hover_btn == HoverButton::Maximize, false, max_glyph);
        // Close button
        draw_window_button(hdc, close_left, btn_top, WIN_BTN_W, btn_h,
            data.hover_btn == HoverButton::Close, true, "✕");
    }
}

fn draw_window_button(
    hdc: HDC,
    x: i32, y: i32, w: i32, h: i32,
    hovered: bool, is_close: bool, glyph: &str,
) {
    unsafe {
        if hovered {
            let color = if is_close { CLR_CLOSE_HOVER } else { CLR_BTN_HOVER };
            let brush = CreateSolidBrush(color);
            let r = RECT { left: x, top: y, right: x + w, bottom: y + h };
            FillRect(hdc, &r, brush);
            let _ = DeleteObject(brush);
        }

        SetTextColor(hdc, CLR_TEXT);
        SetBkMode(hdc, TRANSPARENT);
        let r = &mut RECT { left: x, top: y, right: x + w, bottom: y + h };
        draw_text(hdc, glyph, r, DT_SINGLELINE | DT_CENTER | DT_VCENTER);
    }
}

fn paint_tabbar(hdc: HDC, client: &RECT, data: &AppData) {
    unsafe {
        let bar_rect = RECT {
            left: 0,
            top: TITLEBAR_H,
            right: client.right,
            bottom: TITLEBAR_H + TABBAR_H,
        };

        // Tab bar background
        let brush = CreateSolidBrush(CLR_TOOLBAR);
        FillRect(hdc, &bar_rect, brush);
        let _ = DeleteObject(brush);

        let old_font = SelectObject(hdc, data.font);
        SetBkMode(hdc, TRANSPARENT);

        // Draw each tab
        let mut x = 4;
        for tab in data.tabs.tabs() {
            let tab_rect = RECT {
                left: x,
                top: TITLEBAR_H + TAB_PAD_TOP,
                right: x + TAB_W,
                bottom: TITLEBAR_H + TAB_PAD_TOP + TAB_H,
            };

            // Tab background
            let bg = if tab.is_active {
                CLR_TAB_ACTIVE
            } else if data.hover_tab == Some(tab.id) {
                CLR_TAB_HOVER
            } else {
                CLR_TOOLBAR
            };
            let tab_brush = CreateSolidBrush(bg);
            FillRect(hdc, &tab_rect, tab_brush);
            let _ = DeleteObject(tab_brush);

            // Active tab accent bar at bottom
            if tab.is_active {
                let accent_brush = CreateSolidBrush(CLR_ACCENT);
                let accent_rect = RECT {
                    left: x,
                    top: TITLEBAR_H + TAB_PAD_TOP + TAB_H - 2,
                    right: x + TAB_W,
                    bottom: TITLEBAR_H + TAB_PAD_TOP + TAB_H,
                };
                FillRect(hdc, &accent_rect, accent_brush);
                let _ = DeleteObject(accent_brush);
            }

            // Favicon placeholder (first char of title)
            SetTextColor(hdc, CLR_ACCENT);
            let first_char = tab.title.chars().next().unwrap_or('●').to_string();
            let fc_rect = &mut RECT {
                left: x + 8,
                top: tab_rect.top,
                right: x + 24,
                bottom: tab_rect.bottom,
            };
            draw_text(hdc, &first_char, fc_rect,
                DT_SINGLELINE | DT_VCENTER | DT_LEFT);

            // Tab title (truncated)
            SetTextColor(hdc, if tab.is_active { CLR_TEXT } else { CLR_TEXT_MUTED });
            let title_rect = &mut RECT {
                left: x + 26,
                top: tab_rect.top,
                right: x + TAB_W - 24, // leave room for close button
                bottom: tab_rect.bottom,
            };
            draw_text(hdc, &tab.title, title_rect,
                DT_SINGLELINE | DT_VCENTER | DT_LEFT | DT_END_ELLIPSIS);

            // Tab close button (✕)
            let close_hovered = data.hover_btn == HoverButton::TabClose(tab.id);
            if close_hovered {
                SetTextColor(hdc, CLR_TEXT);
            } else {
                SetTextColor(hdc, CLR_TEXT_MUTED);
            }
            let close_rect = &mut RECT {
                left: x + TAB_W - 22,
                top: tab_rect.top,
                right: x + TAB_W - 4,
                bottom: tab_rect.bottom,
            };
            draw_text(hdc, "✕", close_rect,
                DT_SINGLELINE | DT_VCENTER | DT_CENTER);

            x += TAB_W + 2;
        }

        // New tab button (+)
        let new_tab_hovered = data.hover_btn == HoverButton::NewTab;
        if new_tab_hovered {
            let hover_brush = CreateSolidBrush(CLR_TAB_HOVER);
            let nt_rect = RECT {
                left: x,
                top: TITLEBAR_H + TAB_PAD_TOP,
                right: x + 28,
                bottom: TITLEBAR_H + TAB_PAD_TOP + TAB_H,
            };
            FillRect(hdc, &nt_rect, hover_brush);
            let _ = DeleteObject(hover_brush);
        }
        SetTextColor(hdc, CLR_TEXT_MUTED);
        let plus_rect = &mut RECT {
            left: x,
            top: TITLEBAR_H + TAB_PAD_TOP,
            right: x + 28,
            bottom: TITLEBAR_H + TAB_PAD_TOP + TAB_H,
        };
        draw_text(hdc, "+", plus_rect,
            DT_SINGLELINE | DT_VCENTER | DT_CENTER);

        let _ = SelectObject(hdc, old_font);
    }
}

fn paint_addressbar(hdc: HDC, client: &RECT, data: &AppData) {
    unsafe {
        let bar_rect = RECT {
            left: 0,
            top: TITLEBAR_H + TABBAR_H,
            right: client.right,
            bottom: TOOLBAR_H,
        };

        // Address bar background
        let brush = CreateSolidBrush(CLR_TOOLBAR);
        FillRect(hdc, &bar_rect, brush);
        let _ = DeleteObject(brush);

        let old_font = SelectObject(hdc, data.font);
        SetBkMode(hdc, TRANSPARENT);

        let y_center = TITLEBAR_H + TABBAR_H;

        // Back button (◀)
        let can_back = data.wv_controller.as_ref()
            .map(|c| webview::can_go_back(c))
            .unwrap_or(false);
        SetTextColor(hdc, if can_back { CLR_TEXT } else { CLR_TEXT_MUTED });
        let back_rect = &mut RECT {
            left: 0,
            top: y_center,
            right: NAV_BTN_W,
            bottom: y_center + ADDRESSBAR_H,
        };
        if data.hover_btn == HoverButton::Back {
            let hb = CreateSolidBrush(CLR_BTN_HOVER);
            FillRect(hdc, back_rect, hb);
            let _ = DeleteObject(hb);
        }
        draw_text(hdc, "◀", back_rect,
            DT_SINGLELINE | DT_VCENTER | DT_CENTER);

        // Forward button (▶)
        let can_fwd = data.wv_controller.as_ref()
            .map(|c| webview::can_go_forward(c))
            .unwrap_or(false);
        SetTextColor(hdc, if can_fwd { CLR_TEXT } else { CLR_TEXT_MUTED });
        let fwd_rect = &mut RECT {
            left: NAV_BTN_W,
            top: y_center,
            right: NAV_BTN_W * 2,
            bottom: y_center + ADDRESSBAR_H,
        };
        if data.hover_btn == HoverButton::Forward {
            let hb = CreateSolidBrush(CLR_BTN_HOVER);
            FillRect(hdc, fwd_rect, hb);
            let _ = DeleteObject(hb);
        }
        draw_text(hdc, "▶", fwd_rect,
            DT_SINGLELINE | DT_VCENTER | DT_CENTER);

        // Reload button (↻)
        SetTextColor(hdc, CLR_TEXT);
        let reload_rect = &mut RECT {
            left: NAV_BTN_W * 2,
            top: y_center,
            right: NAV_BTN_W * 3,
            bottom: y_center + ADDRESSBAR_H,
        };
        if data.hover_btn == HoverButton::Reload {
            let hb = CreateSolidBrush(CLR_BTN_HOVER);
            FillRect(hdc, reload_rect, hb);
            let _ = DeleteObject(hb);
        }
        draw_text(hdc, "↻", reload_rect,
            DT_SINGLELINE | DT_VCENTER | DT_CENTER);

        // Bookmark star (★/☆) — right side
        let is_bookmarked = data.tabs.active_tab()
            .map(|t| data.db.is_bookmarked(&t.url))
            .unwrap_or(false);
        let star_glyph = if is_bookmarked { "★" } else { "☆" };
        SetTextColor(hdc, if is_bookmarked { CLR_ACCENT } else { CLR_TEXT_MUTED });
        let star_left = client.right - ADDR_INPUT_RIGHT + 8;
        let star_rect = &mut RECT {
            left: star_left,
            top: y_center,
            right: star_left + 32,
            bottom: y_center + ADDRESSBAR_H,
        };
        if data.hover_btn == HoverButton::BookmarkStar {
            let hb = CreateSolidBrush(CLR_BTN_HOVER);
            FillRect(hdc, star_rect, hb);
            let _ = DeleteObject(hb);
        }
        draw_text(hdc, star_glyph, star_rect,
            DT_SINGLELINE | DT_VCENTER | DT_CENTER);

        // Menu button (☰)
        SetTextColor(hdc, CLR_TEXT_MUTED);
        let menu_left = star_left + 36;
        let menu_rect = &mut RECT {
            left: menu_left,
            top: y_center,
            right: menu_left + 32,
            bottom: y_center + ADDRESSBAR_H,
        };
        if data.hover_btn == HoverButton::MenuBtn {
            let hb = CreateSolidBrush(CLR_BTN_HOVER);
            FillRect(hdc, menu_rect, hb);
            let _ = DeleteObject(hb);
        }
        draw_text(hdc, "☰", menu_rect,
            DT_SINGLELINE | DT_VCENTER | DT_CENTER);

        let _ = SelectObject(hdc, old_font);
    }
}

// ── Hit testing for custom titlebar ────────────────────────────────

fn nchittest(hwnd: HWND, screen_x: i32, screen_y: i32) -> LRESULT {
    unsafe {
        let mut pt = windows::Win32::Foundation::POINT {
            x: screen_x,
            y: screen_y,
        };
        let _ = ScreenToClient(hwnd, &mut pt);
        let x = pt.x;
        let y = pt.y;

        let mut rect = RECT::default();
        let _ = GetClientRect(hwnd, &mut rect);

        // Resize borders (8px)
        let border = 8;
        if y < border {
            if x < border { return LRESULT(HTTOPLEFT as isize); }
            if x > rect.right - border { return LRESULT(HTTOPRIGHT as isize); }
            return LRESULT(HTTOP as isize);
        }
        if y > rect.bottom - border {
            if x < border { return LRESULT(HTBOTTOMLEFT as isize); }
            if x > rect.right - border { return LRESULT(HTBOTTOMRIGHT as isize); }
            return LRESULT(HTBOTTOM as isize);
        }
        if x < border { return LRESULT(HTLEFT as isize); }
        if x > rect.right - border { return LRESULT(HTRIGHT as isize); }

        // Window control buttons area (top right)
        if y < TITLEBAR_H {
            let close_left = rect.right - WIN_BTN_W;
            let max_left = close_left - WIN_BTN_W;
            let min_left = max_left - WIN_BTN_W;

            if x >= min_left && x < close_left + WIN_BTN_W {
                return LRESULT(HTCLIENT as isize);
            }

            // Rest of titlebar is drag area
            return LRESULT(HTCAPTION as isize);
        }

        // Everything below titlebar is client area
        LRESULT(HTCLIENT as isize)
    }
}

// ── Mouse tracking ─────────────────────────────────────────────────

fn handle_mouse_move(hwnd: HWND, x: i32, y: i32) {
    let data = match get_app_data(hwnd) {
        Some(d) => d,
        None => return,
    };

    // Track mouse leave
    if !data.tracking_mouse {
        unsafe {
            let mut tme = TRACKMOUSEEVENT {
                cbSize: mem::size_of::<TRACKMOUSEEVENT>() as u32,
                dwFlags: TME_LEAVE,
                hwndTrack: hwnd,
                dwHoverTime: 0,
            };
            let _ = TrackMouseEvent(&mut tme);
        }
        data.tracking_mouse = true;
    }

    let mut rect = RECT::default();
    unsafe { let _ = GetClientRect(hwnd, &mut rect); }
    let old_btn = data.hover_btn;
    let old_tab = data.hover_tab;

    data.hover_btn = HoverButton::None;
    data.hover_tab = None;

    // Titlebar buttons
    if y < TITLEBAR_H {
        let close_left = rect.right - WIN_BTN_W;
        let max_left = close_left - WIN_BTN_W;
        let min_left = max_left - WIN_BTN_W;

        if x >= close_left {
            data.hover_btn = HoverButton::Close;
        } else if x >= max_left {
            data.hover_btn = HoverButton::Maximize;
        } else if x >= min_left {
            data.hover_btn = HoverButton::Minimize;
        }
    }
    // Tab bar
    else if y >= TITLEBAR_H && y < TITLEBAR_H + TABBAR_H {
        let tab_y = TITLEBAR_H + TAB_PAD_TOP;
        if y >= tab_y && y < tab_y + TAB_H {
            let mut tx = 4;
            for tab in data.tabs.tabs() {
                if x >= tx && x < tx + TAB_W {
                    // Check if hovering close button
                    if x >= tx + TAB_W - 22 && x < tx + TAB_W - 4 {
                        data.hover_btn = HoverButton::TabClose(tab.id);
                    } else {
                        data.hover_tab = Some(tab.id);
                    }
                    break;
                }
                tx += TAB_W + 2;
            }
            // New tab button
            if data.hover_tab.is_none() && data.hover_btn == HoverButton::None {
                if x >= tx && x < tx + 28 {
                    data.hover_btn = HoverButton::NewTab;
                }
            }
        }
    }
    // Address bar
    else if y >= TITLEBAR_H + TABBAR_H && y < TOOLBAR_H {
        if x < NAV_BTN_W {
            data.hover_btn = HoverButton::Back;
        } else if x < NAV_BTN_W * 2 {
            data.hover_btn = HoverButton::Forward;
        } else if x < NAV_BTN_W * 3 {
            data.hover_btn = HoverButton::Reload;
        } else {
            let star_left = rect.right - ADDR_INPUT_RIGHT + 8;
            if x >= star_left && x < star_left + 32 {
                data.hover_btn = HoverButton::BookmarkStar;
            } else if x >= star_left + 36 && x < star_left + 68 {
                data.hover_btn = HoverButton::MenuBtn;
            }
        }
    }

    // Repaint toolbar area only if hover changed
    if old_btn != data.hover_btn || old_tab != data.hover_tab {
        unsafe {
            let toolbar_rect = RECT {
                left: 0,
                top: 0,
                right: rect.right,
                bottom: TOOLBAR_H,
            };
            let _ = InvalidateRect(Some(hwnd), Some(&toolbar_rect), false);
        }
    }
}

// ── Click handling ─────────────────────────────────────────────────

fn handle_click(hwnd: HWND, x: i32, y: i32) {
    let data = match get_app_data(hwnd) {
        Some(d) => d,
        None => return,
    };

    let mut rect = RECT::default();
    unsafe { let _ = GetClientRect(hwnd, &mut rect); }

    // Titlebar buttons
    if y < TITLEBAR_H {
        let close_left = rect.right - WIN_BTN_W;
        let max_left = close_left - WIN_BTN_W;
        let min_left = max_left - WIN_BTN_W;

        if x >= close_left {
            unsafe { let _ = PostMessageW(hwnd, WM_CLOSE, WPARAM(0), LPARAM(0)); }
            return;
        } else if x >= max_left {
            if data.is_maximized {
                unsafe { let _ = ShowWindow(hwnd, SW_RESTORE); }
            } else {
                unsafe { let _ = ShowWindow(hwnd, SW_MAXIMIZE); }
            }
            return;
        } else if x >= min_left {
            unsafe { let _ = ShowWindow(hwnd, SW_MINIMIZE); }
            return;
        }
        return;
    }

    // Tab bar
    if y >= TITLEBAR_H && y < TITLEBAR_H + TABBAR_H {
        let tab_y = TITLEBAR_H + TAB_PAD_TOP;
        if y >= tab_y && y < tab_y + TAB_H {
            let mut tx = 4;
            let tabs_snapshot: Vec<(u32, bool)> = data.tabs.tabs()
                .iter()
                .map(|t| (t.id, t.is_active))
                .collect();

            for (tab_id, _is_active) in &tabs_snapshot {
                if x >= tx && x < tx + TAB_W {
                    // Close button
                    if x >= tx + TAB_W - 22 && x < tx + TAB_W - 4 {
                        close_tab(hwnd, *tab_id);
                    } else {
                        switch_tab(hwnd, *tab_id);
                    }
                    return;
                }
                tx += TAB_W + 2;
            }
            // New tab button
            if x >= tx && x < tx + 28 {
                create_blank_tab(hwnd);
                return;
            }
        }
        return;
    }

    // Address bar buttons
    if y >= TITLEBAR_H + TABBAR_H && y < TOOLBAR_H {
        if x < NAV_BTN_W {
            // Back
            if let Some(ref controller) = data.wv_controller {
                webview::go_back(controller);
            }
            return;
        } else if x < NAV_BTN_W * 2 {
            // Forward
            if let Some(ref controller) = data.wv_controller {
                webview::go_forward(controller);
            }
            return;
        } else if x < NAV_BTN_W * 3 {
            // Reload
            if let Some(ref controller) = data.wv_controller {
                webview::reload(controller);
            }
            return;
        }

        let star_left = rect.right - ADDR_INPUT_RIGHT + 8;
        if x >= star_left && x < star_left + 32 {
            // Toggle bookmark
            toggle_bookmark(hwnd);
            return;
        } else if x >= star_left + 36 && x < star_left + 68 {
            // Menu (toggle sidebar) — future enhancement
            data.sidebar_visible = !data.sidebar_visible;
            unsafe { let _ = InvalidateRect(Some(hwnd), None, false); }
            return;
        }
    }
}

// ── Keyboard handling ──────────────────────────────────────────────

fn handle_keydown(hwnd: HWND, vk: u32) {
    let ctrl = unsafe { GetKeyState(VK_CONTROL.0 as i32) } < 0;

    if ctrl {
        match vk {
            // Ctrl+L: Focus address bar
            0x4C => {
                if let Some(data) = get_app_data(hwnd) {
                    unsafe {
                        let _ = SetFocus(Some(data.hwnd_address));
                        let _ = SendMessageW(
                            data.hwnd_address,
                            EM_SETSEL,
                            WPARAM(0),
                            LPARAM(-1),
                        );
                    }
                }
            }
            // Ctrl+T: New tab
            0x54 => {
                create_blank_tab(hwnd);
            }
            // Ctrl+W: Close current tab
            0x57 => {
                if let Some(data) = get_app_data(hwnd) {
                    if let Some(id) = data.tabs.active_id() {
                        close_tab(hwnd, id);
                    }
                }
            }
            // Ctrl+R: Reload
            0x52 => {
                if let Some(data) = get_app_data(hwnd) {
                    if let Some(ref controller) = data.wv_controller {
                        webview::reload(controller);
                    }
                }
            }
            _ => {}
        }
    }

    // F5: Reload
    if vk == VK_F5.0 as u32 {
        if let Some(data) = get_app_data(hwnd) {
            if let Some(ref controller) = data.wv_controller {
                webview::reload(controller);
            }
        }
    }

    // Enter in address bar: navigate
    if vk == VK_RETURN.0 as u32 {
        if let Some(data) = get_app_data(hwnd) {
            let focused = unsafe { GetFocus() };
            if focused == data.hwnd_address {
                navigate_from_addressbar(hwnd);
            }
        }
    }
}

// ── Tab operations ─────────────────────────────────────────────────

/// Creates a new blank tab and activates it.
pub fn create_blank_tab(hwnd: HWND) {
    let data = match get_app_data(hwnd) {
        Some(d) => d,
        None => return,
    };

    // Close existing WebView2 controller
    if let Some(ref controller) = data.wv_controller {
        // Snapshot current tab URL/title before closing
        if let Some(active_id) = data.tabs.active_id() {
            if let Ok(url) = webview::get_source(controller) {
                data.tabs.set_tab_url(active_id, &url);
            }
            if let Ok(title) = webview::get_title(controller) {
                data.tabs.set_tab_title(active_id, &title);
            }
        }
        webview::close_controller(controller);
        data.wv_controller = None;
    }

    // Create new blank tab
    let tab = TabInfo::new_blank();
    let tab_id = data.tabs.add_tab(tab);

    // Create WebView2 for the new tab
    if let Some(ref env) = data.wv_env {
        match webview::create_controller(env, hwnd) {
            Ok(controller) => {
                let _ = webview::register_events(&controller, hwnd);
                // Navigate to about:blank
                let _ = webview::navigate(&controller, BLANK_URL);
                data.wv_controller = Some(controller);
            }
            Err(e) => {
                eprintln!("Failed to create WebView2 controller: {e:?}");
            }
        }
    }

    // Update address bar
    update_address_bar(hwnd, "");

    unsafe { let _ = InvalidateRect(Some(hwnd), None, false); }
}

/// Switches to a tab by ID.
pub fn switch_tab(hwnd: HWND, tab_id: TabId) {
    let data = match get_app_data(hwnd) {
        Some(d) => d,
        None => return,
    };

    // Don't switch if already active
    if data.tabs.active_id() == Some(tab_id) {
        return;
    }

    // Snapshot current tab before switching
    if let Some(ref controller) = data.wv_controller {
        if let Some(active_id) = data.tabs.active_id() {
            if let Ok(url) = webview::get_source(controller) {
                data.tabs.set_tab_url(active_id, &url);
            }
            if let Ok(title) = webview::get_title(controller) {
                data.tabs.set_tab_title(active_id, &title);
            }
        }
        webview::close_controller(controller);
        data.wv_controller = None;
    }

    // Activate the target tab
    data.tabs.set_active(tab_id);

    // Create WebView2 for the target tab
    let url = data.tabs.get_tab(tab_id)
        .map(|t| t.url.clone())
        .unwrap_or_default();

    if let Some(ref env) = data.wv_env {
        match webview::create_controller(env, hwnd) {
            Ok(controller) => {
                let _ = webview::register_events(&controller, hwnd);
                let _ = webview::navigate(&controller, &url);
                data.wv_controller = Some(controller);
            }
            Err(e) => {
                eprintln!("Failed to create WebView2 controller: {e:?}");
            }
        }
    }

    // Update address bar
    let display = url_util::display_url(&url);
    update_address_bar(hwnd, &display);

    unsafe { let _ = InvalidateRect(Some(hwnd), None, false); }
}

/// Closes a tab by ID.
pub fn close_tab(hwnd: HWND, tab_id: TabId) {
    let data = match get_app_data(hwnd) {
        Some(d) => d,
        None => return,
    };

    let was_active = data.tabs.active_id() == Some(tab_id);

    // If closing the active tab, close its WebView2
    if was_active {
        if let Some(ref controller) = data.wv_controller {
            webview::close_controller(controller);
            data.wv_controller = None;
        }
    }

    // Remove the tab
    data.tabs.remove_tab(tab_id);

    // If no tabs left, create a blank one
    if data.tabs.count() == 0 {
        create_blank_tab(hwnd);
        return;
    }

    // If the closed tab was active, switch to the new active tab
    if was_active {
        if let Some(new_active) = data.tabs.active_id() {
            let url = data.tabs.get_tab(new_active)
                .map(|t| t.url.clone())
                .unwrap_or_default();

            if let Some(ref env) = data.wv_env {
                match webview::create_controller(env, hwnd) {
                    Ok(controller) => {
                        let _ = webview::register_events(&controller, hwnd);
                        let _ = webview::navigate(&controller, &url);
                        data.wv_controller = Some(controller);
                    }
                    Err(e) => {
                        eprintln!("Failed to create WebView2 controller: {e:?}");
                    }
                }
            }

            let display = url_util::display_url(&url);
            update_address_bar(hwnd, &display);
        }
    }

    unsafe { let _ = InvalidateRect(Some(hwnd), None, false); }
}

// ── Navigation ─────────────────────────────────────────────────────

/// Reads the address bar text and navigates to it.
pub fn navigate_from_addressbar(hwnd: HWND) {
    let data = match get_app_data(hwnd) {
        Some(d) => d,
        None => return,
    };

    // Read text from address bar EDIT control
    let mut buf = [0u16; 4096];
    let len = unsafe {
        GetWindowTextW(data.hwnd_address, &mut buf)
    };
    let text = String::from_utf16_lossy(&buf[..len as usize]);
    let url = url_util::normalize_url(&text);

    if url.is_empty() {
        return;
    }

    // Only allow HTTP/HTTPS and about:blank
    if !url_util::is_http(&url) && url != "about:blank" {
        return;
    }

    // Navigate
    if let Some(ref controller) = data.wv_controller {
        let _ = webview::navigate(controller, &url);
        // Update tab state
        if let Some(active_id) = data.tabs.active_id() {
            data.tabs.set_tab_url(active_id, &url);
            data.tabs.set_tab_loading(active_id, true);
        }
    }

    // Return focus to main window
    unsafe { let _ = SetFocus(Some(hwnd)); }
}

/// Navigates the active tab to a specific URL.
pub fn navigate_to(hwnd: HWND, url: &str) {
    let data = match get_app_data(hwnd) {
        Some(d) => d,
        None => return,
    };

    if !url_util::is_http(url) && url != "about:blank" {
        return;
    }

    if let Some(ref controller) = data.wv_controller {
        let _ = webview::navigate(controller, url);
        if let Some(active_id) = data.tabs.active_id() {
            data.tabs.set_tab_url(active_id, url);
            data.tabs.set_tab_loading(active_id, true);
        }
    }

    let display = url_util::display_url(url);
    update_address_bar(hwnd, &display);
}

// ── WebView2 event callbacks ───────────────────────────────────────

fn on_navigation_complete(hwnd: HWND) {
    let data = match get_app_data(hwnd) {
        Some(d) => d,
        None => return,
    };

    if let Some(active_id) = data.tabs.active_id() {
        data.tabs.set_tab_loading(active_id, false);

        // Update URL from WebView2
        if let Some(ref controller) = data.wv_controller {
            if let Ok(url) = webview::get_source(controller) {
                data.tabs.set_tab_url(active_id, &url);
                // Add to history (skip about:blank)
                if url_util::is_http(&url) {
                    let title = data.tabs.get_tab(active_id)
                        .map(|t| t.title.clone())
                        .unwrap_or_default();
                    let _ = data.db.add_history(&url, &title);
                }
            }
        }
    }

    unsafe { let _ = InvalidateRect(Some(hwnd), None, false); }
}

fn on_title_changed(hwnd: HWND) {
    let data = match get_app_data(hwnd) {
        Some(d) => d,
        None => return,
    };

    if let Some(active_id) = data.tabs.active_id() {
        if let Some(ref controller) = data.wv_controller {
            if let Ok(title) = webview::get_title(controller) {
                data.tabs.set_tab_title(active_id, &title);
            }
        }
    }

    unsafe {
        let toolbar_rect = RECT {
            left: 0,
            top: 0,
            right: 4000, // wide enough
            bottom: TOOLBAR_H,
        };
        let _ = InvalidateRect(Some(hwnd), Some(&toolbar_rect), false);
    }
}

fn on_source_changed(hwnd: HWND) {
    let data = match get_app_data(hwnd) {
        Some(d) => d,
        None => return,
    };

    if let Some(ref controller) = data.wv_controller {
        if let Ok(url) = webview::get_source(controller) {
            if let Some(active_id) = data.tabs.active_id() {
                data.tabs.set_tab_url(active_id, &url);
            }
            let display = url_util::display_url(&url);
            update_address_bar(hwnd, &display);
        }
    }
}

// ── Bookmarks ──────────────────────────────────────────────────────

fn toggle_bookmark(hwnd: HWND) {
    let data = match get_app_data(hwnd) {
        Some(d) => d,
        None => return,
    };

    if let Some(tab) = data.tabs.active_tab() {
        let url = tab.url.clone();
        let title = tab.title.clone();
        if url_util::is_http(&url) {
            if data.db.is_bookmarked(&url) {
                let _ = data.db.remove_bookmark(&url);
            } else {
                let _ = data.db.add_bookmark(&url, &title, "");
            }
            unsafe {
                let mut rect = RECT::default();
                let _ = GetClientRect(hwnd, &mut rect);
                let addr_rect = RECT {
                    left: 0,
                    top: TITLEBAR_H + TABBAR_H,
                    right: rect.right,
                    bottom: TOOLBAR_H,
                };
                let _ = InvalidateRect(Some(hwnd), Some(&addr_rect), false);
            }
        }
    }
}

// ── Helpers ────────────────────────────────────────────────────────

fn update_address_bar(hwnd: HWND, text: &str) {
    if let Some(data) = get_app_data(hwnd) {
        unsafe {
            let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
            let _ = SetWindowTextW(data.hwnd_address, PCWSTR(wide.as_ptr()));
        }
    }
}

/// Initializes the WebView2 environment and creates the first tab.
pub fn init_webview(hwnd: HWND) {
    let data = match get_app_data(hwnd) {
        Some(d) => d,
        None => return,
    };

    // Create WebView2 environment
    match webview::create_environment() {
        Ok(env) => {
            data.wv_env = Some(env);

            // Try to restore session
            let restored = if let Ok(Some(json)) = data.db.get_last_session() {
                if let Ok(session) = serde_json::from_str::<crate::tabs::SessionData>(&json) {
                    if !session.tabs.is_empty() {
                        let ids = data.tabs.restore_session(&session);
                        // Activate the first tab's WebView
                        if let Some(active_id) = data.tabs.active_id() {
                            let url = data.tabs.get_tab(active_id)
                                .map(|t| t.url.clone())
                                .unwrap_or_else(|| BLANK_URL.to_string());

                            if let Some(ref env) = data.wv_env {
                                match webview::create_controller(env, hwnd) {
                                    Ok(controller) => {
                                        let _ = webview::register_events(&controller, hwnd);
                                        let _ = webview::navigate(&controller, &url);
                                        data.wv_controller = Some(controller);
                                    }
                                    Err(e) => {
                                        eprintln!("Failed to create WebView2: {e:?}");
                                    }
                                }
                            }

                            let display = url_util::display_url(&url);
                            update_address_bar(hwnd, &display);
                        }
                        true
                    } else {
                        false
                    }
                } else {
                    false
                }
            } else {
                false
            };

            // If no session restored, create a blank tab
            if !restored {
                create_blank_tab(hwnd);
            }
        }
        Err(e) => {
            eprintln!("Failed to create WebView2 environment: {e:?}");
            // Show error — WebView2 runtime may not be installed
            unsafe {
                let msg = HSTRING::from(format!(
                    "No se pudo inicializar WebView2.\n\
                     Asegúrate de que el WebView2 Runtime está instalado.\n\n\
                     Error: {e:?}"
                ));
                let title = HSTRING::from("Error – Streamify Browser");
                MessageBoxW(
                    Some(hwnd),
                    &msg,
                    &title,
                    MB_OK | MB_ICONERROR,
                );
            }
        }
    }

    unsafe { let _ = InvalidateRect(Some(hwnd), None, false); }
}
