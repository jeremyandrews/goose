# Project Goose

Goose is a Rust load testing tool, inspired by Locust.

### In progress

- [ ] web UI
- [ ] alternative HTTP clients

### Future

- [ ] website
- [ ] detect/report when available CPU power is bottleneck
- [ ] add TLS support (https://gitlab.com/neachdainn/nng-rs/-/issues/46)
- [ ] alternative non-HTTP clients
  - [ ] gRPC
- [ ] detect terminal width and adjust statistics output (when wide enough collapse into a single table, etc)
- [ ] more complicated wait_time implementations
  - [ ] constant pacing (https://github.com/locustio/locust/blob/795b5a14dd5b0991fec5a7f96f0d6491ce19e3d0/locust/wait_time.py#L30)
  - [ ] custom wait_time implementations

### Completed Column ✓

- [x] --list TaskSets and Tasks
- [x] --log-level to increase debugging verbosity to log file
- [x] --log-file to specify path and name of log file
- [x] --verbose to increase debugging verbosity to stdout
- [x] --print-stats to show statistics at end of load test
- [x] --clients to specify number of concurrent clients to simulate
- [x] --run-time to control how long load test runs
- [x] weighting of TaskSets and Tasks
- [x] spawn clients in threads
- [x] move counters into a per-request HashMap instead of a per-Task Vector (currently limited to including only one request per task for accurate statistics)
  - [x] remove per-Task atomic counters (rely instead on per-request statistics)
- [x] --reset-stats to optionally reset stats after all threads have hatched
- [x] GET request method helper
  - [x] properly identify method in stats
- [x] optionally track fine-grained per-request response codes (ie, GET /index.html: 5 200, 2 500)
- [x] provide useful statistics at end of load-test
  - [x] merge statistics from client threads into parent
  - [x] response time calculations: min, max, average, median
  - [x] show total and per-second success and fail counts
  - [x] include aggregated totals for all tasks/requests
  - [x] break down percentage of requests within listed times for all tasks/requests
  - [x] optionally provide running statistics
  - [x] only sync client threads to parent when needing to display statistics
  - [x] don't collect response time and other statistics if not displaying them
  - [x] catch ctrl-c and exit gracefully, displaying statistics if enabled
- [x] host configuration
  - [x] -H --host cli option
  - [x] host attribute
- [x] wait_time attribute, configurable pause after each Task runs
- [x] HEAD request method helper
- [x] PUT request method helper
- [x] PATCH request method helper
- [x] DELETE request method helper
- [x] TaskSequence
  - [x] add special-case 'on-start' that is always first in TaskSet
  - [x] allow weighting of tasks to always run in a given order
  - [x] add special-case 'on-stop' that is always last in TaskSet
- [x] POST request method helper
- [x] turn Goose into a library, create a loadtest by creating an app with Cargo
  - [x] compare the pros/cons of this w/ going the dynamic library approach
  - [x] upload to crates.io
- [x] optimize statistics
  - [x] round and group response times
  - [x] maintain running min/max/average
  - [x] offload to parent process as much as possible
- [x] add polling to sleeping clients to exit quicker
- [x] automated testing of Goose logic
- [x] documentation
- [x] gaggle support (distributed Geese)
  - [x] 1:n manager:worker processes
  - [x] make gaggle mode optional (adds cmake requirement)
  - [x] load test checksum, warn/err if workers are running different tests
  - [x] code cleanup, better code re-use
- [x] metaprogramming, implement goose_codegen macros to simplify goosefile creation
- [x] async clients
   - [x] use Reqwest async client
   - [x] audit code for additional places to use async
- [x] request logging
