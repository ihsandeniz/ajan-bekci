# Measurements

The source of every number in the README. Sections: [O1](#o1) baseline without the warden · [O2](#o2) with the
warden on Linux · [O3](#o3) `tara` · [O4](#o4) `tara --temizle` · [O5](#o5) Windows (CI) · [O6](#o6) tests ·
[O7](#o7) not measured. Türkçe: [olcumler.tr.md](olcumler.tr.md).

Date: 2026-10-11. Machine: single-user Linux desktop with a systemd user session. This summary is distilled from the
raw measurement records; process IDs, paths and machine-specific details were removed, only the numbers remain.

## Test rig (shared by O1 and O2)

- **Fake MCP server:** a minimal stdio MCP server (`initialize` / `tools/list`). On start it spawns two children:
  a regular "tool" process and a "dev server" (grandchild) that leaves the group and session via `setsid`.
- **The agent is started with this server,** no prompt is sent, then it is closed by one of the paths below.
- **Every run in its own cage:** a separate `systemd-run --user --scope` unit, a separate home directory, a separate
  tmux server. Counting and cleanup use only that unit's process list; no matching processes by name.
- **Leak** = a process still alive in the run's cage (and in the warden's cage, if any) 5 s after closing.
- **Closing paths:**

| Code | Path | How |
|---|---|---|
| S1 | normal exit | `/exit` (claude) · `/quit` (codex); the agent is left to exit on its own |
| S2 | terminal closed | the tmux server is killed (SIGHUP) |
| S3 | agent crashed | `kill -9` on the agent itself. Without the warden the tmux server is then killed too (S2's SIGHUP is added); with the warden the pane closes on its own together with the warden |
| S4 | warden killed | `kill -9` on the warden itself (O2 only) |

- 5 runs per cell. Versions: Claude Code 2.1.294, Codex CLI 0.161.0.
- The real agents were started with credentials given only to the measurement cage. The Claude runs with the warden
  used a separate token created only for this measurement.

<a id="o1"></a>
## O1 — Baseline without the warden

| Path | Claude Code | Codex CLI |
|---|---|---|
| S1 normal exit | leak in 5/5 runs · 1 process (setsid grandchild) | leak in 5/5 · 1 process (setsid grandchild) |
| S2 terminal closed | leak in 5/5 · 1 process (setsid grandchild) | leak in 5/5 · **2 processes** (setsid grandchild + the MCP's tool child) |
| S3 agent crashed | leak in 5/5 · 1 process (setsid grandchild) | leak in 5/5 · **2 processes** (the same two) |

- **30/30 runs started and were counted.** Every leftover was reparented to PID 1.
- **The MCP server itself** died in 30/30 runs (it exits when its stdin closes).
- **Codex's extra leak:** the MCP's tool child. Its fields: process group = the MCP server, session = the agent's
  session. Reading (inference): codex starts the MCP in a separate process group. It cleans that group up on normal
  exit, but not when the terminal closes (S2) or on crash + terminal close (S3). The effect of a crash alone,
  without the terminal closing, was not measured separately.
- **Repeat:** Claude without the warden, S2, was run 5 more times with a separate token. The result was again
  leak in 5/5 · 1 process (setsid grandchild), consistent with the first measurement.
- **Caveat:** the `setsid` grandchild is a pattern the rig produces on purpose. How often real MCP servers do this
  was not measured.

<a id="o2"></a>
## O2 — With the warden (Linux, `--grup b`, `--kafes cgroup`)

| Path | `ajan-bekci -- claude` | `ajan-bekci -- codex` | `ajan-bekci -- <fake agent>` |
|---|---|---|---|
| S1 normal exit | **0/5** · warden exited on its own 5/5 | **0/5** · warden exited on its own 5/5 | 0/5 |
| S2 terminal closed | **0/5** | **0/5** | 0/5 |
| S3 agent crashed | **0/5** · warden swept and exited 5/5 | **0/5** · warden swept and exited 5/5 | 0/5 |
| S4 `kill -9` on the warden | 1 process left in 5/5 (setsid grandchild), **1/1 in the warden's scope** | 2 processes left in 5/5, **2/2 in the warden's scope** | 1 process in 5/5, 1/1 in the scope |

- **S1-S3 total:** claude 0/15 · codex 0/15 · fake 0/15. Without the warden, the same paths leaked in claude 15/15
  and codex 15/15 runs ([O1](#o1)).
- **S4:** once the warden gets SIGKILL nobody is left to sweep; the parent-death signal does not reach
  grandchildren. Leftovers stay in a named scope. `tara` found this case as K1 with the fake agent (`kill -9` on a
  real warden) ([O3](#o3)); `tara` was not run on real claude/codex leftovers.
- **Shutdown and cage (exercised by tests):** the tree is swept SIGTERM → `--bekle` s (default 3) → SIGKILL.
  With `--kafes cgroup` the warden re-execs itself via `systemd-run --user --scope`; the PID does not change.
  Falling back to plain mode without a systemd bus is design only: no test, not measured ([O7](#o7)).
- **Cage check:** in 40/40 codex + fake runs the warden, the agent and the MCP were in the warden's scope. In the
  Claude runs 25/25 started (agent + MCP + tool child + setsid grandchild seen in every run).
- **Interaction smoke test (claude, 5 items × 2 group modes):** 5/5. Items: start, text on screen, Ctrl-C closed
  neither the agent nor the warden, window 160→100 columns with the agent redrawing, `/exit` → warden exit code 0
  and 0 leaks.
- **Choosing the group mode:**
  - `--grup a`: Ctrl-Z stopped only the agent, the warden kept waiting and the shell did not come back. Neither
    `fg` nor Ctrl-C helped.
  - `--grup b`: Ctrl-Z stopped both, `fg` resumed both, Ctrl-C exited both cleanly.
  - Hence the default is `b`. Measured inside a real `bash -i` with a plain `sleep` as the agent.

<a id="o3"></a>
## O3 — `tara` (report only)

- **Dry run on a real machine:**
  - Of 53 legitimate processes under PID 1 and the user systemd (terminal, tmux server, sound server, shells …),
    **0** showed up in K1/K2.
  - Of the grandchildren of 7 live Claude sessions, 0 showed up.
- **Fake leftover scenario:** 10 of 10 findings were found (4 K1, 6 K2), 10/10 in 10 separate runs. The 4 controls
  did not show up: a live `CLAUDE_PID` carrier, two processes in a live warden's scope, an unmarked process.
- **Exit and signals:** exit 0 = no K1/K2, 3 = some. `tara` sends no signal to any process (no `kill` call in the
  module).
- **Mutation:** verification lines were deleted one by one (live-warden scope filter, start-time comparison, live
  target, uid filter). The tests caught 4/4.
- **Environment reads:** only the values of `CLAUDE_PID`, `AJAN_BEKCI_PID`, `AJAN_BEKCI_KAFES` are kept. A unit test
  checks this.
- **Leftovers of agents started without the warden:**
  - Leftovers of an unwrapped codex do not show up as K1/K2, because codex leaves no PID-carrying environment
    marker.
  - The tool-child pattern ("group leader and session leader dead") is listed as K3; the unit test uses the O1 shape.
  - The setsid grandchild in the measurement (`sleep`, its own group and session leader) matches none of K3's name
    or group criteria. It would enter K3 only if its environment carried a codex trace (`CODEX_*`); whether it
    does was not measured.
  - Whether an unwrapped claude's MCP carries `CLAUDE_PID` was not measured.
- **Leftovers of agents started with the warden:** in the fake scenario, leftovers after `kill -9` on a real warden
  were found as K1 in 10 repetitions. `tara` was not run on real claude/codex S4 leftovers (O2).

<a id="o4"></a>
## O4 — `tara --temizle`

- **Flow:** the dry list is always printed first; without `--kuru` only that plan is applied, and anything that
  changed in between is skipped.
- **Fake scenario:** 6 targets (3 K2, 1 K2 that ignores SIGTERM and was closed by SIGKILL, 2 K1). Dry list = real
  cleanup 6/6, 10/10 in 10 repetitions.
- **0 wrong kills.** The 4 controls survived every repetition: one on the allow-list, one in a live warden's tree,
  one outside `--sadece`, and an unmarked one.
- **PID reuse** was simulated at two points with a fake system:
  - start time changed after listing → no signal;
  - PID reused after SIGTERM → no SIGKILL.
- **Mutation 7/7:** start time, exe, uid, evidence before SIGTERM, live agent tree, allow-list, K3 exclusion. The
  second evidence check before SIGKILL was not mutated separately; it is identical to the one before SIGTERM.
- **On a real machine only `--kuru` was run: 0 targets.**
- **Known:** a server that a dead session intentionally backgrounded shows up as K2; the only protection is the
  allow-list. Live-agent detection is by process or exe name.

<a id="o5"></a>
## O5 — Windows (CI only)

**There is no real Windows machine.** All results are from the GitHub Actions `windows-latest` (Windows Server)
image.

| Attempt | Linux | Windows | Windows tests | Smoke (4 scenarios × 3 rounds) |
|---|---|---|---|---|
| 1 | ✅ | ✅ | 5/5 | 12/12 |
| 2 | ✅ | ✅ | 5/5 | 12/12 |
| 3 | ✅ | ✅ | 5/5 | 12/12 |

- **Total:** tests 15/15, smoke 36/36.
- **Fake agent:** written for `node`. It spawns an MCP-like grandchild and its child, a `start /b` grandchild and a
  `detached` grandchild (5 processes).
- **`TerminateProcess` on the warden → 0 left.** Nested job (outer warden closed) → 0 left.
- **Exit code pass-through:** `cmd /c exit 7` → 7, `node process.exit(3)` → 3, missing command → 127, nested code 9
  passed through both levels.
- **`tara` v0.1:** an orphaned `node` showed up as K3 and was still alive after `tara`. `--temizle` → exit 1.
- **No CI scenario:** ignoring Ctrl-C/Ctrl-Break is code + unit test only (console event classification);
  console inheritance and watching the parent handle are code only (no test).
- **Not tested:** real Ctrl-C, an interactive console, real claude/codex, a home Windows 11 desktop (Defender, a
  different console host).

<a id="o6"></a>
## O6 — Tests and static checks

- 66 tests on Linux (`cargo test --workspace`; including the bilingual output tests).
- `cargo clippy --workspace --all-targets -- -D warnings` clean on Linux and for `x86_64-pc-windows-gnu`.
- Windows tests (`tests/windows.rs`, `olcum/windows/duman.ps1`) run only in CI.
- Every test that kills processes signals only the tree it started or its own scope. Tests use marked
  `sleep <number>` processes; no name matching.

<a id="o7"></a>
## O7 — Not measured

- How often real MCP servers spawn detached children.
- Leftovers of the agent's own tool shells (Bash etc. run by prompts); no prompt was sent during measurement.
- The everyday terminal chain (terminal → shell → `ajan-bekci -- claude`) and real Ctrl-Z/`fg` in it.
- Plain mode on a machine without systemd. If the systemd bus exists but does not answer, the warden exits before
  starting the agent; this was not measured either.
- Multi-user machines.
- Real Windows; minimum Rust version (only built with rustc 1.96).
