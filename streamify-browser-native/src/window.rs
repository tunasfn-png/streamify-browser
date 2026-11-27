use std::{cell::RefCell, mem, path::Path, rc::Rc};

use webview2_com::Microsoft::Web::WebView2::Win32::{
    ICoreWebView2Controller, ICoreWebView2Environment, COREWEBVIEW2_MOVE_FOCUS_REASON_PROGRAMMATIC,
};
use windows::core::*;
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Controls::{EM_LIMITTEXT, EM_SETSEL, WM_MOUSELEAVE};
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass};
use windows::Win32::UI::WindowsAndMessaging::*;

use crate::constants::*;
use crate::database::Database;
use crate::tabs::{SessionData, TabId, TabInfo, TabState};
use crate::{url_util, webview};

struct LiveView {
    tab: TabId,
    generation: usize,
    controller: ICoreWebView2Controller,
}

impl Drop for LiveView {
    fn drop(&mut self) {
        unsafe {
            let _ = self.controller.Close();
        }
    }
}

struct AppData {
    tabs: TabState,
    db: Database,
    env: Option<ICoreWebView2Environment>,
    active: Option<LiveView>,
    spotify: Option<LiveView>,
    pending: Option<(TabId, usize)>,
    generation: usize,
    address: HWND,
    font: HFONT,
    font_bold: HFONT,
    input_brush: HBRUSH,
    hover_tab: Option<TabId>,
    hover_btn: HoverButton,
    tracking_mouse: bool,
    is_maximized: bool,
    can_back: bool,
    can_forward: bool,
    bookmarked: bool,
    error: Option<String>,
}

impl Drop for AppData {
    fn drop(&mut self) {
        unsafe {
            let _ = DeleteObject(self.font.into());
            let _ = DeleteObject(self.font_bold.into());
            let _ = DeleteObject(self.input_brush.into());
        }
    }
}

#[derive(Clone, Copy, Default, PartialEq)]
enum HoverButton {
    #[default]
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
    Tab(TabId),
    TabClose(TabId),
}

fn with_app<T>(hwnd: HWND, f: impl FnOnce(&mut AppData) -> T) -> Option<T> {
    unsafe {
        let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const RefCell<AppData>;
        if ptr.is_null() {
            return None;
        }
        // Keep the state alive if a synchronous Win32 message destroys the window.
        Rc::increment_strong_count(ptr);
        let state = Rc::from_raw(ptr);
        let mut state = state.try_borrow_mut().ok()?;
        Some(f(&mut state))
    }
}

pub fn create_main_window(db: Database) -> Result<HWND> {
    unsafe {
        let instance = HINSTANCE(GetModuleHandleW(None)?.0);
        let class = HSTRING::from(WINDOW_CLASS);
        let title = HSTRING::from(APP_TITLE);
        let wc = WNDCLASSEXW {
            cbSize: mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(wnd_proc),
            hInstance: instance,
            hCursor: LoadCursorW(None, IDC_ARROW)?,
            lpszClassName: PCWSTR(class.as_ptr()),
            ..Default::default()
        };
        if RegisterClassExW(&wc) == 0 {
            return Err(Error::from_thread());
        }
        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            &class,
            &title,
            WS_THICKFRAME | WS_SYSMENU | WS_MAXIMIZEBOX | WS_MINIMIZEBOX | WS_CLIPCHILDREN,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            DEFAULT_W,
            DEFAULT_H,
            None,
            None,
            Some(instance),
            None,
        )?;
        let mut rect = RECT::default();
        GetClientRect(hwnd, &mut rect)?;
        let address = CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            w!("EDIT"),
            PCWSTR::null(),
            WS_CHILD | WS_VISIBLE | WINDOW_STYLE(ES_AUTOHSCROLL as u32),
            ADDR_INPUT_LEFT,
            TITLEBAR_H + TABBAR_H + 6,
            rect.right - ADDR_INPUT_LEFT - ADDR_INPUT_RIGHT,
            ADDR_INPUT_H,
            Some(hwnd),
            Some(HMENU(IDC_ADDRESS as *mut _)),
            Some(instance),
            None,
        )?;
        if !SetWindowSubclass(address, Some(address_proc), 1, hwnd.0 as usize).as_bool() {
            return Err(Error::from_thread());
        }
        let font = make_font(FW_NORMAL);
        let font_bold = make_font(FW_BOLD);
        SendMessageW(address, EM_LIMITTEXT, Some(WPARAM(8192)), None);
        SendMessageW(address, WM_SETFONT, Some(WPARAM(font.0 as usize)), None);
        let mut tabs = TabState::new();
        if let Ok(Some(session)) = db.get_last_session() {
            if let Ok(session) = serde_json::from_str::<SessionData>(&session) {
                tabs.restore_session(&session);
            }
        }
        if tabs.count() == 0 {
            tabs.add_tab(TabInfo::new_blank());
        }
        let data = Rc::new(RefCell::new(AppData {
            tabs,
            db,
            address,
            font,
            font_bold,
            input_brush: CreateSolidBrush(CLR_INPUT_BG),
            env: None,
            active: None,
            spotify: None,
            pending: None,
            generation: 0,
            hover_tab: None,
            hover_btn: HoverButton::None,
            tracking_mouse: false,
            is_maximized: false,
            can_back: false,
            can_forward: false,
            bookmarked: false,
            error: None,
        }));
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, Rc::into_raw(data) as isize);
        let _ = ShowWindow(hwnd, SW_SHOW);
        refresh_address(hwnd);
        Ok(hwnd)
    }
}

