#set page(width: 420pt, height: 250pt, margin: 28pt,
  header: [List header],
  footer: grid(columns: (1fr, 1fr), [List footer],
    rect(width: 24pt, height: 8pt, fill: blue)))
#set text(font: "Libertinus Serif", size: 14pt)
#set par(leading: 0.7em)
#list(..range(18).map(i => [
  #("item-" + str(i) + "-end")

  #link("https://example.com")[Details] and #super[raised] text.
  #enum(start: 3, [NestedAlpha], [NestedBeta])
]))
