//! build.rs del api: garantiza que `console/dist/` exista antes de que
//! `rust-embed` lo incruste. Si la consola no se ha construido (sin Node),
//! genera un stub mínimo: compilar sigue funcionando y `/console` sirve una
//! página que lo explica.

use std::path::PathBuf;

fn main() {
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("manifest dir"));
    let dist = manifest.join("../../console/dist");
    if !dist.exists() {
        std::fs::create_dir_all(&dist).expect("crear console/dist");
        std::fs::write(
            dist.join("index.html"),
            "<!doctype html><html lang=\"es\"><head><meta charset=\"utf-8\">\
             <title>Integraciones — consola</title></head>\
             <body><h1>La consola no está construida</h1>\
             <p>Ejecuta <code>npm install &amp;&amp; npm run build</code> en <code>console/</code>.</p>\
             </body></html>",
        )
        .expect("escribir el stub de console/dist");
    }
    println!("cargo:rerun-if-changed=../../console/dist");
}