unsafe fn make_font(weight: FONT_WEIGHT) -> HFONT {
    CreateFontW(
        -FONT_SIZE,
        0,
        0,
        0,
        weight.0 as i32,
        0,
        0,
        0,
        DEFAULT_CHARSET,
        OUT_DEFAULT_PRECIS,
        CLIP_DEFAULT_PRECIS,
        CLEARTYPE_QUALITY,
        (FF_SWISS.0 | VARIABLE_PITCH.0) as u32,
        &HSTRING::from(FONT_NAME),
    )
}

pub fn init_webview(hwnd: HWND, profile: &Path) {
    let result = webview::create_environment(profile, move |result| match result {
        Ok(env) => {
            with_app(hwnd, |data| data.env = Some(env));
            activate_view(hwnd);
        }
        Err(error) => show_error(
            hwnd,
            "No se pudo iniciar el motor Microsoft Edge WebView2",
            error,
        ),
    });
    if let Err(error) = result {
        show_error(
            hwnd,
            "No se pudo iniciar el motor Microsoft Edge WebView2",
            error,
        );
    }
}

fn show_error(hwnd: HWND, message: &str, error: Error) {
    with_app(hwnd, |data| {
        data.error = Some(format!("{message}.\n{error}"))
    });
    unsafe {
        let _ = InvalidateRect(Some(hwnd), None, false);
    }
}

fn activate_view(hwnd: HWND) {
    refresh_address(hwnd);
    let Some((id, parked, create)) = with_app(hwnd, |data| {
        let id = data.tabs.active_id()?;
        if data.active.is_some() {
            return None;
        }
        data.error = None;
        if data.spotify.as_ref().is_some_and(|live| live.tab == id) {
            data.active = data.spotify.take();
            return Some((
                id,
                data.active.as_ref().map(|live| live.controller.clone()),
                None,
            ));
        }
        let env = data.env.clone()?;
        if data.pending.is_some() {
            return None;
        }
        data.generation += 1;
        data.pending = Some((id, data.generation));
        Some((id, None, Some((env, data.generation))))
    })
    .flatten() else {
        return;
    };

    if let Some(controller) = parked {
        webview::resize_controller(&controller, hwnd);
        unsafe {
            let _ = webview::set_visible(
                &controller,
                !IsIconic(hwnd).as_bool(),
                GetForegroundWindow() == hwnd,
            );
        }
        refresh_history_buttons(hwnd, &controller);
    }
    if let Some((env, generation)) = create {
        if let Err(error) = webview::create_controller(&env, hwnd, move |result| {
            controller_ready(hwnd, id, generation, result);
        }) {
            controller_ready(hwnd, id, generation, Err(error));
        }
    }
}

fn controller_ready(
    hwnd: HWND,
    id: TabId,
    generation: usize,
    result: Result<ICoreWebView2Controller>,
) {
    let live = result.map(|controller| LiveView {
        tab: id,
        generation,
        controller,
    });
    let wanted = with_app(hwnd, |data| {
        if data.pending != Some((id, generation)) {
            return None;
        }
        data.pending = None;
        Some(data.active.is_none() && data.tabs.active_id() == Some(id))
    })
    .flatten();
    match wanted {
        None => return,
        Some(false) => {
            drop(live);
            activate_view(hwnd);
            return;
        }
        Some(true) => {}
    }
    let ready = live.and_then(|live| {
        webview::configure(&live.controller, hwnd, generation)?;
        unsafe {
            webview::set_visible(
                &live.controller,
                !IsIconic(hwnd).as_bool(),
                GetForegroundWindow() == hwnd,
            )?;
        }
        Ok(live)
    });
    if !with_app(hwnd, |data| {
        data.active.is_none() && data.tabs.active_id() == Some(id)
    })
    .unwrap_or(false)
    {
        return;
    }
    match ready {
        Ok(live) => {
            let controller = live.controller.clone();
            if let Some(url) = with_app(hwnd, |data| {
                let url = data.tabs.active_tab()?.url.clone();
                data.active = Some(live);
                Some(url)
            })
            .flatten()
            {
                if let Err(error) = webview::navigate(&controller, &url) {
                    with_app(hwnd, |data| data.active.take());
                    show_error(hwnd, "No se pudo abrir la pestaña", error);
                }
            }
        }
        Err(error) => show_error(hwnd, "No se pudo crear la pestaña de WebView2", error),
    }
}

fn active_controller(hwnd: HWND) -> Option<ICoreWebView2Controller> {
    with_app(hwnd, |data| {
        data.active.as_ref().map(|live| live.controller.clone())
    })
    .flatten()
}

