// https://touying-typ.github.io/docs/reference
#import "@preview/touying:0.8.0": *
#import themes.simple: *
#import "../slide-content.typ": lists, details
#show: simple-theme.with(aspect-ratio: "16-9", footer: [Touying footer])
#set text(font: "Libertinus Serif")
#slide[== Native structure
  #lists
]
#slide[== Native details
  #details
]
