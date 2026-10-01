#set page(
  width: 160mm,
  height: auto,
  margin: 0mm,
  fill: none
)

#let typst-color = rgb("#239DAD")
#let pptx-color = rgb("#C43E1C")
#let muted = rgb("#66747A")

#let background = gradient.linear(
  rgb("#E9F6F7"),
  rgb("#F7F5F1"),
  rgb("#FFF0E9"),
  angle: 0deg
)

// ─────────────────────────────────────
// typptx logo
// ─────────────────────────────────────

#let typptx-logo(size: 48pt) = context {
  let k-yp = size * 0.035
  let k-pp = size * -0.025
  let k-pt = size * -0.015

  let mix-pptx(base, amount) = color.mix(
    (base, 100% - amount),
    (pptx-color, amount),
    space: oklab,
  )

  let styled(body) = {
    set text(
      font: "Buenard",
      weight: "bold",
      size: size,
    )
    body
  }

  let whole = styled({
    text("t")
    text("y")
    h(k-yp)
    text("p")
    h(k-pp)
    text("p")
    h(k-pt)
    text("t")
    text("x")
  })

  let before-p1 = styled({
    text("t")
    text("y")
    h(k-yp)
  })

  let through-p1 = styled({
    text("t")
    text("y")
    h(k-yp)
    text("p")
  })

  let through-p2 = styled({
    text("t")
    text("y")
    h(k-yp)
    text("p")
    h(k-pp)
    text("p")
  })

  let through-t = styled({
    text("t")
    text("y")
    h(k-yp)
    text("p")
    h(k-pp)
    text("p")
    h(k-pt)
    text("t")
  })

  let total = measure(whole).width.pt()

  let ratio(body) = 100% * measure(body).width.pt() / total

  let p1-start = ratio(before-p1)
  let p1-end   = ratio(through-p1)
  let p2-end   = ratio(through-p2)
  let t-end    = ratio(through-t)

  let p1-mid = (p1-start + p1-end) / 2

  let pivot = typst-color.lighten(18%)

  let grad = gradient.linear(
    (typst-color.darken(12%), 0%),
    (typst-color, p1-start * 0.48),
    (pivot, p1-start),

    (mix-pptx(pivot, 30%), p1-mid),
    (mix-pptx(pivot, 55%), p1-end),
    (mix-pptx(pivot, 78%), p2-end),
    (mix-pptx(pivot, 92%), t-end),
    (pptx-color, 100%),

    angle: 0deg,
    space: oklab,
    relative: "parent"
  )

  box({
    set text(
      font: "Buenard",
      weight: "bold",
      size: size,
      fill: grad,
    )

    text("t")
    text("y")
    h(k-yp)

    text("p")
    h(k-pp)

    text("p")
    h(k-pt)

    text("t")
    text("x")
  })
}

// ─────────────────────────────────────
// geometric hex pattern
// ─────────────────────────────────────

#let soften(c, amount) = color.mix(
  (c, 100% - amount),
  (white, amount),
  space: oklab
)

#let brand-mix(amount) = color.mix(
  (typst-color, 100% - amount),
  (pptx-color, amount),
  space: oklab
)

#let hex-tile(
  size: 13mm,
  mix: 50%,
  wash: 55%,
  outline: false
) = {
  let base = brand-mix(mix)
  let light = soften(base, wash)

  box(
    width: size,
    height: size
  )[
    #if not outline {
      place(
        top + left,
        dx: 0.8mm,
        dy: 1.0mm,
        rotate(
          30deg,
          polygon.regular(
            vertices: 6,
            size: size,
            fill: soften(base.darken(20%), 72%),
            stroke: none
          )
        )
      )
    }

    #place(
      top + left,
      rotate(
        30deg,
        if outline {
          polygon.regular(
            vertices: 6,
            size: size,
            fill: none,
            stroke: (
              paint: soften(base, 48%),
              thickness: 0.65pt
            )
          )
        } else {
          polygon.regular(
            vertices: 6,
            size: size,
            fill: gradient.linear(
              soften(base, wash + 10%),
              light,
              angle: 135deg
            ),
            stroke: (
              paint: soften(base, wash - 8%),
              thickness: 0.45pt
            )
          )
        }
      )
    )
  ]
}


#let hex-field() = {
  let tiles = (
    (1mm,    15mm,    4%,   72%, true),
    (6mm,    31mm,   10%,   68%, true),

    (11mm,    5mm,   14%,   67%, false),
    (11mm,   25mm,   18%,   70%, true),
    (11mm,   43mm,   20%,   65%, false),

    (21mm,   -1mm,   27%,   60%, false),
    (21mm,   17mm,   31%,   52%, false),
    (21mm,   35mm,   36%,   62%, false),

    (31mm,    8mm,   43%,   56%, false),
    (31mm,   26mm,   48%,   48%, false),
    (31mm,   44mm,   52%,   59%, false),

    (41mm,   -1mm,   60%,   58%, false),
    (41mm,   17mm,   65%,   43%, false),
    (41mm,   35mm,   70%,   54%, false),

    (51mm,    8mm,   78%,   47%, false),
    (51mm,   26mm,   82%,   39%, false),
    (51mm,   44mm,   86%,   52%, false),

    (61mm,   -1mm,   91%,   48%, false),
    (61mm,   17mm,   95%,   35%, false),
    (61mm,   35mm,  100%,   43%, false),

    (71mm,    8mm,  100%,   41%, false),
    (71mm,   26mm,  100%,   34%, false),
    (71mm,   44mm,  100%,   46%, false)
  )

  box(
    width: 76mm,
    height: 52mm,
    clip: true
  )[
    #for (x, y, mix, wash, outline) in tiles {
      place(
        top + left,
        dx: x,
        dy: y,
        hex-tile(
          mix: mix,
          wash: wash,
          outline: outline,
        )
      )
    }
  ]
}

// ─────────────────────────────────────
// Hero
// ─────────────────────────────────────

#block(
  width: 160mm,
  radius: 5mm,
  clip: true,
  fill: background,
  inset: 0mm,
  stroke: none
)[
  #grid(
    columns: (1fr, 76mm),
    column-gutter: 0mm,
    align: (left + horizon, right + horizon),

    [
      #pad(
        left: 11mm,
        right: 9mm,
        top: 9mm,
        bottom: 8mm,
      )[
        #typptx-logo(size: 51pt)

        #v(4mm)

        #text(
          size: 14pt,
          weight: "bold",
          fill: muted,
        )[
          Typst in. PowerPoint out.
        ]
      ]
    ],

    [
      #hex-field()
    ]
  )
]
