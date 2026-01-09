# RISC-V Trace Debugger

This is a program that presents a [gdbserver](https://sourceware.org/gdb/current/onlinedocs/gdb.html/Remote-Protocol.html) interface to a debugger and pretends to be a RISC-V core that is running an ELF, while in reality it is just executing it from a trace file that you provide.

It contains a model of the registers and memory of the hart, but instead of actually executing instructions from memory, it simply applies the register and memory writes found in the trace file.

The end result is that you can take an instruction execution trace (currently only the ad-hoc Ibex format is supported), and the corresponding ELF, and then debug the trace in a real debugger (e.g. VSCode/CodeLLDB) with full access to the entire machine state, debug info, source code, locals/globals, stack frames, breakpoints, watchpoints, etc. Even reverse debugging works so you can step backwards!

Also it's written in Rust so it's automatically awesome.

## Installation

Currently you need to build from source, e.g. `cargo install --path .`.

## Use

First run something like this:

    riscv_trace_debugger --elf hello_world.elf --ibex-trace trace.log

It will print something like this:

    loading section ".vectors" into memory from [0x00100000..0x00100084]
    loading section ".text" into memory from [0x00100084..0x00100462]
    loading section ".rodata" into memory from [0x00100464..0x001004cf]
    loading section ".data" into memory from [0x001004cf..0x001004d0]
    Setting PC to 0x00100080
    Waiting for a GDB connection on "127.0.0.1:9001"...
    Debugger connected from 127.0.0.1:40352

Then configure your debugger to connect to that port. You can also use Unix sockets via the `--uds` flag. In VSCode's `launch.json` you want something like:

        {
            "type": "lldb",
            "request": "attach",
            "name": "Remote attach",
            "targetCreateCommands": ["target create ${workspaceFolder}/hello_world.elf"],
            "processCreateCommands": ["gdb-remote 127.0.0.1:9001"],
            "reverseDebugging": true,
        },

This uses the excellent [CodeLLDB](https://github.com/vadimcn/codelldb) extension. I haven't tried other VSCode debuggers, e.g. [vscode-lldb](https://github.com/llvm/vscode-lldb) or [code-debug](https://github.com/WebFreak001/code-debug) or Microsoft's one (probably doesn't support gdbserver though).

When you start that debugging session it should connect to `riscv_trace_debugger` and then you can set breakpoints, step through code, examine variables and so on.

## Bugs

There are some known bugs/issues:

1. CHERI mostly ignores tags and metadata. I'm not sure if it is possible to display capability registers in the debugger.
2. LLDB doesn't support reverse debugging properly so if you use it it switches to disassembly view unfortunately.
3. The Ibex trace doesn't tell you the size of a memory access, so any non-32-bit accesses will break things currently.
4. The Ibex trace format doesn't record traps, so you can't break on trap; instead it will just magically jump to the trap handler. You can put a breakpoint in the trap handler though.
5. No support for float or vector registers.

## Building

Set up Rust in the usual way, then I recommend building a fully static binary with Musl:

    rustup target add x86_64-unknown-linux-musl
    cargo build --release --target=x86_64-unknown-linux-musl
