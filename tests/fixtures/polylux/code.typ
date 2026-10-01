// https://polylux.dev/book/dynamic/reveal-code.html
#import "@preview/polylux:0.4.0": *
#set page(paper: "presentation-16-9", margin: 25pt)
#set text(font: "Libertinus Serif", size: 20pt)
#slide[
  #reveal-code(lines: (1, 2), full: true)[```
  CODE_FIRST
  CODE_SECOND
  CODE_THIRD
  ```]
]
