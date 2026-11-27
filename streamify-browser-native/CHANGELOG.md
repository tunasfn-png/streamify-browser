# Changelog – Streamify Browser

## v0.2.0 – Nativo Win32 + WebView2 (2026-11-26)

### Migración completa de Tauri/Svelte a Win32 nativo

**Eliminado:**
- Tauri 2 (framework completo)
- Svelte 5 / TypeScript / Vite (frontend web)
- Node.js (dependencia de runtime)
- sysinfo crate (reemplazado por Windows API directa)
- tokio (no necesario sin async Tauri commands)
- chrono, uuid (no necesarios)

**Nuevo:**
- UI nativa Win32 con GDI (custom titlebar, tab bar, address bar)
- WebView2 vía `webview2-com` 0.39 (bindings COM directos)
- `windows` crate 0.62 para toda la API Win32
- `--disable-gpu` en WebView2 (renderizado por software)
- Controlador WebView2 único: solo la pestaña activa tiene WebView vivo
- Pestañas inactivas son solo metadatos (URL + título)
- Loop de mensajes event-driven (`GetMessage`/`DispatchMessage`), sin polling
- Medición de rendimiento con Windows API directa (sin sysinfo)
- Compilación a binario único (~3-5 MB estimado en release)

**Conservado de v0.1.3:**
- Esquema SQLite: history, bookmarks, sessions, settings
- Pragmas SQLite: WAL, NORMAL sync, 2000 cache, MEMORY temp
- Lógica de normalización de URLs (HTTP/HTTPS only)
- Restauración de sesión al iniciar
- Guardado de sesión al cerrar
- Tema oscuro (#0d0d0d fondo, #10b981 emerald accent)
- Geometría toolbar: 32px titlebar + 36px tabbar + 38px addressbar = 106px
- Atajos: Ctrl+L (address bar), Ctrl+T (nueva pestaña), Ctrl+W (cerrar), F5/Ctrl+R (reload)

**Seguridad (sin cambios):**
- Sin `--no-sandbox`, `--single-process`, ni flags inseguros
- Sin host objects para contenido remoto
- Solo navegación HTTP/HTTPS
- Sin telemetría propia
- No modifica Windows globalmente ni procesos externos
- No borra datos del usuario (historial, cookies, marcadores)

## v0.1.3 – Tauri + Svelte (versión anterior)

- Navegador basado en Tauri 2 + Svelte 5
- WebView2 con `--disable-gpu`
- Pestaña activa única con descarga de WebViews inactivos
- SQLite para persistencia
- Nunca compilado ni medido en Windows (solo revisión estática)
