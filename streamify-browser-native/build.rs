// Streamify Browser v0.2.0 – build.rs
// Embeds a Windows manifest for DPI awareness + visual styles via .rc compilation.
// Note: #![windows_subsystem = "windows"] in main.rs handles the subsystem setting.
// DPI awareness is also set at runtime via SetProcessDpiAwareness as a fallback.

fn main() {
    // Only run on Windows targets
    if std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default() != "windows" {
        return;
    }

    // Write the application manifest for:
    // - DPI awareness (per-monitor v2)
    // - Visual styles (Common Controls v6)
    // - UTF-8 code page
    let out_dir = std::env::var("OUT_DIR").unwrap();
    let manifest = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <application xmlns="urn:schemas-microsoft-com:asm.v3">
    <windowsSettings>
      <dpiAwareness xmlns="http://schemas.microsoft.com/SMI/2016/WindowsSettings">PerMonitorV2</dpiAwareness>
      <dpiAware xmlns="http://schemas.microsoft.com/SMI/2005/WindowsSettings">true/pm</dpiAware>
      <activeCodePage xmlns="http://schemas.microsoft.com/SMI/2019/WindowsSettings">UTF-8</activeCodePage>
    </windowsSettings>
  </application>
  <dependency>
    <dependentAssembly>
      <assemblyIdentity type="win32" name="Microsoft.Windows.Common-Controls" version="6.0.0.0"
        processorArchitecture="*" publicKeyToken="6595b64144ccf1df" language="*" />
    </dependentAssembly>
  </dependency>
</assembly>"#;

    let manifest_path = std::path::Path::new(&out_dir).join("app.manifest");
    std::fs::write(&manifest_path, manifest).expect("write manifest");

    // Write .rc file that references the manifest (resource ID 1, type RT_MANIFEST=24)
    let rc_path = std::path::Path::new(&out_dir).join("app.rc");
    let rc_content = format!(
        "1 24 \"{}\"",
        manifest_path.display().to_string().replace('\\', "\\\\")
    );
    std::fs::write(&rc_path, &rc_content).expect("write rc");

    // Try to compile the .rc with the Windows SDK resource compiler (rc.exe).
    // If rc.exe is not available, the manifest won't be embedded but the app still
    // works because DPI awareness is also set at runtime in main.rs.
    let res_path = std::path::Path::new(&out_dir).join("app.res");
    let rc_status = std::process::Command::new("rc.exe")
        .args(["/nologo", "/fo"])
        .arg(&res_path)
        .arg(&rc_path)
        .status();

    match rc_status {
        Ok(status) if status.success() => {
            // Link the compiled resource
            println!("cargo:rustc-link-arg={}", res_path.display());
        }
        _ => {
            // rc.exe not found or failed — skip manifest embedding.
            // DPI awareness and visual styles may need runtime init (already done).
        }
    }

    println!("cargo:rerun-if-changed=build.rs");
}
