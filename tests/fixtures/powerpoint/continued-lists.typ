#set page(width:600pt,height:400pt,margin:30pt)
#set text(font:"Libertinus Serif",size:18pt)
#set heading(numbering:none)
= Lists continuing inside table cells
#table(columns:(250pt,250pt),inset:10pt,
  [#list(..range(18).map(i=>[Left-#i]))],
  [#set list(marker:[$x^2$])
   #list(..range(18).map(i=>[Right-#i]))],
)
#pagebreak()
= Text and equation markers
#place(top+left,dy:55pt)[
  #set list(marker:[#text(fill:red)[A]#text(fill:blue)[B]])
  - Two-color marker
  - Editable body
]
#place(top+left,dy:125pt)[
  #set list(marker:[$x^2$])
  - Superscript marker
  - Editable body
]
#place(top+left,dy:195pt)[
  #set list(marker:[$frac(1,2)$])
  - Fraction marker
  - Editable body
]
#place(top+left,dy:270pt)[
  #set list(marker:rotate(25deg,reflow:true)[X])
  - Rotated text marker
  - Editable body
]
