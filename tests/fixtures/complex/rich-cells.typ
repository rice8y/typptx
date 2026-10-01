#set page(width: 540pt, height: 740pt, margin: 30pt)
#set text(font: "Libertinus Serif", size: 14pt)
#let card(body) = block(inset: 6pt, fill: luma(95%), body)
#table(columns: (100pt, 1fr), inset: 8pt,
  [Grid], [#grid(columns: (1fr, 1fr, 1fr), gutter: 5pt,
    [GridAlpha], [GridBeta], [GridGamma])],
  [Lists], [
    - ListAlpha
      - ListNested
    - ListBeta

    + EnumAlpha
    + EnumBeta
  ],
  [Nested], [
    BeforeNested

    #table(columns: 2, [InnerAlpha], [InnerBeta])

    AfterNested
  ],
  [Math], [$frac(a+b, sqrt(c)) + cancel(x)$],
  [Graphics], [#image(bytes("<svg xmlns='http://www.w3.org/2000/svg' width='40' height='20'><rect width='40' height='20' fill='blue'/></svg>"), width: 40pt)],
  [Block], [#card[StyledCell]],
)
