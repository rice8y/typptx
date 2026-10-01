#set page(width:600pt,height:480pt,margin:30pt)
#set text(font:"Libertinus Serif",size:18pt)
= Cell paragraph alignment
#table(columns:(240pt,240pt),rows:(65pt,90pt),inset:10pt,align:left,
  [#align(right)[Right aligned]],
  [#align(center)[Centered]],
  [#align(right)[First right]

   #align(left)[Then left]],
  table.cell(align:right)[Inherited right],
)
#pagebreak()
= Empty and hidden list markers
#place(top+left,dy:60pt)[
  #set list(marker:[])
  - Empty marker
  -
  - Same indent
]
#place(top+left,dx:270pt,dy:60pt)[
  #set list(marker:hide[•])
  - Hidden marker
  -
  - Same indent
]
#pagebreak()
= Native objects inside list bodies
+ Before #box[#rect(width:12pt,height:12pt,fill:red)] after
+ Callout below

  #block(width:260pt,inset:8pt,fill:luma(93%))[
    This callout wraps over several lines while its background stays editable.
  ]
+ Table below

  #table(columns:(120pt,140pt),inset:8pt,
    [Editable cell], [#set list(marker:[])
      - Inner item
    ],
  )
+ Last item
