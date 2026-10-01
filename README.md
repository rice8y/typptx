# <img src="docs/assets/logo.svg" alt="Typptx" width="100%">

Typptx is a command-line tool that converts Typst to PowerPoint with editable text, lists, tables, equations, and vector shapes.

Typptx has the following features:

- Native list numbering and tables with merged cells
- Editable Office Math or Typst-rendered SVG equations
- Font embedding with support for variable fonts

## Installation

### From crates.io

> [!NOTE]
> v0.1.0 is currently under development and has not yet been published to crates.io.

Requires Rust 1.93+ and a C++17 compiler (MSVC on Windows). HarfBuzz is bundled in the crate.

```sh
cargo install typptx --locked
```

### From prebuilt binaries

Download from [Releases](https://github.com/rice8y/typptx/releases) and add the executable to your `PATH`.

- macOS: Apple Silicon / Intel
- Linux: x86_64, glibc 2.35+
- Windows: x86_64

### From source

```sh
git clone https://github.com/rice8y/typptx.git
cd typptx
cargo install --path . --locked
```

## Usage

```sh
typptx slides.typ -o slides.pptx
```

| Option | Description |
| --- | --- |
| `--math-format office\|svg` | Editable Office Math (default) or SVG equations |
| `--image-dpi DPI` | Limit embedded image resolution |
| `--root PATH` | Typst project root |
| `--font-path PATH` | Additional font directory |
| `--input KEY=VALUE` | Typst `sys.inputs` value |
| `--report PATH` | JSON diagnostics |
| `--strict` | Fail on any diagnostic |
| `--allow-image-fallback` | Export unsupported content as images |

See `typptx --help` for all options.

## Limitations

- Layout may differ from Typst.
- Rotated tables, text on paths, complex SVG effects, and some equations are unsupported.
- Embedded PDFs become native paths, gradients, and individual pictures. Text with usable OpenType fonts and Unicode mappings stays editable; other glyphs become vector outlines. PDF blend modes, soft masks, and mesh shadings are unsupported.
- Unsupported content stops conversion by default. Image fallbacks lose internal editability.
- Fonts that cannot be embedded must be installed locally.
- All pages must have the same dimensions.
- Touying animation steps become static slides.

## License

This project is distributed under the MIT License. See [LICENSE](https://github.com/rice8y/typptx/blob/main/LICENSE) for details.
