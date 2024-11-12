# tatzelwurm

`tatzelwurm` is a lightweight remote task broker with smart queue system and persistent task message storage.

The design considerations are collected in [DESIGN.md](https://github.com/unkcpz/tatzelwurm/blob/main/DESIGN.md)

## Architecture

![The architecture summary of the new design](./misc/tatzelwurm-arch-tatz-arch.svg)

The tatzelwurm plays two major roles: a) the queue system and b) the message broker.
The queue system is to manage which task should be run first and should be run by which worker.
The massege broker is for the coordinator need to communicate between the actioner and worker for results and operations exchange.

Two tables are stored in the disk, a) the task table b) worker table. 
By tables lookup, the coordinator can then decide which worker to run which task.

The worker has types and will only get certain type of tasks.
The goal is to distinguish the synchronous task which can block the running thread from asynchronous task that can just added to the runtime and run concurrently.

The message is passing between different entity over tcp wire, which for future design that the worker and actioner can be run on other machine.
The interface with the coordinator alone is re-interpreted and abstracted as messages.
Therefore in principle the actioner and worker can be implemented in any programming language by just follow the massege protocol.

![Running a single task](./misc/tatzelwurm-arch-UML-lifetime.svg)

The UML shows the life span of how different components coorperate to run a single task.
The coordinator has its own persistent table maintainance.
By move coordinator away from task pool, the architecture lower the access requirement to database therefore increase the performance.

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

To check the task list and filtering on specific state of tasks 

```bash
./actionwurm task list --filter <state>
```

By default, if no argument is passing, only running state tasks are listed.

Start the worker, or multiple workers if want to test load balancing.

```bash
./workwurm
```

To check the worker table and task table run

```bash
./actionwurm worker list
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

Prototype:

- [x] Basic communication between coordinator and workers.
- [x] Basic communication between coordinator and actioner.
- [x] basic handshake to check the client type and to set the communication mode.
- [x] worker manage tasks through channels (_launch).
- [x] enum message with types for easy message transision and pattern match
- [x] in memory worker table.
- [x] in memory tasks table.
- [x] pretty print table and passing it to actioner.
- [x] mock use dummy async sleep tasks.
- [x] mock the task pool where the task are constructed to perform. (#13)
- [x] worker manage tasks through channels (_kill).
- [x] task pool mixed of sync/async tasks. (#13) 
- [x] create -> ready state by the `play` signal.
- [x] sound CLI for register and play a single task.
- [ ] benchmark throughput.

Before I move to next intense development and huge refactoring, the items above server as a scaffold for playing with different detail design and hold for design feedbacks from the team.
I should polish and clear about design note and make an AEP for it first.

At the current stage, the code base is small and every part is clear defined without too much abstractions.

---------------------

- [ ] pyo3 interface to expose the communicate part for python runner.
- [ ] Adding unit tests for things above so stady to move forward.
- [ ] table management using actor model instead of using mutex.
- [ ] worker task dispatch internaly should also by treating every worker as an actor. 
- [ ] Finalize the protocol for message transmission.
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
- [ ] stress test and handle the edge cases such as actors are over-loaded.
- [ ] integrating to test with plumpy.
- [ ] integrating to aiida-core.
- [ ] Move aiida specific design note (comparison with the legacy RMQ parts) to wiki for reference, and leave the generic design part in the design note.
- [ ] Take a look at hyperequeue to learn good design and Rust technology.

### Misc

- settle all the todos (should do this frequently when it is at proper timing)
- Polish the design note (should do this frequently when it is at proper timing)
