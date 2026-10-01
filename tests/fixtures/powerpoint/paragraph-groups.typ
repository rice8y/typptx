#set page(width:600pt,height:460pt,margin:30pt)
#set text(font:"Libertinus Serif",size:18pt)
= Aligned lists
#align(center)[
- First
- Second
]
#align(right)[
- Third
- Fourth
]
#align(center)[
  #set text(dir:rtl)
  - RTL first
  - RTL second
]
#table(columns:300pt,inset:8pt,[#align(center)[
- Cell first
- Cell second
]])
#block(width:300pt)[#align(center)[
+ Short \
  A longer centered line remains in one paragraph.
]]
#pagebreak()
= Side-by-side list bodies
+ Before grid

  #grid(columns:(150pt,150pt),[Left],[Right])
+ After grid

+ Before columns

  #columns(2)[Left column contains enough text to wrap over two lines. #colbreak() Right column also wraps into several lines.]
+ After columns
#pagebreak()
= Inline graphics and highlights
Before #box[#rect(width:12pt,height:12pt,fill:red)] after.

Before #highlight(fill:yellow)[highlighted words] after.

#block(width:220pt)[Before #highlight(fill:yellow)[highlighted words that wrap over several lines] after.]

#table(columns:200pt,inset:8pt,[EDIT ME])

#block(width:220pt)[#box[#rect(width:12pt,height:12pt,fill:blue)] Leading graphic before a long sentence that wraps over several lines.]
