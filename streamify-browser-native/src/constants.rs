// Streamify Browser v0.2.0 – constants
// Colors, dimensions, control IDs, and string constants.

use windows::Win32::Foundation::COLORREF;

// ── Dark theme colors ──────────────────────────────────────────────
// COLORREF is 0x00BBGGRR (little-endian RGB)

/// Main background (#0d0d0d)
pub const CLR_BG: COLORREF = COLORREF(0x000D0D0D);
/// Surface / slightly lighter (#1a1a1a)
pub const CLR_SURFACE: COLORREF = COLORREF(0x001A1A1A);
/// Tab bar / address bar background (#111111)
pub const CLR_TOOLBAR: COLORREF = COLORREF(0x00111111);
/// Active tab highlight (#1e1e1e)
pub const CLR_TAB_ACTIVE: COLORREF = COLORREF(0x001E1E1E);
/// Tab hover (#252525)
pub const CLR_TAB_HOVER: COLORREF = COLORREF(0x00252525);
/// Emerald accent (#10b981) → BGR: 0x0081B910
pub const CLR_ACCENT: COLORREF = COLORREF(0x0081B910);
/// Primary text (#e5e5e5)
pub const CLR_TEXT: COLORREF = COLORREF(0x00E5E5E5);
/// Secondary / muted text (#888888)
pub const CLR_TEXT_MUTED: COLORREF = COLORREF(0x00888888);
/// Address bar input background (#0d0d0d)
pub const CLR_INPUT_BG: COLORREF = COLORREF(0x000D0D0D);
/// Border / separator (#2a2a2a)
pub const CLR_BORDER: COLORREF = COLORREF(0x002A2A2A);
/// Close button hover red (#e53e3e) → BGR: 0x003E3EE5
pub const CLR_CLOSE_HOVER: COLORREF = COLORREF(0x003E3EE5);
/// Window button hover (#333333)
pub const CLR_BTN_HOVER: COLORREF = COLORREF(0x00333333);

// ── Toolbar geometry (px) ──────────────────────────────────────────

/// Custom titlebar height
pub const TITLEBAR_H: i32 = 32;
/// Tab bar height
pub const TABBAR_H: i32 = 36;
/// Address bar height
pub const ADDRESSBAR_H: i32 = 38;
/// Total toolbar height = TITLEBAR_H + TABBAR_H + ADDRESSBAR_H
pub const TOOLBAR_H: i32 = TITLEBAR_H + TABBAR_H + ADDRESSBAR_H; // 106

/// Tab width (fixed)
pub const TAB_W: i32 = 180;
/// Tab height (inside tab bar)
pub const TAB_H: i32 = 28;
/// Tab top padding within tab bar
pub const TAB_PAD_TOP: i32 = 4;

/// Window control button width (minimize / maximize / close)
pub const WIN_BTN_W: i32 = 46;

/// Navigation button width (back, forward, reload)
pub const NAV_BTN_W: i32 = 32;
/// Navigation button height
pub const NAV_BTN_H: i32 = 28;

/// Address input left offset (after nav buttons)
pub const ADDR_INPUT_LEFT: i32 = NAV_BTN_W * 3 + 8; // 3 buttons + gap = 104
/// Address input right margin (bookmark star + menu button)
pub const ADDR_INPUT_RIGHT: i32 = 80;
/// Address input height
pub const ADDR_INPUT_H: i32 = 26;

// ── Window defaults ────────────────────────────────────────────────

/// Default window width
pub const DEFAULT_W: i32 = 1280;
/// Default window height
pub const DEFAULT_H: i32 = 760;
/// Minimum window width
pub const MIN_W: i32 = 800;
/// Minimum window height
pub const MIN_H: i32 = 500;

// ── Control IDs ────────────────────────────────────────────────────

/// Address bar EDIT control ID
pub const IDC_ADDRESS: u16 = 1001;

// ── WM_APP custom messages ─────────────────────────────────────────

/// WebView2 navigation completed – WPARAM: 0, LPARAM: 0
pub const WM_WEBVIEW_NAV_COMPLETE: u32 = 0x8001; // WM_APP + 1
/// WebView2 title changed – WPARAM: 0, LPARAM: 0
pub const WM_WEBVIEW_TITLE_CHANGED: u32 = 0x8002;
/// WebView2 source changed – WPARAM: 0, LPARAM: 0
pub const WM_WEBVIEW_SOURCE_CHANGED: u32 = 0x8003;

// ── Strings ────────────────────────────────────────────────────────

/// Window class name
pub const WINDOW_CLASS: &str = "StreamifyBrowserNative";
/// Application title
pub const APP_TITLE: &str = "Streamify Browser";
/// Blank new tab label
pub const BLANK_TAB_TITLE: &str = "Nueva pestaña";
/// Internal blank page URL (about:blank served from local)
pub const BLANK_URL: &str = "about:blank";
/// Search engine URL prefix (DuckDuckGo)
pub const SEARCH_URL: &str = "https://duckduckgo.com/?q=";
/// WebView2 additional browser args
pub const WEBVIEW_ARGS: &str = "--disable-gpu";

// ── Database ───────────────────────────────────────────────────────

/// Database file name (stored next to executable)
pub const DB_FILENAME: &str = "streamify_browser.db";
/// Maximum sessions to keep
pub const MAX_SESSIONS: usize = 5;

// ── Font ───────────────────────────────────────────────────────────

/// UI font name
pub const FONT_NAME: &str = "Segoe UI";
/// UI font size for tab titles, address bar
pub const FONT_SIZE: i32 = 13;
/// Small font size for muted text
pub const FONT_SIZE_SMALL: i32 = 11;
/// App icon glyph
pub const APP_ICON_GLYPH: &str = "◈";
