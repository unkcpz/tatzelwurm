# How to kill the Rabbit

The initial goal of this tool is to replace the use of RabbitMQ as message broker in [AiiDA](https://aiida.net).
The tool however can be more generic as a lightweight task broker that handle the task distribution and task state persistent store with tasks are workflows orchestrated by different programming languages.

> ![Note] 
> - "worker" and "runner" are used interchangeably.
> - most of time "coordinator" is the "server".
> - sometimes the "process" and "mission" is used interchangeably.

### When we say replace RMQ what we are talking about?

In addition to the [AEP PR#30: Remove the dependence on RabbitMQ](https://github.com/aiidateam/AEP/pull/30)

We are talking about remove `kiwipy` (therefore also a hidden package `pytray` for convert back and forth the sync <-> corountine) from `plumpy` as dependencies. 
The `plumpy` is the real internal engine on top of which the `aiida-core` extent the `Process` to be `ProcessFunction`, `CalcJob` and `WorkChain`. 
Meanwhile, the `plumpy` also define the base interfaces on how to persistent store the `Process` state so that can be recovered from checkpoints. 
The `plumpy` need `kiwipy` to provide interface to talk to RMQ to manage the process running on the specific event loop of a worker's interpretor (the event loop is then in `aiida-core` managed inside daemon runner). 

The connections between `plumpy` and `kiwipy` is actually not too many as I originally thought, the `plumpy` treat `kiwipy` as a so called "Communicator".
The most important interface is `add_task_subscriber` which register a function (coroutine) that upon consuming message from RMQ the worker side decide how to deal with the `Process`. 
Two parts require this `add_task_subscriber` interface.

- The `Process` can be `launch` or `continue` by the `ProcessLauncher` registered with its `__call__` (queue up tasks). 
- The `Process` itself when initilized, will registered the `message_reciever` to listen to queue when there are action signals to `play`, `pause` or `kill` the `Process`.

The reason why we should not rely on 3rd-part message broker is not only that the extra service is hard to deploy.
Those message broker is design for large distribute system runs for microservices and usually involve with a lot of machines. 
In AiiDA case, the common scenario is that there are not too many clients (workers/actioners) talk to server (coordinator), thus it requires a very neat implementation that can add more bussiness logic around but still keep it lightweight. 

#### Legacy architecture

It is worth to have a look at the legacy architecture when using RabbitMQ + kiwipy as message broker system for task control.
The figure shows how a task from generated to it is running at the worker. 

![The architecture summary of the legacy task launch and controlling system design](./misc/tatzelwurm-arch-legacy-arch.svg)

The user has access to the "actioner" to control the state of a task.
The actioner in AiiDA context is the CLI or the API to communicate with the backend when a task state requires to be changed.
Typically, here is how a task from it is generated to it is being run at "worker".
Firstly, user create an AiiDA "Process" (`ProcessFunction`, `CalcJob` or `WorkChain`), in the context here we call it a "task".
The task object is store into the task pool.
At the same time, a message is send through the message broker and taken by a worker that there is a task to be proceeded. 
Finally, the worker construct the task from task pool and run it.

The task consists of its state and how it will involve over the time, so when it just created the task is in its initial state.
The real entity of the task is stored in the task pool, which in the AiiDA context it is the persistent data storage i.e database + disk-objectstore.
For the task pool, the task can be deserialized and run by the "worker". (althrough the task pool has all the information to recover a task to run, but it contains the borrowed reference to the environment or source plugin instance, this will be further described in the other post hopefully).
To let the worker run a task after submitting the task, a message is passing around at the same time through the communicator, which is what I am going to improve by replace the backend design with `tatzelwurm`.
Two message management patterns are used which are [publisher-subscriber pattern](https://learn.microsoft.com/en-us/azure/architecture/patterns/publisher-subscriber) (?in the context of RMQ it is a work queues?) and [remote procedure call (RPC)/broadcast](https://www.rabbitmq.com/tutorials/tutorial-six-python).

The pub/sub pattern is applied between actioner and multiple workers. 
The actioner is the publisher that will send the message about which task should be run.
The workers are the subscribers that take the message and then process on the task. 
**However**, here comes the problem with pub/sub pattern. 
After the publisher send a message to the subscriber, it expect an acknowledge from subscriber to tell that the task is taken and is proceeded. 
If the message not passing to the subscriber, the publisher has the responsibility to send the message again.
The task in AiiDA can take hours and even days to run to its terminated state and then the acknowledgement message will be sent to the publisher. 
The RabbitMQ is designed for the ligth message and the default timeout for delivery acknowledgement of RMQ is 30 mins.
The timeout is not recommended to be changed.

The RPC and broadcast patterns can be described together. 
They are applied between actioner and the tasks instance directly. 
The different between RPC and broadcast is that RPC is 1-to-1 while broadcast is in 1-to-N mode. 
It is not a mistake to use RPC in the AiiDA scenario but there is [a note on RPC](https://www.rabbitmq.com/tutorials/tutorial-six-python#a-note-on-rpc) in the RabbitMQ documentation that "when in doubt avoid RPC. If you can, you should use an asynchronous pipeline".
I extend the discussion on secion [_Is RPC/broadcast really needed in AiiDA?_](#is-rpc-and-broadcast-really-needed).
From the architecture figuer, it is interesting to see that the direct interaction between actioner and task acrossing the worker is a bit awkward.
If skip all the handling of edge cases when the worker is dead.
You'll find that in the new design, I try to give/ask worker for more responsibility to handle the cancellaition and notification back to the actioner. 

## New architecture

The new design is sketched in the figure below, where I replace "communicator" to "coordinator" by giving more explicit role.

![The architecture summary of the new design](./misc/tatzelwurm-arch-tatz-arch.svg)

- The RMQ as message broker is replaced with the tatzelwurm.
- The tatzelwurm play two major roles: queue system and message broker.
- Two tables stored in the disk. Two tables lookup to decide which worker to run which task.
- The worker has type and will only get certain type of tasks. 
- The interface is re-interpreted and abstructed as messages.
- The kiwipy is the interface for actioner and worker on bundle the operations to talk to RMQ, this part is not yet have the decision on design, see section [xxx] for more information. 

### Design note

The note is this section futher extend on the details of the new architecture.

The related design pattern is:
- [pub/sub](https://learn.microsoft.com/en-us/azure/architecture/patterns/publisher-subscriber).
- [actor with tokio](https://ryhl.io/blog/actors-with-tokio/)

Not all but quite a bit are inspired by:

- https://github.com/chrisjsewell/aiida-process-coordinator
- https://github.com/chrisjsewell/aiida-process-coordinator/discussions/4
- https://github.com/chrisjsewell/aiida-process-coordinator/issues/7#issuecomment-943361982
- The AEP mentioned above.

#### The components

1. Task coordinator

The coordinator plays two major roles. 
It is a message broker that communicate with the worker connected and it is a queue system that knows when to send which process to run on which worker (load balencing).

It is in order to replace RMQ for queuing the tasks and persistent the task list on the disk. 
Additional to using RMQ which lack of API to introspect task queues to determine the task list and task priorities.
Using rust is just for the edging performance that can potentially handle millions of processes in the foreseeable future. 

- It requires a server (task coordinator) runnig and waiting for messages to send to workers, the workers are client that runs on a python interpreter with an event loop.
- The server will listen for incoming tasks, manage worker connections (handshake and monitoring the heartbeat of the worker), and handle the load balencing of tasks to workers.
- There are cases the workers will closed without finishing its tasks, when this happened coordinator need to know the state change of workers and resend the unfinished tasks to another worker. 
- task coordinator implement persistent task queuing so that tasks can be recovered after a machine restarts. The go to solution I can see is using an embeded DB like RocksDB or use Redis with its RDB+AOL. 
- Persistent (continue of the point above): the embedeb DB only allowed one connection and this connection is from the coordinator (therefore, only the coordinator is directly communicate to the embedeb DB), it communicating with the worker and when worker respond, it update tasks (e.g. `pending`, `in_progress`, `completed`) into the serialized task data. (More details are discussed in later sections.) 

2. Worker

The worker is responsible for running python functions (or more generic if the worker backend is in Rust, I can build on top the wrapper to more different languages).
In the context of plumpy, it is for running python coroutines by push the coroutines to the event loop that bind to the worker interpretor. 
There are two ways of implementing the worker, one is having the worker as client and implemented in python, the harder way is implement worker in rust and expose the `add_task_subscriber` to python using `pyo3`.
(? May possible to further seperate define the functionalitios of worker. It needs to communicate and running things. The communicate port is de/serializing the data and this port can definitly use the API exposed through `pyo3` here.)
Having worker implemented in rust has the advantage that for future design to having multi-threading support or having specific threads for CPU-bound blocking functions the rust implementation is close to the low-lever threads management and has no limitation with GIL.
However, the worker anyway require a wrapper layer between rust and python, which potentially make the debugging more difficult if really something goes wrong. 
For simplicity, the worker should first implemented in python as a `Worker` class that can strat with event loop and has `add_task_subscriber` method to push coroutines to its event loop. 

- Workers communicate with the rust server, getting messages from server and trigger the subscribed functions/coroutines to run on its own thread.
- Each worker runs an event loop in Python, which waits for messages from the rust server. 
- Upon receiving a message, the worker loads and executes the task. It should acknowledge back to coordinator that it get the task and keep on update the state of task running so the coordinator knows its loading. 
- The first time when the worker start, it need to register to the coordinator a long-live task subscriber that responsible for launching the task and continuing the task. This subscriber will be removed with closing the worker or the coordinator is gone (TBD, not yet clear whether coordinator need to booking the state of subscribers)
- Inside the worker, `add_task_subscriber` creates a task handler and the handler (coroutien) is scheduled to the event loop (this is the difficult part, I am not sure it is even possible. Because it seems require async imlementation that not block on waiting for the coroutine to finish but just push the coroutine to the worker event loop. May need async in rust worker?). 
- Each real task is proceeded individually (corresponding to the `Process` in plumpy), and once run to comleted, the task's state is marked as complete and remove from coordinator's booking. 
- only one worker working on a task a time, automatic requeueing of tasks if the worker died for whatever reason

3. Actioner

There is always a guy look into system from outside and try to make some action. 
It is us the regular aiida user who is willing to launch, pause, resume, and kill the process.
I call this role the actioner. 
The actioner will send the operation through message to coordinator and coordinator then send (or broadcast) the message to workers and take actions to manipulate the processes.

#### Key Advantages of Rust Over Python

In the initial goal of language design, rust was putting [fearless concurrency](https://blog.rust-lang.org/2015/04/10/Fearless-Concurrency.html) in its core.
People from AiiDA team worked on engine were very suffered from debugging the `asyncio` more or less.

Rust bring memory safety without GC.
Memory issues are a major pain point in high-throughput systems, and Rust offers a unique advantage with its zero-cost abstractions for memory safety. 
Rust’s borrow checker and ownership model prevent common errors like null pointer dereferencing, dangling pointers, and data races—all of which Python’s garbage collection cannot handle efficiently. 
Rust eliminates these risks at compile-time, resulting in a more reliable and crash-resistant service.

Rust's Concurrency scales with modern Multi-Core Systems
With Rust, we gain access to high-performance asynchronous programming through frameworks like `Tokio`, which is highly optimized for I/O-bound tasks. 
Python’s GIL (Global Interpreter Lock) inherently limits true parallel execution in most cases, leading to bottlenecks that severely hinder performance as load increases. 
Rust’s async capabilities allow for parallelism without these limitations, ideal for network-intensive applications like our communication component.

Rust has great error handling and stability
Rust’s strict compile-time checks lead to a higher degree of confidence in error handling and code stability. 
Rust doesn’t just allow developers to skip error handling, unlike Python where errors can bubble up silently, resulting in unhandled exceptions and potential system crashes. 
By enforcing rigorous handling of potential failures, and ensures that our component will be robust, resilient, and dependable, even under heavy load.

I can image people from the team may against it because they may not at the moment very familiar with Rust and Rust is well-known for its steep learning curve.
My opinion is, in order to write a package from scratch maybe difficult and requires a lot effort to learn to make mistakes.
But read, understand and make further change should be easier with the clear design and the powerful asynchronous weapons from Tokio.
It is like writing AiiDA from scratch is hard, but we can all understand and contribute to it.

#### Two tables

The coordinator should grab two tables to operate on its mission.
The two tables are workers table which record the workers state information and the tasks table which record the states of tasks.

Everytime when the coordinator "look at" two tables, it needs to make the decision on what operation needs to be take.

#### Task states transition

The task has its states and the state will be changed upon signal received from actioner/worker/assigner.
The task in `tatzelwurm` has its own states transition logic and may different from states definition from its real entity in task pool (e.g. in AiiDA).
In `tatzelwurm` the state is for managing the state transition by operations applied and is a marker to know if the operation valid to take.

- `Created`: ( x -> `Ready`, x -> `Pause`) when the task entity is push to task pool and its existence signal registered to the coordinator. In this state, the assigner will **NOT** trigger this task to run.
- `Ready`: (`Created` -> x, x -> `Submit`, x -> `Pause`, `Pause` -> x) is the only state that will be picked by the assigner and send to worker to be proceeded.
- `Submit`: (`Ready` -> x, x -> `Run`, `x` -> `Pause`) after assigner send it to a worker and worker not yet start working on it.
- `Pause`: (`Created` -> x, `Ready` -> x, `Submit` -> x, `Run` -> x) is a buffer and fallback state that hold the assigner to assign it to worker. It waits for a signal from actioner to resume. If the task is in `Run` state and the worker lost heartbeat, the coordinator will transite it to `Pause`.
- `Run`: (`Submit` -> x) the worker is working on it.
- `Terminated(exit_code)` (`Run` -> x) the worker finish on it and send the signal back. The exit_code 0 for complete, -1 for killed and other positive number for except with certain exit code.

#### Message types

In the whole design, the message is used to communicate between different entities. 
I want to have to different type of messages for two purposes:

- On the one hand, for message passing between clients and server over tcp stream and require serialize and deserialize.
- On the other hand, for in-processing communications only the message is passing through channels and can contain oneshot channel for ack which can not be serialized.

For the tcp communication over wires, the message will be send and will triggle a bundle of operations on two side.
I call this message type `ExMessage`, where "Ex" for "external".
This type of message is already contain abstraction of operation and become interfaces for communication between clients and the coordinator.

For the in-processing communication that use shared memory over mpsc or oneshot channels, the message contains the atomic operation on for example manipulating worker and task table.
I call this message type `IMessage`, where "I" for "internal".

```rust
#[derive(Serialize, Deserialize, Debug)]
pub enum XMessage {
    // dummy type for fallback general unknown type messages
    BulkMessage(String),

    // The Uuid is the task uuid
    // coordinator -> worker
    TaskLaunch(Uuid), 

    // hand shake message when the msg content is a string
    // <-> between server and clients
    HandShake(String),

    // Heartbeat with the port as identifier
    // <-> between server and clients
    HeartBeat(u16),

    // Notify to coordinator that worker changes state of task
    TaskStateChange{id: Uuid, from: TaskState, to: TaskState},
}

#[derive(Debug)]
pub enum IMessage {
    // dummy type for fallback general unknown type messages
    BulkMessage(String),

    // The Uuid is the task uuid
    // dispatcher using worker's tx handler -> worker's rx, after table lookup
    TaskLaunch(Uuid), 

    // Operation act on worker table
    WorkerTableOp {
        op: TableOp,
        id: Uuid,
    },

    // Operation act on worker table
    TaskTableOp {
        op: TableOp,
        id: Uuid,
        from: TaskState,
        to: TaskState,
    },
}
```

There is a typical case that one bundled operation relys on both.
When dispatch a task to worker by two table lookup, an internal message `IMessage::TaskLaunch(Uuid)` first fired from a dispatch which runs concurrently.
The message is relayed to worker client by parsing the message and convert it to an external message with type `XMessage::TaskLaunch(Uuid)`, which send to the correspond worker.

#### Proactive mission assignment to workers

The major different between the legacy design and the new design will be the way how missions assigned to worker.

- legacy design: worker take from queue channel.
- new: coordinator hold a booking (with mutex so no racing). In every tcp stream with worker the coordinator look at booking and push mission to worker. 

In the new design, the coordinator not only just fan-out the mission and wait it to be consumed when new mission created. 
It also waits for the response that the mission is taken and wait for the message that the mission is finished. 
The coordinator needs to frequently look at the booking and decide which to do with each worker. 
But what is the driven force for coordinator to look at the booking? 
Everytime the coordinator look at the booking it has some cost, thus too often booking check will bring overhead. 
On the contrary, less often booking check will cause the missions not efficiently assigned and therefore jams the pipeline.
To ensure the good rate of throughput and avoid leaving workers to unnessesary idle, the booking checking is happened in the following case:

- when it is certain that the state of table is changed such as new process opened and process terminated.
- in the certain interval it ask itself to chek the booking and keep things ongoing.

It requires to benchmark the performance of booking check on a very big table and decide which should be the proper default interval for booking check.

Design diverge (TBD): there can be two ways to assign missions:

- directly send to worker, which seems easier to implement but a bit couple for the stream.
- send to the channel subscribed by the worker and let worker to consume. The channel decouple the stream so maybe a bit more flexible.

#### Task priorities

The goals of this design are:

- workers are not blocked by the slots limit instead it can have a max number of tasks and even it set to `1` things will continue and it can be the way to limit the resources usage. 
- the workchains start running again once the child process they were waiting for were reach the terminated state (Seb's CIF cleaning).
- in a regular load (?definition required after implementation and bench) only one worker required for the non-blocking processes, and number of worker controller is for blocking running workers only.

The task (`Process`) sending to the coordinator need to have a priority field to distinguish from its type and high priority tasks should run before the worker start to consume the low priority task.
The purpose in the context of AiiDA is that the workchain itself is not a runnable process but the processes it encapsulates such as `CalcJob` and `ProcessFunction` are runnable.
The workchain can be nested, therefore the inner workchain should have higher priority then the outer workchain to be run first. 

In the design, the processes have different categories which fall in to different topic when the subscriber need to pick up to run.
The fundamental tasks e.g. `CalcJob` and `ProcessFunction` that not rely on the completion of other process are in the topic 'baseproc-a' (`ProcessFunction`) and 'baseproc-b' (`CalcJob`), and they have the same running priority `0`.
The reason to distinguish the base runnable processes is for future improvement, where we can have dedicated interperetor to run blocking CPU-bound processes. 
For the workchain the process is bind with topic 'compproc', it should have a stack of calling order to know if it is called from another workchain.
The most outside workchain is given the priority value `1` and the value incremented when it goes deeper to ensure that inner process will be picked by the coordinator to run on worker first, that is to say the large the priority number the ealier it is picked up to be run.

There is still chances that the blocking process will starving the workers but it required to be solved if we can separate dedicated threads/runners for such tasks and having regular workers only to push running event loop forward. 
Based on the topic, it can have dedicated worker that only to consuming 'baseproc-a' topic for running `ProcessFunction`.
While for the `CalcJob` and `WorkChain` they can all push to be run in the same async runtime.

#### Persistent queue state to disk

The queue or aka processes state record booking should be able to be persistently stored in the disk between the restart session.
In order to maximize the performance for high-throughput, the running processes booking is in memory. 
For persistent, when the coordinator shutdown, the current booking is write to the file as booking database (BDB) for recover the booking of next restart.
However, there is possibility that the coordinator may not gracefully shutdown. 
As a backup solution of such scenario, the append only file (AOF) is keeping the logs of every booking change and can be used to reconstruct full booking before shutdown.
These two mechanism are exclusively used for the newly restart of coordinator. 
If the coordinator is gracefully shutdown, BDB is written for recovering the next coordinator session and the AOF file will be cleaned up and log events will only for recording the new running session.

The cons of this design is the size AOF can grown fast in the high-throughput, and if the coordinator keep on running of long time (in the scale of monthes, the AOF can be extremely large and expensive to rerun events to recover the booking).
The enhancement can be the BDB is dumped to the disk from memory periodicly and remove the events happened until the dump period so the AOF only contain the events for recovering from dumped BDB.

#### When and what to communicate between coordinator and workers

The [comment](https://github.com/aiidateam/AEP/pull/30#discussion_r766481043) enphasize that run a task zero or one time makes more sense for AiiDA since resounces consumed by AiiDA is always a concern in terms of running heavy calculation on the remote.
In order to make task completion information more verbose so the coordinator can make good decision on whether send the task to other workers, the worker need to communicate two times back to coordinator, one when the worker get the task assigned, it reply back it will running on it, then when the task is complete it need to send back again to say the task is real done.
The coordinator will keep the list of running tasks and ensure no duplicate tasks are send to other workers. 
The AiiDA has its DB that also hold the process completion information, and this information will be synchronize with coordinator whenever coordinator restart or manually triggered when needed. 
When the tasks is really completed, it is safe to be removed from the running tasks list and coordinator can assign new to workers.
This strategy is different from but better than the current RMQ solution, where when a process submitted, a request to run the process is sent to RMQ which will then send the task to a worker (worker get the message and put it in its event loop). To guarantee that each task will be executed, RMQ will wait for the worker to confirm the task is done or not. 
If the task is not done, RMQ will send the task to another worker if the previous worker was detected to miss the heartbeat and regarded as "died worker". 
Two times communication release the problem that coordinator need to keep task on the queue until it finished.

Therefore for worker, it has following communication scenario:

- Handshake: for security purpose, talk to coordinator to show it is a valid worker. Bidirectional communication.
- Heartbeat: monodirectional communication from worker to coordinator. It requires write half from stream.
- Mission assigned: worker get a mission ID from coordinator, reply when it create the mission from ID (push to its own event loop). Bidirectional communication.
- Mission complete: worker finish the mission, send an ack to coordinator so coordinator can make the booking and assign more missions to worker. 

As corresponding, the coordinator needs to:

- Handshake: get to know new worker is willing to connect, authorize it. Bidirectional communication.
- Heartbeat: monodirection from worker, and coordinator is only read from stream to check the worker is alive.
- Assign mission: send mission Id to the lowest load worker, and change the mission state to assigning. When worker reply with on processing, coordinator update state to "running".
- Mission complete: worker will send a message when mission complete, the coordinator need then update the process state to complete (delete from list).

TBD: whether coordinator needs to keep heartbeat and notify workers it is alive to update the booking?

#### Data frame and serialization

- **Decision**: framed data (if rust, using `codec`, if python using `asyncio` stream). 
- **Decision**: de/serialize message, using `MessagePack` for extensibility and simplicity.

Since I need to take care of bytes over wire with async, I need to transfer framed data.
I decide to use `tokio_util::codec` to frame the data.
For the protocol, instead of design a new one, using `MessagePack` and wrap struct into it can be easy to start.
The MessagePack is like JSON but a binary message which can be more efficient in transportation.
If in future we find the message between clients and coordinator is always simple, we can change to using self designed protocol such as redis-like protocol.
If we find in the future we need more big chunk of data to communicate, the MessagePack can fit for storing more complicate format.

#### How to deal with missions when worker is "dead"

Related isuses: https://github.com/aiidateam/aiida-core/issues/5278

- (TBD) **Decision**: implement the lease for simplicity which requires only logic in the communication level.

The worker keep on sending heartbeat, when it missed one, coordinator can regard it as "dead".
Then there is a problem that the worker may still hold the access to the remote resources and to the database. 
Unexpected thing may happen if the mission is re-assigned to other worker at the same time.

This is not a uncommon scenario in the distribution system design.
I searched on the internet and it gives following solutions that we can keep on investigating and comparing:

- leases: the coordinator can only send the mission to other worker when the original worker miss the heartbeat and the lease timeout expired.
- task cancellation: when heartbeat missed, coordinator send cancellation to worker and worker need to gracefully cancel all running missions. 
- quorum-based consensus protocol: (mentioned by ChatGPT, I don't understand how it works.)
- Idempotency and atomic on resource manipulating: if the operations on resources (remote HPC or database) are idempotency, then it doesn't matter if the operations happen multiple times. But in AiiDA this is not guranteed especially for the remote calculations which may have two `sbatch` submit in the same folder (maybe I am wrong?). For the DB, the issue of duplicate output port as mentioned is exactly what happened.
- tag and version on the operations: if the operation is not idempotency, then if same resource is manipulated by the different workers then giving version or tag to make sure it is not duplicately running same operation can be a solution, but this require the tag feature in the DB and in the transport plugin. 

#### Is rpc (and broadcast) really needed?

- **Decision**: don't use rpc but asynchronous pipeline and let the worker manage processes.** 

In legacy (current) `aiida-core`, during the lifetime of a mission (an aiida process), it has a rpc channel attached. 
The rpc channel has worker side as server to respose the rpc calls from actioner which is the user's CLI.
The only function registered on the rpc is the `message_reciever` which answer to three remote operations i.e. `kill`, `pause` and `play`.
The rpc is a common pattern when differnt components require to interact with each other in an asynchronous manner, but misuse may lead to unclear code.

In `aiida-core` use case, this rpc pattern is overkill.
Instead, what we really need is a listener of the process that knows what operation to take for the process, depend on the message received. 
This listener can skip runner and interact with coordinator (as legacy design where every process has a rpc subscriber to RMQ).
This listener can interact with worker and let the worker to deal with communication with coordinator. 
But is it possible to add more hierachy the communication logic?

I think the better solution is having runner in between the process and coordinator to avoid coordinator have direct control of processes.

In the POC of `aiida-process-coordinator`, it uses a dict to handle the signal to the specified process.
It is possible (and I think it is better) to use channel for runner to manage sending signal to operate on the process.
By using signal it futher decouple the runner and processes.

The new design has pros and cons:
- Pros 1: processes are grouped under runner which makes runner management (restart and cancellation) more robust.
- Pros 2: coordinator get less load on monitoring processes and therefore can handle larger throughput. 
- Cons 1: the runner has a bit more responsibility then before.
- Cons 2: the broadcast should go runner by runner. 

Let's look at the process kill operation as a typical example that reveal the new design is better.
In legacy design, the kill signal directly send to process and process register the operation to the runner's event loop to be executed.
The control flow goes from coordinator to deepest process and back to runner and then to process.
In the new design, the kill signal send to runner, and the runner then manage the close of the process. 
The control flow goes from coordinator -> runner -> process which is easy to reasoning and every two parts can be isolated to debug.

#### Is pub/sub suitable for process launch and ack?

(comming)

- no, it is the source of RMQ 15mins limit.
- use `mpsc` to get message from runner, use `oneshot` to send terminated signals (finish/killed) back.

#### One glimpse multiple tasks assignment

When dispatch tasks to workers, there are two ways of updating worker table.
First is after every task assignment, I check the worker table again to get the latest one and to assign task using the new table.
Second is to do only one glimpse on the worker table and use this table to assign all the tasks in a loop.
The different between two strategies are during the tasks assignement, the worker table will be update in-time.
The worker table can also changed if there are short running tasks that finish during assignments. 

The decision was made mostly for less table lookup and to make the assignment more easy to predict.
For debug purpose, it is easy to just print out two tables and see if the assignment works as expected.

#### Info to task to limit the maximum number to run on remote

A very [old issue](https://github.com/aiidateam/aiida-core/issues/88) was not able to be solved because the lack of process assignment strategy.
At the moment, there is no way to count the number of tasks are running on the remote resources. 
When assigning the remote run task (`CalcJob` in AiiDA context) to worker, it requires to add a check on the limit amout of jobs are able to run on the remote.
If such type of tasks exceed the limit, not task should be assigned to the worker until some tasks are completed later.

#### Gracefully kill a task shutdown runner

- [Issue #2985 not gracefully killed](https://github.com/aiidateam/aiida-core/issues/2985)
- [Issue when only CMD interpreter runner closed](https://github.com/aiidateam/aiida-core/issues/2711)

When task is killed, it requires to

1. Go through all the sub-process and schedule cancelling coroutines.
1. Change the DB record for the task (by aiida-core).
1. When all cancelling finished, close the event loop and mark the process as killed in the table.

### Experiments required before start

These are collection and short summary of awswers from ChatGPT 4o, which give the hints for tools and technique stacks where I should look and clear the path before start.
I don't fully trust so the experimentals required.

#### Can I run a python coroutine in the thread that spawned in the rust side?

ChatGPT points that:

1. Python's GIL must be acquired before interacting with Python objects from Rust. (first prepares the Python interpretor by `pyo3::prepare_freethreaded_python()` and then `Python::with_gil()`)
2. Set up an asyncio event loop. This requires to run inside the thread `asyncio.get_event_loop` and in the loop call `run_until_complet` of the coroutine from the task handler.

Comments: 

- The key point to check is make sure not new thread is created for each coroutine but the thread/event_loop has the lifetime of worker (this is the new design after move from `tornado` to `asyncio` which can only support one event loop, so probably with handle thread by rust every process can again run on their own event loop??). 
- It may also be possible (better) to turn the python coroutine into a rust future and then using tokio runtime to poll the future to complete. This require the crate `pyo3_asyncio` as bridge to pass python's coroutine to rust tokio runtime. 

#### The interface for worker and actioner

(comming soon)

- worker and actioner are independent of central design, since the interface can be very flexible as messages and therefore make the use of the system programming language agnostic.
- But it anyway requires real implementation for the actioner and worker.
- although the create provide example of worker and actioner by rust, in its first real world use case in AiiDA, the python interface is must to have thing.
- The interface is the kiwipy as for the RMQ. 
- In the repo, the interface is put as separate crates and provide with python as must have, and probably with julia and lua in plan.

### TBD

- Q: Is it better to store checkpoint in a seperate (in legacy it is with process node) table or even in a separate resource? 
    - A (@unkcpz): The checkpoint contains two types of information, which are the information to recover the task and the information of task running state. The info to recover the task from task pool should be in the task pool. But the information about running state should be in the other entity with more fast access. These two information are well decoupled.
    - But this change require more design change in the AiiDA.

- Q: Who should create (create means initialize the instance, store it is the "DB" and set to the created state) the task? Coordinator or worker or actioner (in legacy it is actioner)? 
    - A (@unkcpz): It should still be the actioner, the architecture overview explain it well.
    - One of the goal is to make the use of the task broker language agnostic and fit for the workflow of orchestrated in different programming languages.

### Performance tips

If the performance become bottleneck and after the benchmark it shows the the bottleneck is not from architecture and implementation.
There are some underlined crates and tools to use as the alternative for message passing or DB management.

- [Rkyv](https://github.com/rkyv/rkyv) for zero-copy deserialization as alternative to msgpack.
- [dashmap](https://github.com/xacrimon/dashmap) as alternative of HashMap if in-memory store with mutex map is used.