fn release_active(hwnd: HWND, keep_spotify: bool) {
    let active = with_app(hwnd, |data| data.active.take()).flatten();
    let Some(live) = active else {
        return;
    };
    let url = webview::source(&live.controller).ok();
    let title = webview::title(&live.controller).ok();
    let keep = with_app(hwnd, |data| {
        if let Some(tab) = data.tabs.get_tab_mut(live.tab) {
            if let Some(url) =
                url.filter(|url| url_util::is_navigable(url) && !url_util::is_blank(url))
            {
                tab.url = url;
            }
            if let Some(title) = title.filter(|title| !title.is_empty()) {
                tab.title = title;
            }
            return keep_spotify && url_util::is_spotify(&tab.url);
        }
        false
    })
    .unwrap_or(false);
    if keep && webview::set_visible(&live.controller, false, false).is_ok() {
        let old = with_app(hwnd, |data| data.spotify.replace(live)).flatten();
        drop(old);
    }
}

fn select_tab(hwnd: HWND, target: HoverButton) {
    match target {
        HoverButton::NewTab => {
            release_active(hwnd, true);
            with_app(hwnd, |data| data.tabs.add_tab(TabInfo::new_blank()));
        }
        HoverButton::Tab(id) => {
            if !with_app(hwnd, |data| {
                data.tabs.active_id() != Some(id) && data.tabs.get_tab(id).is_some()
            })
            .unwrap_or(false)
            {
                return;
            }
            release_active(hwnd, true);
            with_app(hwnd, |data| data.tabs.set_active(id));
        }
        HoverButton::TabClose(id) => {
            if with_app(hwnd, |data| data.tabs.active_id() == Some(id)).unwrap_or(false) {
                release_active(hwnd, false);
            }
            let closed = with_app(hwnd, |data| {
                data.tabs.remove_tab(id);
                if data.tabs.count() == 0 {
                    data.tabs.add_tab(TabInfo::new_blank());
                }
                if data.spotify.as_ref().is_some_and(|live| live.tab == id) {
                    data.spotify.take()
                } else {
                    None
                }
            })
            .flatten();
            drop(closed);
        }
        _ => return,
    }
    unsafe {
        let _ = SetFocus(Some(hwnd));
    }
    activate_view(hwnd);
}

fn refresh_address(hwnd: HWND) {
    if let Some((edit, url)) = with_app(hwnd, |data| {
        let url = data
            .tabs
            .active_tab()
            .map(|tab| tab.url.as_str())
            .unwrap_or(BLANK_URL);
        data.bookmarked = url_util::is_http(url) && data.db.is_bookmarked(url);
        if data.active.is_none() {
            data.can_back = false;
            data.can_forward = false;
        }
        (
            data.address,
            if url_util::is_blank(url) {
                String::new()
            } else {
                url.to_owned()
            },
        )
    }) {
        // Preserve a URL in the middle of being edited while a page changes its own address.
        unsafe {
            if GetFocus() != edit {
                let _ = SetWindowTextW(edit, &HSTRING::from(url));
            }
        }
        invalidate_toolbar(hwnd);
    }
}

fn refresh_history_buttons(hwnd: HWND, controller: &ICoreWebView2Controller) {
    let (back, forward) = webview::history_buttons(controller);
    with_app(hwnd, |data| {
        data.can_back = back;
        data.can_forward = forward;
    });
    invalidate_toolbar(hwnd);
}

fn webview_event(hwnd: HWND, event: u32, generation: usize, success: bool) {
    let Some((tab_id, controller)) = with_app(hwnd, |data| {
        data.active
            .iter()
            .chain(data.spotify.iter())
            .find(|live| live.generation == generation)
            .map(|live| (live.tab, live.controller.clone()))
    })
    .flatten() else {
        return;
    };
    let url = if event == WM_WEBVIEW_SOURCE_CHANGED || event == WM_WEBVIEW_NAV_COMPLETE {
        webview::source(&controller)
            .ok()
            .filter(|url| url_util::is_navigable(url))
    } else {
        None
    };
    let title = if event == WM_WEBVIEW_TITLE_CHANGED || event == WM_WEBVIEW_NAV_COMPLETE {
        webview::title(&controller).ok()
    } else {
        None
    };
    let active = with_app(hwnd, |data| {
        if let Some(tab) = data.tabs.get_tab_mut(tab_id) {
            if let Some(url) = url {
                tab.url = url;
            }
            if let Some(title) = title {
                tab.title = if url_util::is_blank(&tab.url) {
                    BLANK_TAB_TITLE.to_owned()
                } else {
                    title
                };
            }
            if event == WM_WEBVIEW_NAV_COMPLETE && success && url_util::is_http(&tab.url) {
                let _ = data.db.add_history(&tab.url, &tab.title);
            }
        }
        data.tabs.active_id() == Some(tab_id)
    })
    .unwrap_or(false);
    if active && (event == WM_WEBVIEW_HISTORY_CHANGED || event == WM_WEBVIEW_NAV_COMPLETE) {
        refresh_history_buttons(hwnd, &controller);
    }
    if active && (event == WM_WEBVIEW_SOURCE_CHANGED || event == WM_WEBVIEW_NAV_COMPLETE) {
        refresh_address(hwnd);
    }
    if event == WM_WEBVIEW_TITLE_CHANGED {
        invalidate_toolbar(hwnd);
    }
}

