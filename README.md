# Ferron Forge

Ferron Forge is a compilation tool for easy compiling of Ferron web server. It outputs ZIP archives that can be used by Ferron installer.

## Usage

```
A compilation tool for easy compiling of Ferron web server

Usage: ferron-forge [OPTIONS]

Options:
  -v, --ferron-version <FERRON_VERSION>  The Ferron version or Git reference name to compile [default: main]
  -m, --modules <MODULES>                List of modules to enable
  -t, --target <TARGET>                  Target triple
  -r, --repository <REPOSITORY>          Git repository URL containing Ferron's source code [default: https://github.com/ferronweb/ferron.git]
  -o, --output <OUTPUT>                  Path to the output ZIP archive [default: ferron-custom.zip]
  -h, --help                             Print help
  -V, --version                          Print version
```

## Installation

To install Ferron Forge, run the command below:

```bash
cargo install ferron-forge --git https://github.com/ferronweb/ferron-forge.git
```

## Building Ferron using Ferron Forge

To build Ferron using Ferron Forge, run the command below:

```bash
ferron-forge
```

To build a specific version of Ferron (like 1.0.0-beta10), run the command below:

```bash
ferron-forge -v 1.0.0-beta10 # Replace "1.0.0-beta10" with desired Ferron version
```

To build Ferron with only `cache` and `rproxy` modules enabled, run the command below:

```bash
ferron-forge -m cache -m rproxy
```