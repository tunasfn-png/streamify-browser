"""Exercise the shipped Windows executable with an isolated profile and a local page."""

import argparse
import ctypes
from ctypes import wintypes
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import sqlite3
from statistics import median
import subprocess
import tempfile
from threading import Thread
import time


user32 = ctypes.WinDLL("user32", use_last_error=True)


def win32(name, result, *arguments):
    function = getattr(user32, name)
    function.restype = result
    function.argtypes = arguments
    return function


find_window = win32("FindWindowW", wintypes.HWND, wintypes.LPCWSTR, wintypes.LPCWSTR)
get_item = win32("GetDlgItem", wintypes.HWND, wintypes.HWND, ctypes.c_int)
is_minimized = win32("IsIconic", wintypes.BOOL, wintypes.HWND)
show_window = win32("ShowWindowAsync", wintypes.BOOL, wintypes.HWND, ctypes.c_int)
foreground_window = win32("GetForegroundWindow", wintypes.HWND)
set_foreground_window = win32("SetForegroundWindow", wintypes.BOOL, wintypes.HWND)
window_pid = win32(
    "GetWindowThreadProcessId", wintypes.DWORD, wintypes.HWND, ctypes.POINTER(wintypes.DWORD)
)
send = win32(
    "SendMessageW", ctypes.c_ssize_t, wintypes.HWND, wintypes.UINT,
    ctypes.c_size_t, ctypes.c_ssize_t,
)
post = win32(
    "PostMessageW", wintypes.BOOL, wintypes.HWND, wintypes.UINT,
    ctypes.c_size_t, ctypes.c_ssize_t,
)


def wait_for(check, description, timeout=45):
    until = time.monotonic() + timeout
    while time.monotonic() < until:
        if result := check():
            return result
        time.sleep(0.2)
    raise TimeoutError(description)


class Page(BaseHTTPRequestHandler):
    def do_GET(self):
        if self.path not in ("/first", "/second"):
            self.send_error(404)
            return
        body = f"<!doctype html><title>Streamify {self.path[1:]}</title><p>Windows smoke test</p>".encode()
        self.send_response(200)
        self.send_header("Content-Type", "text/html; charset=utf-8")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, *_):
        pass


def address(hwnd):
    buffer = ctypes.create_unicode_buffer(8193)
    send(get_item(hwnd, 1001), 0x000D, len(buffer), ctypes.addressof(buffer))  # WM_GETTEXT
    return buffer.value


def navigate(hwnd, url):
    edit = get_item(hwnd, 1001)
    value = ctypes.create_unicode_buffer(url)
    assert send(edit, 0x000C, 0, ctypes.addressof(value))  # WM_SETTEXT
    assert post(edit, 0x0102, 13, 0)  # WM_CHAR / Enter


def history_has(database, url, title):
    if not database.is_file():
        return False
    with sqlite3.connect(database) as conn:
        return conn.execute(
            "SELECT 1 FROM history WHERE url = ? AND title = ?", (url, title)
        ).fetchone()


def start(exe, data):
    process = subprocess.Popen([exe], env={**os.environ, "LOCALAPPDATA": str(data)})

    def owned_window():
        hwnd = find_window("StreamifyBrowserNative", None)
        pid = wintypes.DWORD()
        if hwnd:
            window_pid(hwnd, ctypes.byref(pid))
        return hwnd if pid.value == process.pid and get_item(hwnd, 1001) else None

    try:
        return process, wait_for(owned_window, "The Streamify window did not appear")
    except Exception:
        process.kill()
        process.wait()
        raise


def close(process, hwnd):
    assert post(hwnd, 0x0010, 0, 0)  # WM_CLOSE
    assert process.wait(timeout=15) == 0


