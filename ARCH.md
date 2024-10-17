# Understanding of related key aiida parts

This is the doc for writing down engine related parts and how they work with each other.
It is also more or less serve as the design notes if some part of aiida can be rewrote (not Rust for sure;p)

## design note

### Process generating

`aiida-core` has one of the major role to convert from user code to the real `Process` logic to DB to be picked up by the runner.
