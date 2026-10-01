// https://polylux.dev/book/toolbox/side-by-side.html
#import "@preview/polylux:0.4.0": *
#import "../slide-content.typ": lists, details
#set page(paper: "presentation-16-9", margin: 25pt, footer: [Polylux footer])
#set text(font: "Libertinus Serif", size: 20pt)
#slide[== Native structure
  #lists
]
#slide[== Native details
  #details
  #toolbox.side-by-side(columns: (1fr, 2fr))[Narrow column][Wide column]
]
