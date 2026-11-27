# Streamify Browser v0.2.0

Navegador nativo para Windows 11 construido con Rust + Win32 API + WebView2.

## Arquitectura

```
┌──────────────────────────────────────┐
│  Rust binary (~3-5 MB)               │
│  ┌────────────────────────────────┐  │
│  │ Win32 Window (GDI painting)    │  │
│  │  ┌─ Titlebar (32px)           │  │
│  │  ├─ Tab bar (36px)            │  │
│  │  ├─ Address bar (38px)        │  │
│  │  └─ WebView2 content area     │  │
│  └────────────────────────────────┘  │
│  ┌────────────────────────────────┐  │
│  │ SQLite (WAL mode)              │  │
│  │  history, bookmarks, sessions  │  │
│  └────────────────────────────────┘  │
└──────────────────────────────────────┘
```

## Requisitos

- **Windows 11** (x64)
- **WebView2 Runtime** (preinstalado en Windows 11)
- Para compilar:
  - Rust stable (1.82+)
  - Visual Studio Build Tools 2022 (C++ workload)

## Compilar

```cmd
COMPILAR_WINDOWS11.cmd
```

O manualmente:

```cmd
cargo build --release --target x86_64-pc-windows-msvc
```

El ejecutable queda en `target/x86_64-pc-windows-msvc/release/streamify-browser.exe`.

## Diseño

### Principios
- **Un solo WebView2 activo**: solo la pestaña activa tiene un controlador WebView2 vivo. Las pestañas inactivas son metadatos (URL + título).
- **`--disable-gpu`**: renderizado por software en WebView2, sin aceleración GPU para contenido.
- **Event-driven**: loop `GetMessage`/`DispatchMessage`, sin timers ni polling para estadísticas.
- **Sin telemetría**: el navegador no envía datos a ningún servidor propio.

### Tema visual
- Fondo: `#0d0d0d`
- Acento emerald: `#10b981`
- Sin animaciones, transiciones ni transparencias

### Seguridad
- Solo navegación HTTP/HTTPS
- Sin `--no-sandbox`, `--single-process` ni flags inseguros
- Contenido remoto sin permisos ni host objects
- No modifica Windows globalmente ni procesos de otras aplicaciones
- No borra datos reales del usuario

### Atajos de teclado
| Atajo | Acción |
|-------|--------|
| Ctrl+L | Enfocar barra de dirección |
| Ctrl+T | Nueva pestaña |
| Ctrl+W | Cerrar pestaña actual |
| Ctrl+R / F5 | Recargar página |

## Estructura del proyecto

```
src/
├── main.rs         – Punto de entrada, COM init, message loop
├── window.rs       – Ventana principal, WndProc, GDI painting
├── webview.rs      – WebView2 environment/controller
├── tabs.rs         – Estado de pestañas
├── database.rs     – SQLite (history, bookmarks, sessions)
├── url_util.rs     – Normalización de URLs
├── performance.rs  – Medición RAM/CPU (Windows API)
└── constants.rs    – Colores, dimensiones, IDs
```

## Dependencias

| Crate | Versión | Propósito |
|-------|---------|-----------|
| windows | 0.62 | Win32 API bindings |
| webview2-com | 0.39 | WebView2 COM bindings |
| rusqlite | 0.32 (bundled) | SQLite |
| serde + serde_json | 1 | Serialización (sesiones) |
| url | 2 | Parsing de URLs |
| urlencoding | 2 | Encoding para búsquedas |

## Benchmarking

Ver `INSTRUCCIONES_PARA_CLAUDE.md` Fase 3 para escenarios de benchmark.
Para medir rendimiento manualmente:

1. Abrir Task Manager → pestaña Detalles
2. Buscar `streamify-browser.exe` y procesos `msedgewebview2.exe` asociados
3. Sumar Private Working Set de todos los procesos del árbol
4. CPU: observar % CPU total en reposo (debe tender a 0% tras carga)

## Estado

- [x] Fase 0: Inspección de v0.1.3
- [x] Fase 1: Prototipo nativo Win32 + WebView2
- [ ] Fase 2: Optimización event-driven, bloqueo de ads opcional
- [ ] Fase 3: Benchmarks y aceptación

> **Nota**: Este código fue desarrollado en Linux y no ha sido compilado ni probado en Windows.
> El spec lo contempla: "Si no puedes ejecutar Windows, entrega cambios verificables,
> pruebas disponibles y procedimiento reproducible, con estado real."
