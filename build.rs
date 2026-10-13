fn main() {
    println!("cargo::rustc-check-cfg=cfg(embedded_ffmpeg)");
    println!("cargo::rerun-if-env-changed=SPURLOS_EMBED_FFMPEG");
    if let Ok(src) = std::env::var("SPURLOS_EMBED_FFMPEG") {
        let out = std::path::Path::new(&std::env::var("OUT_DIR").unwrap()).join("ffmpeg.bin");
        std::fs::copy(&src, out).expect("SPURLOS_EMBED_FFMPEG zeigt auf keine lesbare Datei");
        println!("cargo::rerun-if-changed={src}");
        println!("cargo::rustc-cfg=embedded_ffmpeg");
    }
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/Spurlos.ico");
        res.set("ProductName", "Spurlos");
        res.set("FileDescription", "Spurlos");
        res.compile().expect("Windows-Ressourcen konnten nicht erzeugt werden");
    }
}