def collect_processes(root):
    # Windows exposes the private working set and accumulated CPU time per process here.
    script = rf"""
$ErrorActionPreference = 'Stop'
$ids = [System.Collections.Generic.HashSet[int]]::new()
[void]$ids.Add({root})
$all = @(Get-CimInstance Win32_Process)
do {{
    $added = $false
    foreach ($p in $all) {{
        if ($ids.Contains($p.ParentProcessId) -and $ids.Add($p.ProcessId)) {{ $added = $true }}
    }}
}} while ($added)
$gpuDisabled = [bool]($all | Where-Object {{ $ids.Contains($_.ProcessId) -and
    $_.Name -eq 'msedgewebview2.exe' -and $_.CommandLine -match '(?:^|\s)--disable-gpu(?:\s|$)' }})
$stats = @(Get-CimInstance Win32_PerfRawData_PerfProc_Process | Where-Object {{ $ids.Contains($_.IDProcess) }} |
    ForEach-Object {{ @{{ pid = $_.IDProcess; ram = $_.WorkingSetPrivate; commit = $_.PrivateBytes; cpu = $_.PercentProcessorTime }} }})
@{{ gpuDisabled = $gpuDisabled; stats = $stats }} | ConvertTo-Json -Depth 4 -Compress
"""
    result = subprocess.run(
        ["pwsh", "-NoProfile", "-NonInteractive", "-Command", script],
        check=True, capture_output=True, text=True, timeout=25,
    )
    return json.loads(result.stdout)


def report(message):
    print(message, flush=True)
    if summary := os.environ.get("GITHUB_STEP_SUMMARY"):
        with open(summary, "a", encoding="utf-8") as output:
            output.write(f"- {message}\n")


def measure_idle(root, label, settle=8, sample=12):
    time.sleep(settle)
    before = collect_processes(root)
    started = time.monotonic()
    time.sleep(sample)
    after = collect_processes(root)
    elapsed = time.monotonic() - started
    assert after["gpuDisabled"], "No WebView2 browser process with --disable-gpu was found"
    previous = {row["pid"]: row["cpu"] for row in before["stats"]}
    total_ram = sum(row["ram"] for row in after["stats"])
    total_cpu = sum(
        max(0, row["cpu"] - previous.get(row["pid"], 0)) for row in after["stats"]
    )
    report(f"{label} | {total_ram / 1024**2:.1f} MiB private working set, "
           f"{total_cpu / 1e7 / elapsed / (os.cpu_count() or 1) * 100:.3f}% CPU "
           f"({len(after['stats'])} processes, {elapsed:.1f}s sample)")


def successful_site_navigation(database, prefix):
    with sqlite3.connect(database) as conn:
        return conn.execute(
            "SELECT 1 FROM history WHERE url LIKE ? AND title <> ''", (prefix + "%",)
        ).fetchone()


def measure_spotify(process, hwnd, database, label):
    set_foreground_window(hwnd)
    navigate(hwnd, "https://open.spotify.com/")
    started = time.monotonic()
    snapshots = []
    for offset in (1, 3, 5, 8, 12, 18, 24, 30, 36, 42):
        time.sleep(max(0, started + offset - time.monotonic()))
        snapshot = collect_processes(process.pid)
        if snapshot["gpuDisabled"]:
            snapshots.append((time.monotonic() - started, foreground_window() == hwnd, snapshot["stats"]))
    loaded = successful_site_navigation(database, "https://open.spotify.com/") is not None
    if not loaded or len(snapshots) < 4:
        report(f"{label} | Spotify single-tab measurement unavailable")
        return loaded

    idle = snapshots[-5:]
    ram = [sum(row["ram"] for row in stats) / 1024**2 for _, _, stats in snapshots]
    committed = [sum(row["commit"] for row in stats) / 1024**2 for _, _, stats in idle]
    cpu_start = {row["pid"]: row["cpu"] for row in idle[0][2]}
    cpu = sum(max(0, row["cpu"] - cpu_start.get(row["pid"], 0)) for row in idle[-1][2])
    duration = idle[-1][0] - idle[0][0]
    cpu_percent = cpu / 1e7 / duration / (os.cpu_count() or 1) * 100
    focused = sum(has_focus for _, has_focus, _ in snapshots)
    report(f"{label} | Spotify only, signed out | {max(ram):.1f} MiB sampled peak, "
           f"{median(ram[-5:]):.1f} MiB median idle private working set, "
           f"{median(committed):.1f} MiB median private commit, {cpu_percent:.3f}% idle CPU "
           f"({duration:.1f}s idle sample, foreground in {focused}/{len(snapshots)} snapshots)")
    return loaded


