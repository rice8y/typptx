Roboto Condensed is from [Google Fonts](https://github.com/google/fonts/tree/main/ofl/robotocondensed), licensed under the [SIL Open Font License](OFL.txt). The original variable font is used without modification as an embedding test fixture.

`SourceSerif4Variable-HelloWorld.otf` is Adobe Source Serif 4, licensed under the
[SIL Open Font License](SourceSerif-OFL.md). This small upstream test subset is
copied unchanged from `test/subset/data/fonts/SourceSerif4Variable-Roman-HelloWorld.otf`
in the official [HarfBuzz 14.5.0 release](https://github.com/harfbuzz/harfbuzz/releases/tag/14.5.0).
Product exports preserve the complete input font; this subset is only a test fixture.

`SourceSerif4-reference.json` contains independent advance widths and outline
bounds from fontTools 4.63.0, instantiated at `(wght, opsz)` values `(300, 12)`,
`(700, 12)`, and `(300, 48)`. Regenerate it with `python cff2_reference.py` from
this directory in a development environment with fontTools installed. The tests
allow one font unit of advance rounding and two units of outline rounding.
