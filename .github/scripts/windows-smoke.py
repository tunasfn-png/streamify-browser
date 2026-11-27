"""Exercise the shipped Windows executable with an isolated profile and a local page."""

import ctypes
from ctypes import wintypes
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import sqlite3
import subprocess
import sys
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
    ForEach-Object {{ @{{ pid = $_.IDProcess; ram = $_.WorkingSetPrivate; cpu = $_.PercentProcessorTime }} }})
@{{ gpuDisabled = $gpuDisabled; stats = $stats }} | ConvertTo-Json -Depth 4 -Compress
"""
    result = subprocess.run(
        ["pwsh", "-NoProfile", "-NonInteractive", "-Command", script],
        check=True, capture_output=True, text=True, timeout=25,
    )
    return json.loads(result.stdout)


def measure_idle(root, label):
    time.sleep(8)
    before = collect_processes(root)
    started = time.monotonic()
    time.sleep(12)
    after = collect_processes(root)
    elapsed = time.monotonic() - started
    assert after["gpuDisabled"], "No WebView2 browser process with --disable-gpu was found"
    previous = {row["pid"]: row["cpu"] for row in before["stats"]}
    total_ram = sum(row["ram"] for row in after["stats"])
    total_cpu = sum(
        max(0, row["cpu"] - previous.get(row["pid"], 0)) for row in after["stats"]
    )
    print(f"{label}: {total_ram / 1024**2:.1f} MiB private working set, "
          f"{total_cpu / 1e7 / elapsed / (os.cpu_count() or 1) * 100:.3f}% CPU "
          f"({len(after['stats'])} processes; {elapsed:.1f}s sample)", flush=True)


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
            print("Verified startup, address bar, navigation, tab switching, session restore and clean shutdown.")
        finally:
            if process.poll() is None:
                process.kill()
                process.wait()
            server.shutdown()


if __name__ == "__main__":
    main(str(Path(sys.argv[1]).resolve(strict=True)))
