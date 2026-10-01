// Shared native-object checks used inside each package's slide layout.
#let lists = [
  #set text(size: 16pt)
  #grid(columns: (1fr, 1fr), gutter: 12pt,
    [
      - Parent marker
        - Nested marker
      - Last marker
    ],
    [
      + First numbered
      + Second numbered
    ],
  )
  #table(
    columns: (1fr, 1fr, 1fr), inset: 4pt, stroke: 0.7pt,
    table.cell(colspan: 2, fill: rgb("ddeeff"))[Merged marker], [Header C],
    [Cell A], [Cell B], [Cell C],
  )
]

#let details = [
  #set text(size: 16pt)
  #grid(columns: (1fr, 1fr), gutter: 12pt,
    [Left column marker\ $ (a+b)/c = sqrt(x) $],
    [Right column marker\ #link("https://typst.app/")[Documentation link]],
  )
  #set raw(lang: "rust")
  ```rust
  let native = 42;
  println!("editable code");
  ```
]