fn navigate(hwnd: HWND, url: &str) {
    if !url_util::is_navigable(url) {
        return;
    }
    with_app(hwnd, |data| {
        if let Some(tab) = data.tabs.active_tab_mut() {
            tab.url = url.to_owned();
        }
    });
    if let Some(controller) = active_controller(hwnd) {
        if let Err(error) = webview::navigate(&controller, url) {
            show_error(hwnd, "No se pudo navegar a la dirección", error);
        }
        unsafe {
            let _ = controller.MoveFocus(COREWEBVIEW2_MOVE_FOCUS_REASON_PROGRAMMATIC);
        }
    } else {
        unsafe {
            let _ = SetFocus(Some(hwnd));
        }
        activate_view(hwnd);
    }
    refresh_address(hwnd);
}

const SHORTCUT_ADDRESS: usize = 1;
const SHORTCUT_NEW_TAB: usize = 2;
const SHORTCUT_CLOSE_TAB: usize = 3;
const SHORTCUT_RELOAD: usize = 4;

pub fn shortcut(key: u32) -> Option<usize> {
    if key == u32::from(VK_F5.0) {
        return Some(SHORTCUT_RELOAD);
    }
    if unsafe { GetKeyState(i32::from(VK_CONTROL.0)) } >= 0 {
        return None;
    }
    match key {
        0x4c => Some(SHORTCUT_ADDRESS),
        0x54 => Some(SHORTCUT_NEW_TAB),
        0x57 => Some(SHORTCUT_CLOSE_TAB),
        0x52 => Some(SHORTCUT_RELOAD),
        _ => None,
    }
}

fn run_shortcut(hwnd: HWND, command: usize) {
    match command {
        SHORTCUT_ADDRESS => focus_address(hwnd, None),
        SHORTCUT_NEW_TAB => {
            select_tab(hwnd, HoverButton::NewTab);
            focus_address(hwnd, None);
        }
        SHORTCUT_CLOSE_TAB => {
            if let Some(id) = with_app(hwnd, |data| data.tabs.active_id()).flatten() {
                select_tab(hwnd, HoverButton::TabClose(id));
            }
        }
        SHORTCUT_RELOAD => click(hwnd, HoverButton::Reload),
        _ => {}
    }
}

fn focus_address(hwnd: HWND, text: Option<&str>) {
    if let Some(address) = with_app(hwnd, |data| data.address) {
        unsafe {
            let _ = SetFocus(Some(address));
            if let Some(text) = text {
                let _ = SetWindowTextW(address, &HSTRING::from(text));
                let start = text.find("<canal>").unwrap_or(text.len());
                SendMessageW(
                    address,
                    EM_SETSEL,
                    Some(WPARAM(start)),
                    Some(LPARAM(text.len() as isize)),
                );
            } else {
                SendMessageW(address, EM_SETSEL, Some(WPARAM(0)), Some(LPARAM(-1)));
            }
        }
    }
}

unsafe extern "system" fn address_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    id: usize,
    parent: usize,
) -> LRESULT {
    let parent = HWND(parent as *mut _);
    if msg == WM_KEYDOWN && wparam.0 != usize::from(VK_RETURN.0) {
        if let Some(command) = shortcut(wparam.0 as u32) {
            run_shortcut(parent, command);
            return LRESULT(0);
        }
    }
    if msg == WM_CHAR && wparam.0 == usize::from(VK_RETURN.0) {
        let mut text = vec![0; GetWindowTextLengthW(hwnd).max(0) as usize + 1];
        let len = GetWindowTextW(hwnd, &mut text).max(0) as usize;
        let url = url_util::normalize_url(&String::from_utf16_lossy(&text[..len]));
        navigate(parent, &url);
        return LRESULT(0);
    }
    if msg == WM_NCDESTROY {
        let _ = RemoveWindowSubclass(hwnd, Some(address_proc), id);
    }
    DefSubclassProc(hwnd, msg, wparam, lparam)
}

