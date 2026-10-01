# Slide package regression fixtures

`cargo test --test slide_packages --test animations` runs the fixtures below. They also run in the
ordinary CI matrix. Imports pin Touying 0.8.0, Polylux 0.4.0, and Diatypst 0.9.3.
The first run needs access to Typst Universe. Later runs use Typst's package
cache. Dates are fixed and focused fixtures use embedded Libertinus fonts.

| Fixture | Assertions |
| --- | --- |
| Each package's `sample.typ` | Page count, native text, no image fallback, valid PPTX; Touying speaker notes and body/footer separation |
| Each package's `structure.typ` | 16:9 layout, nested bullets, numbering or styled number markers, merged table cells, code, Office Math, columns and web links |
| Touying `overlays.typ` | Visible and absent markers for pause, meanwhile, only, uncover, alternatives; final subslide in handout mode |
| Touying `overflow.typ` | Logical slide boundaries when an overlay spans several physical pages |
| Polylux `overlays.typ` | Visible and absent markers for only, uncover, one-by-one and item-by-item; handout visibility |
| Polylux `code.typ` | Progressive code visibility and cover color |
| Polylux `animation-cases.typ` | Empty click steps, internal link remapping, and notes from individual pdfpc metadata |
| Diatypst `navigation.typ` | 4:3 layout, title, sections, footer, normal/full themes and internal slide destinations |

`slide-content.typ` supplies the same lists, table and detailed content to each
package. Diatypst's default numbering function produces colored literal
markers; the test checks that these remain native bullet markers.

`slide_packages` explicitly uses `--animations slides` semantics to compare
every source page. `animations` uses the default native mode and verifies that
each click state retains those pages' object geometry and stacking order. It
also checks timing targets, click counts, handout behavior, and Touying with
pdfpc export disabled. Diatypst has no animation API and remains static.

References used to design the focused fixtures:

- [Touying API reference](https://touying-typ.github.io/docs/reference), [simple animations](https://touying-typ.github.io/docs/tutorials/dynamic/simple), [handout mode](https://touying-typ.github.io/docs/tutorials/dynamic/handout)
- [Polylux manual](https://polylux.dev/book/), [handout mode](https://polylux.dev/book/dynamic/handout.html), [code reveal](https://polylux.dev/book/dynamic/reveal-code.html)
- [Diatypst reference](https://mdwm.org/diatypst/reference.html)

These checks validate source visibility, native object structure, and OOXML.
PowerPoint rendering and save/edit/reopen checks use the separate workflow in
[`tools/README.md`](../../tools/README.md).
