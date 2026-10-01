# PowerPoint regression checks

`powerpoint_qa.py` checks three kinds of evidence separately:

- PPTX package: native tables, equations, column widths, row heights, and pictures.
- Local PDF export: text, line breaks, glyph positions, and bounds in declared regions.
- Office save/edit/reopen: native cell content, merges, margins, and track geometry survive; a declared cell edit is present after reopening.

Package success does not prove that PowerPoint renders the file correctly. Missing Office evidence is reported as `skipped`. `--require-office` fails when any required evidence is absent. A fixture only covers its declared regions and objects; these checks do not replace visual review of complex formulas or artwork.

## Local use

Python 3.11+ is sufficient for package checks and harness tests. PDF checks also need `pdfplumber`:

```sh
python -m pip install -r tools/powerpoint-qa-requirements.txt
python -m unittest discover -s tools -p 'test_*.py'
cargo run -- tests/fixtures/powerpoint/text-layout.typ --strict -o build/qa/input.pptx
python tools/powerpoint_qa.py \
  --manifest tests/fixtures/powerpoint/text-layout.json \
  --pptx build/qa/input.pptx --pdf build/qa/powerpoint.pdf \
  --output build/qa/report.json
```

Export `powerpoint.pdf` with the desktop PowerPoint local PDF exporter. On macOS, select **Best for printing** each time; the export dialog can reset to its online option. Use a separate output copy to test saving and editing. The source deck is never a save target.

`--baseline reviewed-report.json` additionally compares line breaks and glyph origins against a reviewed PDF observation. Keep separate baselines for each Office/OS/font environment. The tool never updates baselines automatically. JSON observations record artifact SHA-256 hashes.

### Animation playback

Build `tests/fixtures/powerpoint/animations.typ` with the default animation mode.
Its seven source pages become three PowerPoint slides. Inspect slideshow playback:

- Slide 1 initially shows the title, `Always visible`, and `Visible on steps one
  and three`. One click reveals `Second step` and replaces the transient text.
  The next click restores the original transient text and reveals the equation.
  Verify that the Left arrow returns to the preceding state.
- Slide 2 starts with its title. The first click reveals the native table; the
  second reveals the blue rectangle. Change `EDITME` to `EDITED` in the table,
  save a separate copy, close and reopen it, and repeat both clicks.
- Slide 3 is static. It follows the last animation step on slide 2.

Also build `tests/fixtures/polylux/animation-cases.typ`. The first click on slide
1 must leave the appearance unchanged; the second reveals `Appears after two
clicks`. The link on slide 2 must reach slide 3, and the notes must contain
`Shared package note` once, followed by `Second step note`. Repeat the empty
click check after saving and reopening. Empty steps use a transparent shape
named `Animation pause`, because PowerPoint discards empty timing nodes.

Run `cargo test --test animations` for package and source-state checks. The PDF
checker above does not validate animation playback. To inspect each source
state as a printable page, also export with `--animations slides`.

### Fixture manifest

`limited-native.typ` covers rotated tables (including nested tables inside a
list), simple horizontal reflections, middle equation delimiters, and
`overshell`. Build it with `--strict`. Tables on slides 1–3 are groups of
editable text and borders. On slide 2, change `EDITME` to `EDITED`, save a copy,
close and reopen it, and check that its position and mirrored glyphs survive.
On slide 4, check that the middle bars stretch to the fractions and that the
shell and its annotation remain above `x+y`.

`links-markers.typ` and `links-markers.json` cover source hyperlink colors and
graphical picture bullets. Build that fixture with `--strict` and use its JSON
manifest with the same Python checker and Windows runner. The declared native
cell edit changes `EDIT ME` to `EDITED` on slide 1.
Table PDF regions check line counts because Office can omit whitespace glyphs
while preserving their visible advances. Exact cell text is checked in the
saved PPTX package.

After saving and reopening, also inspect these details in desktop PowerPoint:

- Slide 1: black, green and red link text keeps its source color. Office may add
  its native hyperlink underline. The green link goes to slide 2.
- Slide 2: square, image-plus-text and circle-plus-text markers remain visible.
  Edit a list item and insert another item to check native bullet behavior,
  including the list inside the table cell.
- Slide 3: rotated, cropped and equation-containing markers remain visible;
  their body text remains editable.

The PDF region checks verify text and wrapping; the visual checks above are
still required for colors and artwork. Record Windows results only after
running them on a Windows desktop with PowerPoint installed.

`continued-lists.typ` and `continued-lists.json` cover a table whose two cells
continue onto a second slide, followed by two-color, superscript, fraction and
rotated text markers. Check that each column contains items 0 through 17 once,
and that the mathematical markers remain attached to their paragraphs. The
declared edit changes `Left-12` to `Edited-12` in the continued cell on slide 2.
Glyph positions also check that cell padding is not added again after the page
break. The first body characters start at 55.318pt and 317.4734pt in the source;
the manifest records reviewed Office glyph origins with a 0.25pt tolerance.
Build this fixture once with default math output and once with
`--math-format svg`; the markers should look the same in both.

