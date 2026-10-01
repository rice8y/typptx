// https://touying-typ.github.io/docs/tutorials/dynamic/simple
// https://touying-typ.github.io/docs/tutorials/dynamic/handout
#import "@preview/touying:0.8.0": *
#import themes.simple: *
#show: simple-theme.with(
  aspect-ratio: "16-9",
  config-common(
    handout: sys.inputs.at("handout", default: "false") == "true",
    enable-pdfpc: sys.inputs.at("pdfpc", default: "true") == "true",
  ),
)
#set text(font: "Libertinus Serif")
#slide[
  Alpha marker
  #pause
  Beta marker
  #pause
  Gamma marker
]
#slide[
  #only(1)[Only first marker]
  #uncover("2-")[Later marker]
  #alternatives[Alternative one][Alternative two]
]
#slide[
  Parallel A
  #pause
  Parallel B
  #meanwhile
  Parallel C
  #pause
  Parallel D
]
