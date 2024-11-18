# Introduction

SCAN (StatistiCal ANalyzer) is a statistical model checker
designed to verify large concurrent systems
for which standard verification techniques do not scale.

It features:
- SCXML as input language (support for other languages is underway)
- Past MTL as property specification language
- State/event dense time traces

SCAN is currently under developement at DIBRIS, University of Genoa
in the context of the [CONVINCE project](https://convince-project.eu/).

While SCAN can also be as a standalone model checker,
it closely integrates with the other tools in the CONVINCE toolchain.
In particular, AS2FM can target the SCXML format accepted by SCAN,
to make verification of robotic systems more accessible to robotic systems developers.

<a href="crates/scan/index.html">API docs for the library crates.</a>
