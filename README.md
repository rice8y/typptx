# <img src="docs/assets/logo.svg" alt="Typptx" width="100%">

Typptx is a command-line tool that converts Typst to PowerPoint with editable text, lists, tables, equations, and vector shapes.

CeTZ and Typst drawing primitives become editable shapes. Imported images, including SVG and PDF pages, stay individual pictures. SVG and PDF pages retain vector graphics within each picture.

Typptx has the following features:

- Editable text, lists, and tables
- Native click animations for Touying and Polylux overlays
- Editable vector shapes from Typst and CeTZ drawings
- Editable Office Math or Typst-rendered SVG equations
- Image import with vector graphics preserved in SVG and PDF pictures
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

Touying and Polylux overlays become click states within one PowerPoint slide.
Each click reproduces the next overlay using Appear and Disappear effects,
including content replacement and cover-color changes. Text, tables, equations,
and graphics remain editable. Internal links are remapped to the combined slides,
and repeated speaker notes are collected once.
If an animated Touying slide overflows onto several source pages, those pages become
additional click states in their original order.

Use `--animations slides` to keep every overlay as a separate static slide for
printing or PDF export. The slide package's own handout mode is also respected.

## Limitations

- Layout may differ from Typst.
- Centered lists and lists with complex layouts use separate text boxes. Numbering does not update automatically across those boxes.
- Inline graphics and highlight backgrounds are separate objects. Editing text does not move or resize them automatically.
- Cells containing centered lists or other complex layouts may use separate text and graphics grouped with the table.
- Rotated or reflected native tables, text on paths, and some equations are unsupported.
- Unsupported content stops conversion by default. Image fallbacks lose internal editability.
- Fonts that cannot be embedded must be installed locally.
- All pages must have the same dimensions.
- Animation changes are discrete; motion paths and timed interpolation are not generated.
- Content that changes between animation steps uses separate editable objects. These can overlap in PowerPoint's editing and print views; edits to one variant do not update the others.
- Links to a particular overlay do not select its animation step within the combined slide.

## License

This project is distributed under the MIT License. See [LICENSE](https://github.com/rice8y/typptx/blob/main/LICENSE) for details.