unsafe extern "system" fn wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_SIZE => {
            if let Some(address) = with_app(hwnd, |data| {
                data.is_maximized = IsZoomed(hwnd).as_bool();
                data.address
            }) {
                let mut rect = RECT::default();
                let _ = GetClientRect(hwnd, &mut rect);
                let _ = SetWindowPos(
                    address,
                    None,
                    ADDR_INPUT_LEFT,
                    TITLEBAR_H + TABBAR_H + 6,
                    (rect.right - ADDR_INPUT_LEFT - ADDR_INPUT_RIGHT).max(0),
                    ADDR_INPUT_H,
                    SWP_NOZORDER | SWP_NOACTIVATE,
                );
            }
            if let Some(controller) = active_controller(hwnd) {
                let minimized = wparam.0 == SIZE_MINIMIZED as usize;
                let _ =
                    webview::set_visible(&controller, !minimized, GetForegroundWindow() == hwnd);
                if !minimized {
                    webview::resize_controller(&controller, hwnd);
                }
            }
            let _ = InvalidateRect(Some(hwnd), None, false);
        }
        WM_ACTIVATE => {
            if let Some(controller) = active_controller(hwnd) {
                let foreground = wparam.0 as u16 != WA_INACTIVE as u16 && !IsIconic(hwnd).as_bool();
                webview::set_memory_target(&controller, foreground);
            }
            return DefWindowProcW(hwnd, msg, wparam, lparam);
        }
        WM_MOVE => {
            if let Some(controller) = active_controller(hwnd) {
                let _ = controller.NotifyParentWindowPositionChanged();
            }
        }
        WM_GETMINMAXINFO => {
            DefWindowProcW(hwnd, msg, wparam, lparam);
            if let Some(info) = (lparam.0 as *mut MINMAXINFO).as_mut() {
                info.ptMinTrackSize.x = MIN_W;
                info.ptMinTrackSize.y = MIN_H;
            }
        }
        WM_ERASEBKGND => return LRESULT(1),
        WM_PAINT => paint_window(hwnd),
        WM_CTLCOLOREDIT => {
            let hdc = HDC(wparam.0 as *mut _);
            SetTextColor(hdc, CLR_TEXT);
            SetBkColor(hdc, CLR_INPUT_BG);
            if let Some(brush) = with_app(hwnd, |data| data.input_brush) {
                return LRESULT(brush.0 as isize);
            }
        }
        WM_NCHITTEST => {
            let result = DefWindowProcW(hwnd, msg, wparam, lparam);
            if result.0 == HTCLIENT as isize {
                let mut pt = point(lparam);
                let mut rect = RECT::default();
                let _ = ScreenToClient(hwnd, &mut pt);
                let _ = GetClientRect(hwnd, &mut rect);
                if pt.y < TITLEBAR_H && pt.x < rect.right - 3 * WIN_BTN_W {
                    return LRESULT(HTCAPTION as isize);
                }
            }
            return result;
        }
        WM_LBUTTONDOWN => click(hwnd, hit_button(hwnd, point(lparam))),
        WM_MOUSEMOVE => {
            let hit = hit_button(hwnd, point(lparam));
            let changed = with_app(hwnd, |data| {
                if !data.tracking_mouse {
                    data.tracking_mouse = TrackMouseEvent(&mut TRACKMOUSEEVENT {
                        cbSize: mem::size_of::<TRACKMOUSEEVENT>() as u32,
                        dwFlags: TME_LEAVE,
                        hwndTrack: hwnd,
                        dwHoverTime: 0,
                    })
                    .is_ok();
                }
                let tab = if let HoverButton::Tab(id) = hit {
                    Some(id)
                } else {
                    None
                };
                if (data.hover_tab, data.hover_btn) == (tab, hit) {
                    return false;
                }
                data.hover_tab = tab;
                data.hover_btn = hit;
                true
            })
            .unwrap_or(false);
            if changed {
                invalidate_toolbar(hwnd);
            }
        }
        WM_MOUSELEAVE => {
            with_app(hwnd, |data| {
                data.tracking_mouse = false;
                data.hover_tab = None;
                data.hover_btn = HoverButton::None;
            });
            invalidate_toolbar(hwnd);
        }
        WM_KEYDOWN => {
            if let Some(command) = shortcut(wparam.0 as u32) {
                run_shortcut(hwnd, command);
            } else {
                return DefWindowProcW(hwnd, msg, wparam, lparam);
            }
        }
        WM_COMMAND
            if wparam.0 as u16 == IDC_ADDRESS && ((wparam.0 >> 16) as u32) == EN_SETFOCUS =>
        {
            SendMessageW(
                HWND(lparam.0 as *mut _),
                EM_SETSEL,
                Some(WPARAM(0)),
                Some(LPARAM(-1)),
            );
        }
        WM_BROWSER_SHORTCUT => run_shortcut(hwnd, wparam.0),
        WM_WEBVIEW_NAV_COMPLETE
        | WM_WEBVIEW_TITLE_CHANGED
        | WM_WEBVIEW_SOURCE_CHANGED
        | WM_WEBVIEW_HISTORY_CHANGED => {
            webview_event(hwnd, msg, wparam.0, lparam.0 != 0);
        }
        WM_CLOSE => {
            release_active(hwnd, false);
            let parked = with_app(hwnd, |data| {
                if let Ok(json) = serde_json::to_string(&data.tabs.get_session_data()) {
                    let _ = data.db.save_session(&json);
                }
                data.spotify.take()
            });
            drop(parked);
            let _ = DestroyWindow(hwnd);
        }
        WM_DESTROY => PostQuitMessage(0),
        WM_NCDESTROY => {
            let ptr = SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0) as *const RefCell<AppData>;
            if !ptr.is_null() {
                drop(Rc::from_raw(ptr));
            }
            return DefWindowProcW(hwnd, msg, wparam, lparam);
        }
        _ => return DefWindowProcW(hwnd, msg, wparam, lparam),
    }
    LRESULT(0)
}

fn point(lparam: LPARAM) -> POINT {
    POINT {
        x: lparam.0 as i16 as i32,
        y: (lparam.0 >> 16) as i16 as i32,
    }
}

fn tab_width(available: i32, count: usize) -> i32 {
    ((available - 36) / i32::try_from(count.max(1)).unwrap_or(i32::MAX) - 2).clamp(45, TAB_W)
}

