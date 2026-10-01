#set page(width:600pt,height:400pt,margin:30pt)
#set text(font:"Libertinus Serif",size:18pt)
#let sample() = table(columns:(130pt,140pt),inset:8pt,fill:(x,y)=>if y==0 {rgb("dcecff")},
 [A],[B], [EDITME],[123], table.cell(colspan:2)[Merged row])
#place(dx:100pt,dy:80pt)[#rotate(20deg,reflow:true)[#sample()]]
#pagebreak()
#place(dx:100pt,dy:80pt)[#scale(x:-100%,y:100%,reflow:true)[#sample()]]
#pagebreak()
#rotate(-15deg,reflow:true)[
- Before

  #table(columns:2,[A],[B], [#table(columns:2,[X],[Y])], [After])
- Final
]

#pagebreak()
#set text(size:24pt)
$ { x mid(|) frac(a,b) > 0 } $
$ lr([a mid(|) frac(b,c) mid(|) d]) $
$ overshell(x+y,z) $
