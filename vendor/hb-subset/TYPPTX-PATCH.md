# Bundled HarfBuzz

The HarfBuzz source is the official [14.5.0 release](https://github.com/harfbuzz/harfbuzz/releases/tag/14.5.0). Its copyright notices and Old MIT license are retained in `harfbuzz/COPYING`.

Typptx builds these sources directly from its root `build.rs`, including when installed from the published crate. It does not depend on the registry version of hb-subset. The build setup originated in hb-subset 0.3.0; its MIT license is retained in `LICENSE.md`. The former Rust wrapper is no longer used.

`src/assets/fonts/harfbuzz.cc` provides a small C ABI for font instancing. It pins every variation axis, preserves all glyphs, and enables `DOWNGRADE_CFF2` for Office embedding. The HarfBuzz algorithms are unchanged. The bundled build uses C++17 and supports MSVC `/bigobj` and MinGW `-Wa,-mbig-obj`.