fn hit_button(hwnd: HWND, pt: POINT) -> HoverButton {
    let mut rect = RECT::default();
    unsafe {
        let _ = GetClientRect(hwnd, &mut rect);
    }
    let POINT { x, y } = pt;
    if y < TITLEBAR_H {
        let distance = rect.right - x;
        return if distance <= WIN_BTN_W {
            HoverButton::Close
        } else if distance <= 2 * WIN_BTN_W {
            HoverButton::Maximize
        } else if distance <= 3 * WIN_BTN_W {
            HoverButton::Minimize
        } else {
            HoverButton::None
        };
    }
    if (TITLEBAR_H + TAB_PAD_TOP..TITLEBAR_H + TAB_PAD_TOP + TAB_H).contains(&y) {
        return with_app(hwnd, |data| {
            let width = tab_width(rect.right, data.tabs.count());
            let mut left = 4;
            for tab in data.tabs.tabs() {
                if (left..left + width).contains(&x) {
                    return if (left + width - 22..left + width - 4).contains(&x) {
                        HoverButton::TabClose(tab.id)
                    } else {
                        HoverButton::Tab(tab.id)
                    };
                }
                left += width + 2;
            }
            if (left..left + 28).contains(&x) {
                HoverButton::NewTab
            } else {
                HoverButton::None
            }
        })
        .unwrap_or_default();
    }
    if (TITLEBAR_H + TABBAR_H..TOOLBAR_H).contains(&y) {
        let star = rect.right - ADDR_INPUT_RIGHT + 8;
        return if x < NAV_BTN_W {
            HoverButton::Back
        } else if x < NAV_BTN_W * 2 {
            HoverButton::Forward
        } else if x < NAV_BTN_W * 3 {
            HoverButton::Reload
        } else if (star..star + 32).contains(&x) {
            HoverButton::BookmarkStar
        } else if (star + 36..star + 68).contains(&x) {
            HoverButton::MenuBtn
        } else {
            HoverButton::None
        };
    }
    HoverButton::None
}

fn click(hwnd: HWND, button: HoverButton) {
    match button {
        HoverButton::Close => unsafe {
            let _ = PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0));
        },
        HoverButton::Minimize => unsafe {
            let _ = ShowWindow(hwnd, SW_MINIMIZE);
        },
        HoverButton::Maximize => unsafe {
            let command = if IsZoomed(hwnd).as_bool() {
                SW_RESTORE
            } else {
                SW_MAXIMIZE
            };
            let _ = ShowWindow(hwnd, command);
        },
        HoverButton::NewTab | HoverButton::Tab(_) | HoverButton::TabClose(_) => {
            select_tab(hwnd, button)
        }
        HoverButton::Back | HoverButton::Forward | HoverButton::Reload => unsafe {
            if let Some(view) =
                active_controller(hwnd).and_then(|controller| controller.CoreWebView2().ok())
            {
                let _ = match button {
                    HoverButton::Back => view.GoBack(),
                    HoverButton::Forward => view.GoForward(),
                    _ => view.Reload(),
                };
            }
        },
        HoverButton::BookmarkStar => {
            with_app(hwnd, |data| {
                if let Some(tab) = data
                    .tabs
                    .active_tab()
                    .filter(|tab| url_util::is_http(&tab.url))
                {
                    let result = if data.bookmarked {
                        data.db.remove_bookmark(&tab.url)
                    } else {
                        data.db.add_bookmark(&tab.url, &tab.title, "")
                    };
                    if result.is_ok() {
                        data.bookmarked = !data.bookmarked;
                    }
                }
            });
            invalidate_toolbar(hwnd);
        }
        HoverButton::MenuBtn => show_menu(hwnd),
        HoverButton::None => {}
    }
}

fn show_menu(hwnd: HWND) {
    let bookmarks =
        with_app(hwnd, |data| data.db.get_bookmarks(12).unwrap_or_default()).unwrap_or_default();
    let mut point = POINT::default();
    let selected = unsafe {
        if GetCursorPos(&mut point).is_err() {
            return;
        }
        let Ok(menu) = CreatePopupMenu() else {
            return;
        };
        let _ = AppendMenuW(menu, MF_STRING, 1, w!("Abrir Spotify"));
        let _ = AppendMenuW(menu, MF_STRING, 2, w!("Abrir chat de Twitch…"));
        if !bookmarks.is_empty() {
            let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
            for (index, bookmark) in bookmarks.iter().enumerate() {
                let name = if bookmark.title.is_empty() {
                    &bookmark.url
                } else {
                    &bookmark.title
                };
                let text = HSTRING::from(name.replace('&', "&&"));
                let _ = AppendMenuW(menu, MF_STRING, index + 3, &text);
            }
        }
        let selection = TrackPopupMenu(
            menu,
            TPM_RETURNCMD | TPM_NONOTIFY,
            point.x,
            point.y,
            None,
            hwnd,
            None,
        )
        .0;
        let _ = DestroyMenu(menu);
        selection
    };
    match selected {
        0 => {}
        1 => {
            select_tab(hwnd, HoverButton::NewTab);
            navigate(hwnd, SPOTIFY_URL);
        }
        2 => {
            select_tab(hwnd, HoverButton::NewTab);
            focus_address(hwnd, Some("twitch:<canal>"));
        }
        selected => {
            if let Some(bookmark) = bookmarks.get((selected - 3) as usize) {
                navigate(hwnd, &bookmark.url);
            }
        }
    }
}

