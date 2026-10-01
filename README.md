# <img src="docs/assets/logo.svg" alt="Typptx" width="100%">

Typptx is a command-line tool that converts Typst to PowerPoint with editable text, lists, tables, equations, and vector shapes.

Typptx has the following features:

- Editable text, lists, and tables
- Native click animations for Touying and Polylux overlays
- Editable vector shapes from Typst and CeTZ drawings
- Editable Office Math or Typst-rendered SVG equations
- SVG and PDF images with vector graphics preserved
- Hyperlinks and speaker notes
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
| `--animations native\|slides` | Click animations (default) or separate static slides for each overlay |
| `--image-dpi DPI` | Limit embedded image resolution |
| `--root PATH` | Typst project root |
| `--font-path PATH` | Additional font directory |
| `--input KEY=VALUE` | Typst `sys.inputs` value |
| `--report PATH` | JSON diagnostics |
| `--strict` | Fail on any diagnostic |
| `--allow-image-fallback` | Export only unsupported content as images and report each fallback |

See `typptx --help` for all options.

## Limitations

- Layout may differ from Typst; all pages must have the same dimensions.
- Centered or complex lists use separate text boxes without automatic renumbering. Complex table cells may also use separate objects.
- Inline graphics and highlights do not follow text edits.
- Rotated or reflected tables, text on paths, and some equations require image fallback, which loses editability. Enable it with `--allow-image-fallback`.
- Fonts that cannot be embedded must be installed locally.
- Animations use discrete states. Changed content uses separate objects that overlap in editing and print views and must be edited independently.
- Links target slides, not individual animation steps.

## License

This project is distributed under the MIT License. See [LICENSE](https://github.com/rice8y/typptx/blob/main/LICENSE) for details.
