// https://mdwm.org/diatypst/reference.html#count
#import "@preview/diatypst:0.9.3": *
#show: slides.with(
  title: "Navigation title", subtitle: "Navigation subtitle",
  authors: ("Author One", "Author Two"), date: "2026-01-01",
  ratio: 4/3, count: "dot-section", toc: true,
  theme: sys.inputs.at("theme", default: "normal"),
  footer-title: "Navigation footer", footer-subtitle: "Footer right",
)
#set text(font: "Libertinus Serif")
= Section Alpha
== Destination Alpha <destination-alpha>
Alpha body marker
= Section Beta
== Destination Beta
Beta body marker
#link(<destination-alpha>)[Return to Alpha]
