# Streamify Browser v0.2.0

La ventana, barra de direcciones y pestañas usan Rust, Win32 y GDI. El contenido web usa Microsoft Edge WebView2 con `--disable-gpu`. WebView2 y SQLite se enlazan mediante `webview2-com` y `rusqlite` con SQLite integrado. El programa no requiere herramientas de desarrollo en el equipo donde se ejecuta.

## Pestañas y datos

- Las pestañas inactivas conservan su URL y título. Se crean controladores WebView2 bajo demanda y se reutiliza el mismo entorno y perfil.
- Puede conservarse una pestaña de Spotify en segundo plano para mantener su reproductor al cambiar al chat. Se oculta su controlador y se solicita al motor un objetivo de memoria bajo. El motor mantiene sus scripts activos.
- La interfaz usa mensajes de Win32. El búfer de GDI ocupa el área de la barra de herramientas y las operaciones de SQLite se ejecutan ante eventos de navegación y sesión.
- El perfil y la base de datos se guardan en `%LOCALAPPDATA%\Streamify Browser`. Si ya hay una base de datos de la versión original junto al ejecutable, se sigue usando esa base de datos.

| Atajo | Acción |
|---|---|
| Ctrl+L | Enfocar la dirección |
| Ctrl+T | Nueva pestaña |
| Ctrl+W | Cerrar la pestaña actual |
| F5 o Ctrl+R | Recargar |

## Compilación y verificación en Windows

[Windows x64](../.github/workflows/windows.yml) compila con Rust 1.100.0 y el toolchain MSVC para `x86_64-pc-windows-msvc` en un runner Windows hospedado por GitHub. Usa `Cargo.lock`, ejecuta pruebas unitarias y publica el ejecutable como artefacto de la ejecución. Puede iniciarse desde **Actions → Windows x64 → Run workflow** cuando el flujo está en la rama predeterminada.

La prueba del `.exe` abre una página local, navega desde el control de edición, abre otra pestaña y verifica la restauración de sesión. También comprueba `--disable-gpu` en el proceso WebView2 y registra el conjunto de trabajo privado y la CPU de todo el árbol durante el reposo de esa página. El rendimiento de una sesión iniciada en Spotify y Twitch depende también del contenido, la actividad del chat y la versión de WebView2.
