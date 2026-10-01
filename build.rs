// Build setup adapted from hb-subset 0.3.0 (MIT).
// See vendor/hb-subset/LICENSE.md for its copyright and license.
fn main() {
    let target = std::env::var("TARGET").unwrap();
    let mut build = cc::Build::new();
    if target.contains("windows-msvc") {
        build.flag("/bigobj").flag("/std:c++17");
    } else {
        build.flag("-std=c++17");
        if target.contains("windows-gnu") {
            build.flag("-Wa,-mbig-obj");
        }
    }
    build
        .cpp(true)
        .warnings(false)
        .include("vendor/hb-subset/harfbuzz/src")
        .file("vendor/hb-subset/harfbuzz/src/harfbuzz-subset.cc")
        .file("src/assets/fonts/harfbuzz.cc")
        .compile("typptx-harfbuzz");
    println!("cargo:rerun-if-changed=vendor/hb-subset/harfbuzz/src");
    println!("cargo:rerun-if-changed=src/assets/fonts/harfbuzz.cc");
}