fn invalidate_toolbar(hwnd: HWND) {
    let mut rect = RECT::default();
    unsafe {
        if GetClientRect(hwnd, &mut rect).is_ok() {
            rect.bottom = rect.bottom.min(TOOLBAR_H);
            let _ = InvalidateRect(Some(hwnd), Some(&rect), false);
        }
    }
}

unsafe fn draw_text(hdc: HDC, text: &str, rect: &mut RECT, flags: DRAW_TEXT_FORMAT) {
    let mut wide: Vec<u16> = text.encode_utf16().collect();
    if !wide.is_empty() {
        DrawTextW(hdc, &mut wide, rect, flags);
    }
}

fn paint_window(hwnd: HWND) {
    unsafe {
        let mut ps = PAINTSTRUCT::default();
        let hdc = BeginPaint(hwnd, &mut ps);
        let mut client = RECT::default();
        let _ = GetClientRect(hwnd, &mut client);
        with_app(hwnd, |data| {
            if ps.rcPaint.bottom > TOOLBAR_H {
                let mut content = client;
                content.top = TOOLBAR_H;
                FillRect(hdc, &content, data.input_brush);
                if let Some(error) = &data.error {
                    content.left += 20;
                    content.top += 20;
                    let previous = SelectObject(hdc, data.font.into());
                    SetBkMode(hdc, TRANSPARENT);
                    SetTextColor(hdc, CLR_TEXT);
                    draw_text(hdc, error, &mut content, DT_WORDBREAK | DT_NOPREFIX);
                    SelectObject(hdc, previous);
                }
            }
            let height = client.bottom.min(TOOLBAR_H);
            if ps.rcPaint.top >= height || height == 0 || client.right == 0 {
                return;
            }
            let mem_dc = CreateCompatibleDC(Some(hdc));
            let bmp = CreateCompatibleBitmap(hdc, client.right, height);
            let buffered = !mem_dc.0.is_null() && !bmp.0.is_null();
            let draw_dc = if buffered { mem_dc } else { hdc };
            let previous = if buffered {
                SelectObject(mem_dc, bmp.into())
            } else {
                HGDIOBJ::default()
            };
            paint_titlebar(draw_dc, &client, data);
            paint_tabbar(draw_dc, &client, data);
            paint_addressbar(draw_dc, &client, data);
            if buffered {
                let _ = BitBlt(hdc, 0, 0, client.right, height, Some(mem_dc), 0, 0, SRCCOPY);
                SelectObject(mem_dc, previous);
            }
            if !bmp.0.is_null() {
                let _ = DeleteObject(bmp.into());
            }
            if !mem_dc.0.is_null() {
                let _ = DeleteDC(mem_dc);
            }
        });
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
        let _ = DeleteObject(brush.into());

        // App icon (◈) + title
        let old_font = SelectObject(hdc, data.font_bold.into());
        SetBkMode(hdc, TRANSPARENT);
        SetTextColor(hdc, CLR_ACCENT);

        let icon_rect = &mut RECT {
            left: 8,
            top: 0,
            right: 28,
            bottom: TITLEBAR_H,
        };
        draw_text(
            hdc,
            APP_ICON_GLYPH,
            icon_rect,
            DT_SINGLELINE | DT_VCENTER | DT_LEFT,
        );

        // App name
        SetTextColor(hdc, CLR_TEXT);
        let title_rect = &mut RECT {
            left: 30,
            top: 0,
            right: client.right - WIN_BTN_W * 3,
            bottom: TITLEBAR_H,
        };
        let _ = SelectObject(hdc, data.font.into());
        draw_text(
            hdc,
            APP_TITLE,
            title_rect,
            DT_SINGLELINE | DT_VCENTER | DT_LEFT,
        );

        let _ = SelectObject(hdc, old_font);

        // Window control buttons: minimize, maximize, close
        let btn_top = 0;
        let btn_h = TITLEBAR_H;
        let close_left = client.right - WIN_BTN_W;
        let max_left = close_left - WIN_BTN_W;
        let min_left = max_left - WIN_BTN_W;

        // Minimize button
        draw_window_button(
            hdc,
            min_left,
            btn_top,
            WIN_BTN_W,
            btn_h,
            data.hover_btn == HoverButton::Minimize,
            false,
            "─",
        );
        // Maximize button
        let max_glyph = if data.is_maximized { "❐" } else { "□" };
        draw_window_button(
            hdc,
            max_left,
            btn_top,
            WIN_BTN_W,
            btn_h,
            data.hover_btn == HoverButton::Maximize,
            false,
            max_glyph,
        );
        // Close button
        draw_window_button(
            hdc,
            close_left,
            btn_top,
            WIN_BTN_W,
            btn_h,
            data.hover_btn == HoverButton::Close,
            true,
            "✕",
        );
    }
}

