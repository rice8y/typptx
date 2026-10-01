#import "@preview/polylux:0.4.0": *
#set page(width: 720pt, height: 405pt, margin: 35pt)
#set text(font: "Libertinus Serif", size: 24pt)

#slide[
  = Native click animation

  Always visible

  #uncover("2-")[Second step]

  #only((1, 3))[Visible on steps one and three]

  #only(2)[Replacement on step two]

  #uncover("3-")[Third step $x^2 + y^2 = z^2$]
]

#slide[
  = Editable animated objects

  #uncover("2-")[
    #table(columns: (150pt, 150pt), [EDITME], [Cell B], [Cell C], [Cell D])
  ]

  #uncover("3-")[#rect(width: 160pt, height: 50pt, fill: blue)]
]

#slide[
  = Last slide

  Finished
]