`rich-lists.typ` and `rich-lists.json` cover paragraph alignment inside cells,
empty and hidden list markers, and list bodies containing native shapes and a
nested table. The declared edit replaces the right-aligned cell text with
`Edited`; it should stay right-aligned after reopening. A second edit changes
the nested table cell from `Editable cell` to `Edited cell`. Also inspect the empty
marker indents, the red inline rectangle, the callout's three lines and its
background, and the numbered items 1 through 4. The list group contains separate
editable text, shape and table objects; moving the group moves them together.

`paragraph-groups.typ` and `paragraph-groups.json` cover centered lists (including
RTL, table cells and multiline items), grids and columns inside lists, and
complete paragraphs containing inline graphics or highlights. Centered list
markers and their body paragraphs are separate native objects: the body keeps
its full wrapping width, while the marker stays at the source margin. In table
cells, this content is grouped with the native table. Numbered marker objects
retain their source numbers but do not form a shared automatic numbering
sequence across text boxes.

The declared cell edit changes `EDIT ME` to `EDITED` on slide 3. Also change
`First` to `Edited` on slide 1 and `Before` to `Ahead` in the first paragraph on
slide 3. Check that the centered body stays centered, its marker stays at the
margin, and `Ahead`, the tab for the rectangle, and `after.` remain in one text
paragraph after saving and reopening. Graphics and highlight backgrounds are
independently editable shapes; changing text does not move or resize those
shapes automatically. The PDF regions check both column baselines and wrapping.

`tests/fixtures/powerpoint/text-layout.json` is a complete example. Slide/table/row/column indexes are one-based. Table order follows native shape order, including groups. All dimensions are points. Region boxes are `[left, top, right, bottom]` in PDF page coordinates. Regions must include the entire area where an unwanted wrapped line could appear. Use `content_box_pt` for a smaller permitted text box inside that search region.

`lines` checks extracted text and wrapping; `line_count` checks only the number of lines. `first_line_x_pt` checks glyph origins against source measurements. `ignore_trailing_space_origins` excludes invisible trailing spaces from that check while preserving them in the PDF observation. `min_font_pt` can exclude nearby small labels. Plain left-to-right text is grouped by glyph top coordinate (`line_tolerance_pt`, default 1.5). Do not apply that line grouping to mixed superscripts, equations, combining marks, right-to-left or vertical text; give plain text its own region or use a separately reviewed visual check. A change in PDF glyph mapping requires review rather than a silently accepted baseline update.

An edit specifies a native cell, its exact `before` and `after` text, and its PDF `region`. Font runs can split or merge without failing the semantic roundtrip checks.

### Office evidence

`--office-evidence office-evidence.json` records the operator or runner's renderer identity; filenames and PDF metadata alone do not identify the renderer. This record is evidence supplied by the operator, not cryptographic proof that an application ran:

```json
{
  "application": "Microsoft PowerPoint",
  "version": "16.x",
  "os": "macOS or Windows version",
  "export_method": "local-pdf-export",
  "artifacts": {
    "pptx": {"sha256": "SHA-256 of input PPTX"},
    "pdf": {"sha256": "SHA-256 of local PDF"},
    "roundtrip_pptx": {"sha256": "SHA-256 of saved and reopened PPTX"},
    "edited_pptx": {"sha256": "SHA-256 of edited and reopened PPTX"},
    "roundtrip_pdf": {"sha256": "SHA-256 of PDF after reopening"},
    "edited_pdf": {"sha256": "SHA-256 of PDF after editing and reopening"}
  }
}
```

Provide the corresponding `--roundtrip-pptx`, `--edited-pptx`, `--roundtrip-pdf`, and `--edited-pdf` arguments for complete coverage. `--require-office` requires every artifact, matching hashes, declared PDF regions, and a declared edit. The report records each check independently.

## Opt-in Windows Office runner

`powerpoint_roundtrip.ps1` opens a copy in desktop PowerPoint, exports locally, saves and reopens it, edits the declared native cell, saves and reopens it again, and exports both results. It writes the evidence record automatically. It refuses to reuse an existing PowerPoint session or overwrite an evidence directory.

```powershell
tools/powerpoint_roundtrip.ps1 -Presentation build/qa/input.pptx `
  -Manifest tests/fixtures/powerpoint/text-layout.json `
  -OutputDirectory build/office-run
```

The [CI workflow](../.github/workflows/ci.yml) runs the Rust and Python tests and builds the packaged crate on pull requests across macOS (Apple Silicon and Intel), Linux, and Windows. Touying, Polylux, and Diatypst fixtures run in the ordinary Rust suite; their pinned Typst packages are downloaded on first use and cached. Hosted runners do not run PowerPoint. The private-document corpus remains opt-in.

Run the PowerPoint script locally in an interactive Windows desktop session with licensed PowerPoint and the required fonts installed. The COM runner needs validation on the configured Windows/Office environment before treating its output as a baseline.

The runner uses Microsoft's documented [Open](https://learn.microsoft.com/en-us/office/vba/api/powerpoint.presentations.open), [SaveCopyAs](https://learn.microsoft.com/en-us/office/vba/api/powerpoint.presentation.savecopyas), [table cells](https://learn.microsoft.com/en-us/office/vba/api/powerpoint.table), and [ExportAsFixedFormat](https://learn.microsoft.com/en-us/office/vba/api/powerpoint.presentation.exportasfixedformat) APIs. It does not use LibreOffice or an online conversion service.