def check_public_sites(exe, data, label="Current executable", spotify_only=False):
    database = data / "Streamify Browser" / "streamify_browser.db"
    process, hwnd = start(exe, data)
    try:
        spotify_loaded = measure_spotify(process, hwnd, database, label)
        if spotify_only:
            close(process, hwnd)
            return
        report(f"Spotify public page | {'loaded' if spotify_loaded else 'unverified in the runner'}")
        assert post(hwnd, 0x0201, 1, 195 | (50 << 16))
        wait_for(lambda: not address(hwnd), "Twitch tab did not open")
        navigate(hwnd, "twitch:monstercat")
        twitch_loaded = True
        try:
            wait_for(lambda: successful_site_navigation(database, "https://www.twitch.tv/popout/monstercat/chat"),
                     "Twitch public chat page did not load", timeout=30)
        except TimeoutError:
            twitch_loaded = False
        report(f"Twitch public chat | {'loaded' if twitch_loaded else 'unverified in the runner'}")
        if spotify_loaded and twitch_loaded:
            measure_idle(process.pid, "Public pages with Twitch visible and Spotify in background, signed out",
                         settle=25, sample=25)
            assert show_window(hwnd, 6)  # SW_MINIMIZE
            wait_for(lambda: is_minimized(hwnd), "The window did not minimize")
            measure_idle(process.pid, "Same public pages with the window minimized, signed out",
                         settle=15, sample=25)
        close(process, hwnd)
    finally:
        if process.poll() is None:
            process.kill()
            process.wait()


def main(exe):
    with tempfile.TemporaryDirectory(ignore_cleanup_errors=True) as directory, ThreadingHTTPServer(("127.0.0.1", 0), Page) as server:
        data = Path(directory)
        database = data / "Streamify Browser" / "streamify_browser.db"
        base = f"http://127.0.0.1:{server.server_port}"
        Thread(target=server.serve_forever, daemon=True).start()
        process, hwnd = start(exe, data)
        try:
            navigate(hwnd, base + "/first")
            wait_for(lambda: history_has(database, base + "/first", "Streamify first"),
                     "The first page did not load through the address bar")
            assert post(hwnd, 0x0201, 1, 195 | (50 << 16))  # WM_LBUTTONDOWN on the new tab button
            wait_for(lambda: not address(hwnd), "The new tab did not open")
            navigate(hwnd, base + "/second")
            wait_for(lambda: history_has(database, base + "/second", "Streamify second"),
                     "The second page did not load")
            measure_idle(process.pid, "Idle with a local page")
            close(process, hwnd)

            process, hwnd = start(exe, data)
            wait_for(lambda: address(hwnd) == base + "/second", "The selected tab was not restored")
            assert post(hwnd, 0x0201, 1, 60 | (50 << 16))  # Click the first tab
            wait_for(lambda: address(hwnd) == base + "/first", "The first tab was not selectable")
            close(process, hwnd)
            report("Verified startup, address bar, navigation, tab switching, session restore and clean shutdown.")
        finally:
            if process.poll() is None:
                process.kill()
                process.wait()
            server.shutdown()
        check_public_sites(exe, data / "sites")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("exe", type=Path)
    parser.add_argument("--spotify-only", action="store_true")
    parser.add_argument("--label", default="Current executable")
    args = parser.parse_args()
    exe = str(args.exe.resolve(strict=True))
    if args.spotify_only:
        with tempfile.TemporaryDirectory(ignore_cleanup_errors=True) as directory:
            check_public_sites(exe, Path(directory), args.label, spotify_only=True)
    else:
        main(exe)
