// https://mdwm.org/diatypst/reference.html
#import "@preview/diatypst:0.9.3": *
#import "../slide-content.typ": lists, details
#show: slides.with(title: "Structure", first-slide: false, toc: false,
  ratio: 16/9, count: "number", footer-title: "Diatypst footer")
#set text(font: "Libertinus Serif")
== Native structure
#lists
== Native details
#details
