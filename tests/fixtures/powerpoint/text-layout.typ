#set page(width: 720pt, height: 480pt, margin: 0pt)
#set text(font: "Libertinus Serif", size: 20pt)
#set par(leading: 6pt)

// Fixed coordinates make the PDF regions independent of surrounding flow.
#place(dx: 32pt, dy: 24pt)[#text(tracking: 1pt)[HHHH]]
#place(dx: 32pt, dy: 80pt)[
  #table(columns: (140pt, 180pt), rows: (64pt, 88pt), inset: 8pt,
    align: (left, right),
    [EDIT ME], [AVATAR],
    [First line\ Second line], [0.95\ 1.25],
  )
]
#place(dx: 384pt, dy: 80pt)[
  #table(columns: 300pt, rows: (76pt, 76pt), inset: 8pt,
    [Before $x^2$ after],
    [An office ligature#super[1] after],
  )
]
#place(dx: 32pt, dy: 280pt)[
  #table(columns: (160pt, 160pt), rows: (48pt, 48pt), inset: 8pt,
    table.cell(colspan: 2)[Merged editable cell],
    [Left], [Right],
  )
]

#pagebreak()

// Source width: 4 * 14.6pt + 3 * 1pt = 61.4pt. The adapter's line-fit
// budget is 61.375pt, so both source lines exercise the rounding boundary.
#place(dx: 32pt, dy: 80pt)[
  #set text(tracking: 1pt)
  #table(columns: 77.4pt, rows: 100pt, inset: 8pt,
    [HHHH\ HHHH],
  )
]
#place(dx: 200pt, dy: 80pt)[
  #set text(tracking: 1pt)
  #table(columns: 77.4pt, rows: 100pt, inset: 8pt,
    [HHHH HHHH],
  )
]
