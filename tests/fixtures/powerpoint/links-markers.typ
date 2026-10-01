#set page(width:600pt,height:400pt,margin:30pt)
#set text(font:"Libertinus Serif",size:18pt)
#set heading(numbering:none)
#let icon = box(image("marker.svg",width:18pt))
= Link text colors
#place(top+left,dy:55pt)[#link("https://example.com/black")[Black link]]
#place(top+left,dy:100pt)[#text(fill:rgb("#15803d"))[#link(<markers>)[Green internal link]]]
#place(top+left,dy:150pt)[#table(columns:(260pt,),rows:(42pt,),inset:8pt,
  [#text(fill:rgb("#b91c1c"))[#link("https://example.com/red")[Red cell link]]],
)]
#place(top+left,dy:220pt)[#table(columns:(260pt,),rows:(42pt,),inset:8pt,[EDIT ME])]
#pagebreak()
= Graphical and compound markers <markers>
#place(top+left,dy:55pt)[
  #set list(marker:rect(width:10pt,height:10pt,fill:red,stroke:none))
  - Square marker
  - Editable body
]
#place(top+left,dy:125pt)[
  #set list(marker:box(width:32pt)[#icon#h(2pt)#text(fill:rgb("#15803d"))[+]])
  - Image and text marker
  - Editable body
]
#place(top+left,dy:205pt)[
  #set list(marker:circle(radius:8pt,fill:rgb("#2563eb"),stroke:none)[#align(center+horizon)[#text(fill:white,size:10pt)[+]]])
  #table(columns:(340pt,),inset:10pt,[
    - Compound marker in a cell
    - Editable cell body
  ])
]
#pagebreak()
= Marker transforms
#place(top+left,dy:55pt)[
  #set list(marker:rotate(25deg,reflow:true,box(width:32pt)[#icon#h(2pt)+]))
  - Rotated compound marker
  - Editable body
]
#place(top+left,dy:145pt)[
  #set list(marker:box(width:12pt,height:9pt,clip:true)[#icon#h(2pt)+])
  - Cropped compound marker
  - Editable body
]
#place(top+left,dy:235pt)[
  #set list(marker:box(width:30pt)[#box(rect(width:8pt,height:8pt,fill:orange,stroke:none))#h(2pt)$x^2$])
  - Shape and equation marker
  - Editable body
]
