#import "@preview/polylux:0.4.0": *
#set page(width: 600pt, height: 360pt, margin: 30pt)
#set text(font: "Libertinus Serif", size: 22pt)

#slide[
  = A step with no visible change

  Stable paragraph

  #uncover("3-")[Appears after two clicks]
]

#slide[
  = Links and notes

  #metadata((t: "Note", v: "Shared package note")) <pdfpc>
  #only(2)[#metadata((t: "Note", v: "Second step note")) <pdfpc>]
  #link(<destination>)[Go to the last slide]

  #uncover("2-")[Second step]
]

#slide[
  = Destination <destination>

  #link("https://typst.app/")[External link]
]
