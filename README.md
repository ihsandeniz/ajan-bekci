# ajan-bekci

> Latest release: [v0.1.0](https://github.com/ihsandeniz/ajan-bekci/releases/latest). Türkçe: [README.tr.md](README.tr.md).
> Bracketed links ([O2](docs/olcumler.md#o2) etc.) point to the matching section of the measurement summary.

`ajan-bekci` ("agent warden") is a small Rust program that **wraps** a terminal coding agent (Claude Code,
Codex CLI …). When the agent exits — on its own, because the terminal closed, or because it crashed — it sweeps
the processes the agent started (MCP servers, their children, dev servers detached with `setsid`). This was
measured on three closing paths with Claude Code, Codex CLI and a fake agent. [O2](docs/olcumler.md#o2)
It also ships `tara` ("scan"), which lists processes left behind by dead agents, **with an evidence level**.

```bash
ajan-bekci -- claude
ajan-bekci -- codex
ajan-bekci tara
```

## Why?

Measured without the warden (Linux, with a fake MCP server, 5 runs per cell): [O1](docs/olcumler.md#o1)

- **Claude Code 2.1.294:** on all three closing paths, 5/5 runs left behind the grandchild that the MCP server
  detached with `setsid`.
- **Codex CLI 0.161.0:** on normal exit the same grandchild was left. When the terminal closed or the agent
  crashed, **one more process** was left: the MCP server's regular child (10/10 runs).
- All leftovers (30/30) were reparented to PID 1.

> Caveat: the `setsid` grandchild is a pattern the test rig produces on purpose. How often real MCP servers do
> this was not measured. [O1](docs/olcumler.md#o1)

## Before / after (Linux)

Leak = a process still alive in the run's cage (scope) 5 s after closing. 5 runs per cell.
[O1](docs/olcumler.md#o1) · [O2](docs/olcumler.md#o2)

| Closing path | claude, no warden | `ajan-bekci -- claude` | codex, no warden | `ajan-bekci -- codex` |
|---|---|---|---|---|
| Normal exit | leak in 5/5 (1 process) | **0/5** | leak in 5/5 (1 process) | **0/5** |
| Terminal closed (SIGHUP) | leak in 5/5 (1 process) | **0/5** | leak in 5/5 (2 processes) | **0/5** |
| Agent crashed (`kill -9`)¹ | leak in 5/5 (1 process) | **0/5** | leak in 5/5 (2 processes) | **0/5** |
| `kill -9` on the warden itself | — | 1 process left in 5/5, **in the warden's scope** | — | 2 processes left in 5/5, **both in the warden's scope** |

¹ Without the warden, the terminal was also closed after `kill -9` on the agent; the effect of the crash alone was
not measured separately.

Same with the fake agent: S1-S3 0/15, the one left after `kill -9` on the warden is in the scope (1/1). `tara`
found the leftovers in the scope as K1 with the fake agent; `tara` was not run on real claude/codex leftovers.
[O2](docs/olcumler.md#o2) · [O3](docs/olcumler.md#o3)

## Install

- **Prebuilt binaries:** [GitHub Releases](https://github.com/ihsandeniz/ajan-bekci/releases/latest) — Linux x86_64
  (`ajan-bekci-<version>-x86_64-linux.tar.gz`) and Windows x86_64 (`…-x86_64-windows.zip`), with `SHA256SUMS`.
- **From source:** Needs Rust edition 2024; only built with rustc 1.96, minimum version not measured.
  [O7](docs/olcumler.md#o7)
- **AUR package:** later.

```bash
git clone https://github.com/ihsandeniz/ajan-bekci && cd ajan-bekci
cargo build --release
install -m 755 target/release/ajan-bekci ~/.local/bin/
```

On Linux, if a `systemd --user` session exists the warden moves itself into a scope; if there is no bus it runs in
plain mode. If the bus socket exists but systemd does not answer, it exits before starting the agent (not
measured). [O7](docs/olcumler.md#o7)

## Usage

Output is in English by default. If the first non-empty value of `LC_ALL` → `LC_MESSAGES` → `LANG` starts with
`tr`, it switches to Turkish;
`--lang en|tr` always overrides the environment. Option names (`--bekle`, `--kafes`, `tara`, `--temizle` …), exit
codes and `K1/K2/K3` are the same in both languages. `--json` is always English. `ajan-bekci --help` prints the
full help.

Option names are Turkish words: `--bekle` = wait, `--kafes` = cage (`yok` = none), `--grup` = group,
`--birim` = unit, `tara` = scan, `--temizle` = clean, `--kuru` = dry run, `--beyaz` = allow list,
`--sadece` = only.

### Wrapper

```bash
ajan-bekci [options] -- <command> [args…]
  --bekle SEC           grace period before SIGKILL (default 3)
  --kafes cgroup|yok    cgroup: run inside a systemd user scope when available (default)
  --grup a|b            b: agent shares the wrapper's process group (default)
  --birim NAME          scope unit name (default ajan-bekci-<pid>)
  --log FILE            append a diagnostic log to FILE (nothing is printed to the terminal)
  --lang en|tr          output language
```

- The agent starts as a child. The warden is a **subreaper** (grandchildren detached with `setsid` are reparented
  to it) and gets a signal when its own parent dies.
- **Shutdown:** when the agent exits, or on terminal close (SIGHUP) / SIGTERM, the tree is swept:
  SIGTERM → `--bekle` seconds → SIGKILL. [O2](docs/olcumler.md#o2)
- **Ctrl-C does not stop the warden;** it goes to the agent. [O2](docs/olcumler.md#o2)
- **`--kafes cgroup`:** the warden re-execs itself inside `ajan-bekci-<PID>.scope` via `systemd-run --user --scope`;
  the PID does not change. Even if the warden gets `kill -9`, leftovers stay in a named scope.
  [O2](docs/olcumler.md#o2)
- **`--grup b`:** the agent stays in the warden's process group; Ctrl-Z / `fg` stop and resume both.
  `--grup a` was measured and broke job control: after Ctrl-Z the shell did not come back. [O2](docs/olcumler.md#o2)
- The warden never writes to the agent's terminal.
- The agent gets an `AJAN_BEKCI_PID` environment variable; `tara` uses it as K2 evidence.
- **Exit code:** the agent's (128+n if killed by a signal) · 2 usage error · 127 could not start · 1 no cage
  (also 1 if the agent's code was unavailable).

### `tara` — list leftovers

```bash
ajan-bekci tara [--json]
```

**Sends no signal to any process.** Exit 0 = no K1/K2, 3 = some. [O3](docs/olcumler.md#o3)

```
$ ajan-bekci tara
    PID  LEVEL AGENT    AGE      RSS  COMMAND · detail
   4242  K1    codex     3m     12MB  node mcp-server.js · in ajan-bekci-4100.scope, no guard alive
   4250  K2    claude    1h      8MB  sleep 600 · CLAUDE_PID=4100 dead
   4300  K3    ?         2d     40MB  node x · orphan · claude trace, mcp in name
K1 certain 1 · K2 strong 1 · K3 suspect 1 (K3 is never cleaned) · no process was signalled
```

(example output; PIDs are illustrative)

| Level | Meaning |
|---|---|
| **K1 certain** | The process's cgroup is `ajan-bekci-*.scope` and no live warden is in that scope. Caught even with a wiped environment |
| **K2 strong** | Its environment has `CLAUDE_PID` or `AJAN_BEKCI_PID`, and that PID is dead, a zombie, or reused |
| **K3 hint** | Orphan (parent is PID 1 or `systemd --user`) + an agent trace. **Never cleaned** |

- Only the values of `CLAUDE_PID`, `AJAN_BEKCI_PID`, `AJAN_BEKCI_KAFES` are read from the environment; nothing
  else is stored. [O3](docs/olcumler.md#o3)
- On a real machine (7 live Claude sessions, 53 legitimate orphans): **0 false alarms** in K1/K2.
  [O3](docs/olcumler.md#o3)
- Fake leftover scenario: 10/10 found, in 10 repetitions. [O3](docs/olcumler.md#o3)

### `tara --temizle` — the safety belt

```bash
ajan-bekci tara --temizle --kuru                 # list only first (no signals)
ajan-bekci tara --temizle --sadece 4242,4250     # only the PIDs you approved
```

```
DRY LIST — 2 targets (K1/K2 only, uid 1000) · allow list /home/u/.config/ajan-bekci/beyaz-liste (missing — empty)
TARGET 4242 K1 node mcp-server.js · in ajan-bekci-4100.scope, no guard alive
TARGET 4250 K2 sleep 600 · CLAUDE_PID=4100 dead
OUTSIDE 4260 in a live agent tree
--kuru: no process was signalled
```

A real run prints `STOPPED <pid> SIGTERM|SIGKILL`, `SKIPPED <pid> <reason>` or `LEFT <pid>` for each target.

- **The dry list is always printed first;** without `--kuru` only that list is applied. [O4](docs/olcumler.md#o4)
- **Only K1/K2 and only your own user.** Never K3. [O4](docs/olcumler.md#o4)
- **Re-verification right before every signal:** is the evidence still valid; are uid + start time + exe the same
  as when listed. Anything that does not match is skipped. Once more before SIGKILL. [O4](docs/olcumler.md#o4)
- **If a PID is reused, it is not touched** (tested by simulation at two points). [O4](docs/olcumler.md#o4)
- **The tree of a live `claude` / `codex` / `ajan-bekci` can never enter the plan.** [O4](docs/olcumler.md#o4)
- **Allow list:** `$XDG_CONFIG_HOME/ajan-bekci/beyaz-liste` (else `~/.config/…`). Line format `exe <full path>`
  or `komut <substring>`, `#` comments. If any line is malformed, nothing is done.
- In the fake scenario: dry list = real cleanup 6/6, 0 wrong kills, in 10 repetitions. Breaking the guard lines one
  by one (7 mutations) was caught 7/7 by the tests; the second evidence check before SIGKILL was not mutated
  separately. [O4](docs/olcumler.md#o4)
- **Exit:** 0 = all stopped or empty plan · 4 = something skipped or left · 2 = usage.

> ⚠️ A server that a dead agent session **intentionally** backgrounded (`nohup … &`) shows up as K2.
> Read the dry list before cleaning; put such processes on the allow list. [O4](docs/olcumler.md#o4)

## Windows (measured in CI)

**Not tried on a real Windows machine.** Measurements were done on GitHub `windows-latest` (Windows Server) CI,
nothing more. Some items exist only in code and unit tests; they are marked. [O5](docs/olcumler.md#o5)

- **Cage:** the wrapper uses a **Job Object** (KILL_ON_JOB_CLOSE; the child starts suspended and is put in the job).
- **Behaviour (no CI scenario):**
  - Ctrl-C / Ctrl-Break are ignored; window close, logoff or shutdown count as shutdown (code + unit test).
  - The console is inherited by the agent; the parent is watched (code only).
- **`TerminateProcess` on the warden → 0 grandchildren left.** Nested job → 0. The exit code is passed through. The
  Linux "leftovers after `kill -9` on the warden" limit does not exist on Windows, because the kernel closes the job.
- **CI:** green in 3 attempts; Windows tests 15/15, smoke 36/36.
- **`tara`:** Windows v0.1 lists **K3 only** (dead parent + agent-named process). No K2 and no `--temizle` on Windows.
- `--kafes`, `--grup`, `--birim` have no effect on Windows.
- **Not tested:** real Ctrl-C, an interactive console, real claude/codex.

## Limits

- **`tara` does not see leftovers of a codex started without the warden as K1/K2**; codex leaves no PID-carrying
  environment marker. It lists the MCP's tool child only as a K3 suspect. The setsid grandchild in the measurement
  matches none of the K3 criteria either (whether its environment carries a codex trace was not measured). So `--temizle` does not touch them. Leftovers of agents started with
  the warden are K1 certain. [O3](docs/olcumler.md#o3)
- **Whether an unwrapped claude's MCP carries `CLAUDE_PID` was not measured;** so it is unknown whether leftovers of
  an unwrapped claude show up as K2. [O3](docs/olcumler.md#o3)
- **On Linux, if the warden itself gets `kill -9`,** nobody is left to sweep. Leftovers wait in the scope; `tara`
  found this case as K1 with the fake agent. [O2](docs/olcumler.md#o2)
- "Live agent" detection is by process or exe name (`claude`, `codex`, `ajan-bekci`). [O4](docs/olcumler.md#o4)
- **Not measured:**
  - plain mode on a machine without systemd
  - the everyday terminal chain, and Ctrl-Z in it
  - how often real MCP servers spawn detached children
  - leftovers of the agent's own tool shells
  - multi-user machines

  [O7](docs/olcumler.md#o7)

## Development

```bash
cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings
```

66 tests on Linux; clippy `-D warnings` is clean for Linux and `x86_64-pc-windows-gnu`. Windows tests run only in
CI. Every test that kills processes signals only the tree it started or its own scope; no name matching
(`pkill`/`killall`). [O6](docs/olcumler.md#o6)

The core (`crates/cekirdek`) was copied from the same author's process-warden project; details are in `KAYNAK.md`.

## License

MIT or Apache-2.0, at your option: [LICENSE-MIT](LICENSE-MIT) · [LICENSE-APACHE](LICENSE-APACHE).
