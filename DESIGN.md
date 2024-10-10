# Design

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

### Design note

The related design pattern is:
- [pub/sub](https://learn.microsoft.com/en-us/azure/architecture/patterns/publisher-subscriber).

Not all but quite a lot are taken from:

- https://github.com/chrisjsewell/aiida-process-coordinator
- https://github.com/chrisjsewell/aiida-process-coordinator/discussions/4
- https://github.com/chrisjsewell/aiida-process-coordinator/issues/7#issuecomment-943361982
- The AEP mentioned above.

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
The actioner will send the operation through message to coordinator and coordinator then send (or broadcase) the message to workers.

#### Two tables

The coordinator should grab two tables to operate on its mission.
The two tables are workers table which record the workers state information and the tasks table which record the states of tasks.

Everytime when the coordinator "look at" two tables, it needs to make the decision on what operation needs to be take.

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

- when new mission is filed and coordinator aware of it.
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

#### Data frame and serialization

- **Decision**: framed data (if rust, using `codec`, if python using `asyncio` stream). 
- **Decision**: de/serialize message, using `MessagePack` for extensibility and simplicity.

Since I need to take care of bytes over wire with async, I need to transfer framed data.
I decide to use `tokio_util::codec` to frame the data.
For the protocol, instead of design a new one, using `MessagePack` and wrap struct into it can be easy to start.
The MessagePack is like JSON but a binary message which can be more efficient in transportation.
If in future we find the message between clients and coordinator is always simple, we can change to using self designed protocol such as redis-like protocol.
If we find in the future we need more big chunk of data to communicate, the MessagePack can fit for storing more complicate format.

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

