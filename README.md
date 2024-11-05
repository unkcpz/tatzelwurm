# tatzelwurm

tatzelwurm is a lightweight persistent queue system to deal with process calls and broadcast operations.

- The design consideration is in [DESIGN.md](https://github.com/unkcpz/tatzelwurm/blob/main/DESIGN.md)

## Architecture

![The architecture summary of the new design](./misc/tatzelwurm-arch-tatz-arch.svg)

## How to play with the prototype

Rust is a compiled language so you can download the binaries from [Releases](https://github.com/unkcpz/tatzelwurm/releases) with your PC architecture.
Three binaries are provided for the coordinator, actioner and worker respectively.

Decompress the file and in different terminals or multiplexers, run

```bash
./tatzelwurm
```

to start the coordinator.

Run

```bash
./actionwurm task add
./actionwurm task play <id>
./actionwurm task play -a
```

to add task to table and run it.

Start the worker, multiple workers if want to test load balancing.

```bash
./workwurm
```

If you want to compile from source code, you can: 

[Install Rust](https://www.rust-lang.org/tools/install).

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

Clone the repo

```bash
git clone https://github.com/unkcpz/tatzelwurm.git && cd tatzelwurm
```

Build the project

```bash
cargo build --release
```

The binaries will be in folder `target/release/`.
Then run binaries as described above.


## Progress

- [x] Basic communication between coordinator and workers.
- [x] Basic communication between coordinator and actioner.
- [x] basic handshake to check the client type and to set the communication mode.
- [x] worker manage tasks through channels (_launch).
- [x] enum message with types for easy message transision and pattern match
- [x] in memory worker table.
- [x] in memory tasks table.
- [x] pretty print table and passing it to actioner.
- [x] mock use dummy async sleep tasks.
- [ ] mock the task pool where the task are constructed to perform.
- [x] worker manage tasks through channels (_kill).
- [ ] task pool mixed of sync/async tasks, benchmark throughput.
- [x] create -> ready state by play signal.
- [x] well CLI for register and play a single task.
- [ ] table management using actor model instead of using mutex.
- [ ] worker task dispatch internaly should also by treating every worker as an actor. 
- [ ] protocol for message transmission.
- [ ] chores: doc for all func and modules
- [x] load balancing on assigning tasks to workers. (pick least load worker)
- [ ] error handling as lib.
- [ ] task cancellation and re-assign.
- [ ] task type and priority deligate.
- [ ] lease expiration for task and re-assign (reset to ready or pause).
- [ ] broadcase message for group operations.
- [ ] rpc (message) to change the state of single running task.
- [ ] persistent store table to disk periodically for recover from reboot.
- [ ] stress test with pseudo tasks (the function re-constructed from files, can be simply async/sync sleep functions)
- [ ] pyo3 interface to expose the communicate part for python runner.
- [ ] stress test and handle the edge cases such as actors are over-loaded.
- [ ] integrating to test with plumpy.
- [ ] integrating to aiida-core.
- [ ] settle all the todos (should do this frequently when it is at proper timing)
- [ ] Polish the design note (should do this frequently when it is at proper timing)
- [ ] Move aiida specific design note (comparison with the legacy RMQ parts) to wiki for reference, and leave the generic design part in the design note.
- [ ] Take a look at hyperequeue to learn good design and Rust technology.
