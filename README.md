# SCAN

SCAN (StatistiCal ANalyzer) is a statistical model checker
designed to verify large concurrent systems
that defy classic verification techniques.

## Developement status

SCAN is currently in developement at DIBRIS, University of Genoa (UniGe)
in the context of the [CONVINCE project](https://convince-project.eu/).
There is no released version yet.

## Formalism

SCAN uses Channel Systems (CS) as models,
and Metric Temporal Logic (MTL) as property specification language.

## Modeling specification languages

SCAN is being developed to accept models specified in multiple, rich modeling languages.
At the moment the following languages are planned or implemented:

- [x] State Charts specified in SCXML format.
- [ ] Promela
- [ ] JANI

## Build instructions

Currently, the only way to use SCAN is to build it from sources.

### Install a Rust toolchain.

The easiest and recommended way is to do so by installing [Rustup](https://rustup.rs/)
either following the instructions on its homepage or through your OS's package manager.

### Clone and compile

Clone the repo and enter its root folder.
From there, compile SCAN with

```
$ cargo build --release
```

(without the `--release` flag, the compiler will build in debug mode).

Then, run SCAN with either

```
$ cargo run --release -- --help
```

or


```
$ target/release/scan --help
```

and the CLI help will show the available functionalities and commands' syntax.
