# Bundled HarfBuzz and portability patch

Source: hb-subset 0.3.0 from crates.io. Its bundled HarfBuzz sources have been
updated from 8.2.2 to the official [HarfBuzz 14.5.0 release](https://github.com/harfbuzz/harfbuzz/releases/tag/14.5.0).
The upstream MIT license and HarfBuzz license are retained.

Changes:

- `Blob::from_file` uses Rust file I/O and a copying HarfBuzz blob, allowing
  native Unicode paths on Windows without Unix-only `OsStrExt`.
- The bundled build uses C++17, with MSVC `/bigobj` or MinGW `-Wa,-mbig-obj`.
- One return type explicitly spells its existing borrowed lifetime to silence a
  new Rust lint; Rust formatting has been normalized.

The subsetting and variation-instancing algorithms are unchanged upstream code.
Typptx pins every variation axis, preserves all glyphs, and sets HarfBuzz's
`DOWNGRADE_CFF2` flag to emit static CFF outlines for Office embedding. No Python
runtime is required. Remove this vendor patch when upstream provides equivalent
Windows support and a HarfBuzz version with CFF2 downgrading.
