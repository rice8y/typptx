// https://polylux.dev/book/dynamic/reserve.html
// https://polylux.dev/book/dynamic/obo-lbl.html
// https://polylux.dev/book/dynamic/handout.html
#import "@preview/polylux:0.4.0": *
#set page(paper: "presentation-16-9", margin: 25pt)
#set text(font: "Libertinus Serif", size: 20pt)
#enable-handout-mode(sys.inputs.at("handout", default: "false") == "true")
#slide[
  Always marker
  #only(1)[Only first marker]
  #uncover("2-")[Later marker]
]
#slide[
  #one-by-one[Alpha marker ][Beta marker ][Gamma marker]
]
#slide[
  #item-by-item[
    - First list marker
    - Second list marker
  ]
]
