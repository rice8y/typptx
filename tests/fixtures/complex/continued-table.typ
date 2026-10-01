#set page(width: 480pt, height: 260pt, margin: 24pt,
  header: [Table header],
  footer: grid(columns: (1fr, 1fr, 1fr),
    [Footer left], [Footer middle], [Footer right]))
#set text(font: "Libertinus Serif", size: 12pt,
  dir: if sys.inputs.at("rtl", default: "false") == "true" { rtl } else { ltr })
#table(
  columns: (72pt, 1fr), rows: 28pt,
  column-gutter: 8pt, row-gutter: 4pt, inset: 4pt,
  table.header([Key], [Value]),
  ..range(24).map(i => ([#i], [#("record-" + str(i) + "-end")])).flatten(),
  table.footer([Done], [Table footer]),
)
