# `rd` The Record & Debug Tool

`rd` is a Rust language port of the [mozilla/rr](https://github.com/mozilla/rr) debugger. 

The port is  _in progress_ but many things work already (see below).

## Installing

rd requires a nightly version of the rust x86_64-unknown-linux-gnu toolchain to compile.

```bash
git clone git@github.com:sidkshatriya/rd.git
cd rd
cargo install --locked --force --path .
```

Alternatively, use `--debug` like below. Things will run much more slowly as the code will be compiled with lower compiler optimizations, extra debug-mode assertions etc. 

```bash
cargo install --debug --locked --force --path .
```

In general, run in release mode as the debug mode can be much slower. Run `rd` in debug mode if you run into issues or are working on developing `rd`.

The program has been tested to compile and run properly on a 64-bit Ubuntu 20.04 installation at the moment only. 

Please file a ticket if rd does not work properly for your specific Linux distribution. In general, if `rr` compiles and runs properly in your Linux distro, `rd` should do the same.

## Running `rd`

Invoking rd without any parameters will give you help.
```bash
$ rd
```

To get help on specific rd sub-command:
```
$ rd rerun --help
```

## Credits

The `rd` project would not have been possible without the work done in the [mozilla/rr](https://github.com/mozilla/rr) project. Many human-years of development effort have gone into making `rr` the truly amazing piece of software that it is. 

The `rd` project is grateful to all the contributors of the `mozilla/rr` project.

## Background 

`rd` works on the same principles as `rr`. Please see [mozilla/rr](https://github.com/mozilla/rr) where you will find further information. More specifically, an excellent technical overview of `rr` can be found at [arXiv:1705.05937](https://arxiv.org/abs/1705.05937). 

## Contributions

Contributions to the Record and Debug Tool (`rd`) are welcome and encouraged!

By contributing to `rd` you agree to license your contributions under the MIT license without any further terms and conditions.

## What works

The port is currently in progress and not ready for end-user usage. However developers interested in contributing to this project will find there is a lot to work with and build upon. The project already contains 30k+ lines of ported over Rust code.

The port is currently capable of only replaying traces recorded previously by [mozilla/rr](https://github.com/mozilla/rr). `rd` is not yet capable of recording traces of its own but this will come in the future as the port progresses.

The following work:
* `rd rerun`
* `rd replay -a`
  * This means that interactive replay (which uses a debugger like gdb) is not yet supported 
* `rd buildid`
* `rd cpufeatures`
* `rd dump`
* `rd traceinfo`

## Tips and Suggestions

### Add an alias
After installing `rd` add an alias like this in your bash (or other shell):

Assuming you have a local source build of `mozilla/rr` at `/home/abcxyz/rr/build` 

```bash
alias rd="rd --resource-path=/home/abcxyz/rr/build
```

This will avoid constantly specifying the resource path on every `rd` invocation.

### Logging

The various logging levels are `debug`, `info`, `warn`, `info` and `fatal`. To log at `warn` by default and `debug` for all messages from the `auto_remote_syscalls` rust module (as an example) do:

```bash
$ RD_LOG=all:warn,auto_remote_syscalls:debug rd <etc params>
```

### Recording traces

`rd` cannot record its own traces at this point in time. It can, however, process traces previously recorded by `rr`. Make sure these traces are recorded with the `-n` flag (disabled syscallbuf). `rd` will support syscallbuf recordings in the future.

```bash
rr record -n <program to be recorded>
```

### _RR_TRACE environment variable

`rd` understands the `_RR_TRACE` environment variable. E.g.

```bash
$ _RR_TRACE=/the/trace/directory rd replay -a
```
