# Streamify Browser v0.2.0

Navegador para Windows 11 x64 con interfaz Rust/Win32 y motor Microsoft Edge WebView2. Incluye accesos al reproductor web de Spotify y al chat de Twitch.

## Descargar y abrir

1. En [GitHub Actions](https://github.com/tunasfn-png/streamify-browser/actions/workflows/windows.yml), abre una ejecución exitosa de **Windows x64**.
2. En **Artifacts**, descarga **Streamify-Browser-v0.2.0-Windows-x64**.
3. Extrae el ZIP y abre `streamify-browser.exe`. Usa el WebView2 Runtime que se distribuye con Windows 11.

Abre **☰ → Abrir Spotify** para ir al reproductor web. Para Twitch, selecciona **☰ → Abrir chat de Twitch…**, reemplaza `<canal>` por el nombre del canal y pulsa **Enter**. El chat se abre en su propia página, sin la transmisión de vídeo. También puedes escribir `twitch:nombre_del_canal` en la barra de direcciones.

Consulta la [arquitectura, atajos y pruebas](streamify-browser-native/README.md).
