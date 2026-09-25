# execsnoop-probe Specification

## Purpose
The eBPF equivalent of BCC `execsnoop`: new processes during the window, by command, including
the short-lived processes that `pidstat`/`top` sampling never sees.

## Requirements

### Requirement: Exec and fork counting
The execsnoop probe SHALL count `sched_process_exec` events per command name (after exec) and in
total, and `sched_process_fork` events in total, over the sampling window. It SHALL expose
metrics `execs`, `execs_per_sec`, `forks_per_sec` and `commands`.

#### Scenario: Top commands
- **WHEN** 200 execs happen in a 2-second window, 150 of `true`, 30 of `sh` and 20 of `date`, with 210 forks
- **THEN** the summary starts with `200 execs (100/s), 105 forks/s, top: true 150, sh 30, date 20`

#### Scenario: Quiet window
- **WHEN** no exec happens and 3 forks happen in 2 seconds
- **THEN** the summary is `no new processes (2 forks/s)` and the status is OK

### Requirement: Exec churn threshold
The execsnoop section SHALL report WARN when execs per second exceed 100, naming the top
command.

#### Scenario: At the threshold
- **WHEN** 200 execs happen in 2 seconds (100/s)
- **THEN** the status is OK

#### Scenario: Above the threshold
- **WHEN** 201 execs happen in 2 seconds
- **THEN** the status is WARN and the finding names the top command

### Requirement: Details
The execsnoop section SHALL list up to 5 commands by exec count as detail lines. When there are
more, a final detail line SHALL say `<N> more commands`.

#### Scenario: Many commands
- **WHEN** 8 distinct commands exec during the window
- **THEN** there are 5 command lines followed by `3 more commands`
