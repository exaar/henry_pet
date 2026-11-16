use std::path::PathBuf;

/// Embed every available skin (spritesheet + pet.json); the tray "Skin" menu switches between them live.
/// Skins: Henry's own sheet (ui/pets/henry, M4). The petdex placeholder skin is retired.
/// HENRY_PUBLIC=1 (npm run build:public) refuses to build without Henry's sheet.
/// HENRY_SPRITE_DIR=<dir> embeds a sheet from any folder as the only skin (dev tests; refused for public builds).
fn main() {
    let repo = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap()).parent().unwrap().to_path_buf();
    let public = std::env::var("HENRY_PUBLIC").as_deref() == Ok("1");
    println!("cargo:rerun-if-env-changed=HENRY_PUBLIC");
    println!("cargo:rerun-if-env-changed=HENRY_SPRITE_DIR");
    // (id, tray label, folder)
    let mut dirs: Vec<(&str, &str, PathBuf)> = Vec::new();
    if let Ok(d) = std::env::var("HENRY_SPRITE_DIR") {
        if public {
            panic!("public build: HENRY_SPRITE_DIR is for dev tests only");
        }
        dirs.push(("dev", "Dev sheet", PathBuf::from(d)));
    } else {
        dirs.push(("henry", "Henry", repo.join("ui/pets/henry")));
        // the petdex placeholder is retired (2026-11-13); HENRY_SPRITE_DIR=ui/pets/placeholder still loads it
    }
    let mut skins = Vec::new();
    for (id, label, d) in &dirs {
        for f in ["spritesheet.webp", "spritesheet.png", "pet.json"] {
            println!("cargo:rerun-if-changed={}", d.join(f).display());
        }
        let Some(sheet) = ["spritesheet.webp", "spritesheet.png"].iter().map(|f| d.join(f)).find(|p| p.exists()) else {
            continue;
        };
        let manifest = sheet.with_file_name("pet.json");
        let manifest = if manifest.exists() { format!("Some(include_str!({:?}))", manifest.display().to_string()) } else { "None".into() };
        skins.push(format!(
            "    Skin {{ id: {id:?}, label: {label:?}, sprite: include_bytes!({:?}), manifest: {manifest} }},\n",
            sheet.display().to_string()
        ));
    }
    if skins.is_empty() {
        if public {
            panic!("public build: Henry's own spritesheet (ui/pets/henry/) is missing");
        }
        panic!("no spritesheet: run `npm run pack-sprites` (ui/pets/henry/)");
    }
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("sprite.rs");
    std::fs::write(out, format!("pub const SKINS: &[Skin] = &[\n{}];\n", skins.concat())).unwrap();
    tauri_build::build()
}
