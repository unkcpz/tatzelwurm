# tatzelwurm

tatzelwurm is a lightweight persistent queue system to deal with process calls and broadcast operations.

- The design consideration is in [DESIGN.md](https://github.com/unkcpz/tatzelwurm/blob/main/DESIGN.md)

## Progress

- [x] Basic communication between coordinator and workers.
- [x] Basic communication between coordinator and actioner.
- [x] basic handshake to check the client type and to set the communication mode.
- [ ] worker manage tasks through channels (_launch, _kill).
- [x] in memory worker table.
- [x] in memory mission table.
- [ ] table management using actor model instead of using mutex.
- [ ] protocol for message transmission.
- [x] load balancing on assigning missions to workers. (pick least load worker)
- [ ] mission cancellation and re-assign.
- [ ] mission type and priority deligate.
- [ ] broadcase message for group operations.
- [ ] rpc to change the state of single running process (mission).
- [ ] persistent store table to disk periodically for recover from reboot.
- [ ] stress test with pseudo missions (the function re-constructed from files, can be simply async/sync sleep functions)
- [ ] pyo3 interface to expose the communicate part for python runner.
- [ ] stress test and handle the edge cases such as actors are over-loaded.
- [ ] integrating to test with plumpy.
- [ ] integrating to aiida-core.