fn draw_window_button(
    hdc: HDC,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    hovered: bool,
    is_close: bool,
    glyph: &str,
) {
    unsafe {
        if hovered {
            let color = if is_close {
                CLR_CLOSE_HOVER
            } else {
                CLR_BTN_HOVER
            };
            let brush = CreateSolidBrush(color);
            let r = RECT {
                left: x,
                top: y,
                right: x + w,
                bottom: y + h,
            };
            FillRect(hdc, &r, brush);
            let _ = DeleteObject(brush.into());
        }

        SetTextColor(hdc, CLR_TEXT);
        SetBkMode(hdc, TRANSPARENT);
        let r = &mut RECT {
            left: x,
            top: y,
            right: x + w,
            bottom: y + h,
        };
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
        let _ = DeleteObject(brush.into());

        let old_font = SelectObject(hdc, data.font.into());
        SetBkMode(hdc, TRANSPARENT);

        // Draw each tab
        let mut x = 4;
        let tab_width = tab_width(client.right, data.tabs.count());
        for tab in data.tabs.tabs() {
            let tab_rect = RECT {
                left: x,
                top: TITLEBAR_H + TAB_PAD_TOP,
                right: x + tab_width,
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
            let _ = DeleteObject(tab_brush.into());

            // Active tab accent bar at bottom
            if tab.is_active {
                let accent_brush = CreateSolidBrush(CLR_ACCENT);
                let accent_rect = RECT {
                    left: x,
                    top: TITLEBAR_H + TAB_PAD_TOP + TAB_H - 2,
                    right: x + tab_width,
                    bottom: TITLEBAR_H + TAB_PAD_TOP + TAB_H,
                };
                FillRect(hdc, &accent_rect, accent_brush);
                let _ = DeleteObject(accent_brush.into());
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
            draw_text(
                hdc,
                &first_char,
                fc_rect,
                DT_SINGLELINE | DT_VCENTER | DT_LEFT,
            );

            // Tab title (truncated)
            SetTextColor(
                hdc,
                if tab.is_active {
                    CLR_TEXT
                } else {
                    CLR_TEXT_MUTED
                },
            );
            let title_rect = &mut RECT {
                left: x + 26,
                top: tab_rect.top,
                right: x + tab_width - 24, // leave room for close button
                bottom: tab_rect.bottom,
            };
            draw_text(
                hdc,
                &tab.title,
                title_rect,
                DT_SINGLELINE | DT_VCENTER | DT_LEFT | DT_END_ELLIPSIS | DT_NOPREFIX,
            );

            // Tab close button (✕)
            let close_hovered = data.hover_btn == HoverButton::TabClose(tab.id);
            if close_hovered {
                SetTextColor(hdc, CLR_TEXT);
            } else {
                SetTextColor(hdc, CLR_TEXT_MUTED);
            }
            let close_rect = &mut RECT {
                left: x + tab_width - 22,
                top: tab_rect.top,
                right: x + tab_width - 4,
                bottom: tab_rect.bottom,
            };
            draw_text(hdc, "✕", close_rect, DT_SINGLELINE | DT_VCENTER | DT_CENTER);

            x += tab_width + 2;
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
            let _ = DeleteObject(hover_brush.into());
        }
        SetTextColor(hdc, CLR_TEXT_MUTED);
        let plus_rect = &mut RECT {
            left: x,
            top: TITLEBAR_H + TAB_PAD_TOP,
            right: x + 28,
            bottom: TITLEBAR_H + TAB_PAD_TOP + TAB_H,
        };
        draw_text(hdc, "+", plus_rect, DT_SINGLELINE | DT_VCENTER | DT_CENTER);

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
        let _ = DeleteObject(brush.into());

        let old_font = SelectObject(hdc, data.font.into());
        SetBkMode(hdc, TRANSPARENT);

        let y_center = TITLEBAR_H + TABBAR_H;

        // Back button (◀)
        let can_back = data.can_back;
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
            let _ = DeleteObject(hb.into());
        }
        draw_text(hdc, "◀", back_rect, DT_SINGLELINE | DT_VCENTER | DT_CENTER);

        // Forward button (▶)
        let can_fwd = data.can_forward;
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
            let _ = DeleteObject(hb.into());
        }
        draw_text(hdc, "▶", fwd_rect, DT_SINGLELINE | DT_VCENTER | DT_CENTER);

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
            let _ = DeleteObject(hb.into());
        }
        draw_text(
            hdc,
            "↻",
            reload_rect,
            DT_SINGLELINE | DT_VCENTER | DT_CENTER,
        );

        // Bookmark star (★/☆) — right side
        let is_bookmarked = data.bookmarked;
        let star_glyph = if is_bookmarked { "★" } else { "☆" };
        SetTextColor(
            hdc,
            if is_bookmarked {
                CLR_ACCENT
            } else {
                CLR_TEXT_MUTED
            },
        );
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
            let _ = DeleteObject(hb.into());
        }
        draw_text(
            hdc,
            star_glyph,
            star_rect,
            DT_SINGLELINE | DT_VCENTER | DT_CENTER,
        );

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
            let _ = DeleteObject(hb.into());
        }
        draw_text(hdc, "☰", menu_rect, DT_SINGLELINE | DT_VCENTER | DT_CENTER);

        let _ = SelectObject(hdc, old_font);
    }
}
