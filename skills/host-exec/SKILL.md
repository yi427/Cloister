---
name: host-exec
description: Use the authenticated Cloister Host MCP bridge to discover and run macOS tools under the Profile's allowlist or on-demand policy. Prefer Guest tools; use Host execution for macOS capabilities or a required tool missing in the Guest. Do not use for reading, writing, listing, or patching files in the selected shared Guest workspace; use Guest file tools there.
---

# Cloister Host Exec

Use only the `cloister_host` MCP tools for host execution. Treat the immutable
policy returned by the bridge as the authority for the entire bridge session.

## Choose Guest or Host

- Use Guest file tools to read, write, list, and patch files in the selected
  Guest workspace: the agent's initial working directory under `/workspace/`.
  It is a live read-write mount of the selected Host workspace, so Guest changes
  already appear in the corresponding Host project.
- Do not use `host.exec` merely to move workspace content across the Guest/Host
  boundary. In particular, do not Base64-encode a workspace file or invoke an
  allowed interpreter such as `python -c` to recreate it on the Host.
- Use Guest commands when they can complete the task. Use Host Exec only when
  the task needs macOS capabilities (such as Xcode or a macOS API) or a required
  executable missing in the Guest. Guest-first routing is behavioral guidance,
  not an enforced security boundary.
- Explain why Host execution is needed before each invocation. Keep calls
  within the user's requested task; avoid redundant probes, repeated discovery,
  and unnecessary Host shell or interpreter wrappers.
  When that executable consumes workspace files, prefer paths relative to its
  fixed Host workspace working directory.
- If a requested file is outside the selected Guest workspace, report that it
  is outside the shared workspace.
  Do not treat an allowed interpreter as a Host file API.

## Execute a host command

1. Call `host.list_commands` before the first host operation in a bridge
   session. Call it again after a reconnect or when the bridge reports that the
   session changed.
2. Read the returned DSL `version`, `mode`, fixed Host working directory, command names,
   descriptions, argument policies, environment mode, environment variable
   names, and audit status. Prefer workspace-relative paths when calling a Host
   command. The bridge uses the Host startup environment; Guest `PATH` does not
   describe Host executables.
3. Follow the mode:
   - `allowlist`: Select only a returned command name. If absent, report it as
     unavailable. Do not substitute another command, interpreter, build tool,
     or execution mechanism to bypass the policy.
   - `on-demand`: Pinned commands remain available, and other bare Host command
     names can be discovered with `host.find_command`. Read `available`,
     `executable`, and `resolved`; if unavailable, report it without attempting
     installation or another execution path. Discovery does not execute the
     program. Reuse successful results in this session. Pass the discovered
     name, never an arbitrary absolute path, to `host.exec`. No registration or
     Profile rewrite is needed. If a cached executable changes, report the
     error; restart the bridge before rediscovering.
4. Call `host.exec` with these model-supplied fields:

   ```json
   {
     "version": 1,
     "command": "allowed-name",
     "args": ["literal", "arguments"],
     "reason": "This task needs the macOS toolchain unavailable in the Guest"
   }
   ```

   Use the version reported by `host.list_commands`; the value above is only an
   example. In `on-demand` mode `reason` is required: a short purpose of at most
   240 characters, without control characters, explaining the need for Host
   execution. It appears in the tool call, but is not persisted in audit logs.
   The bridge checks its presence and shape, not whether its explanation is true.
   In `allowlist` mode it is optional. Pass every argument as a separate literal string. Do not provide an
   executable path, working directory, environment, shell command string, or
   any extra field.
5. Read the returned `execution_id`, `state`, output `chunks`, `next_cursor`,
   byte counts, truncation flag, `exit_code`, and `duration_ms`. Preserve each
   chunk's `stdout` or `stderr` stream when reporting it.
6. While `state` is `running`, call `host.exec_status` with the returned
   `execution_id`, the last `next_cursor`, and `wait_ms: 10000`. Keep only one
   status wait in flight. The bridge returns when retained output is available
   after the cursor, the execution becomes terminal, or the bounded wait
   expires. Process only chunks after the supplied cursor so output is not
   duplicated. If the returned state is still `running`, repeat with its new
   `next_cursor`; omit `wait_ms` or use `0` only when an immediate snapshot is
   needed.
7. Stop waiting at `completed`, `failed`, or `cancelled`. Treat a nonzero exit
   code in `completed` as a completed command result, not an MCP transport
   failure. Keep empty streams distinct from unavailable fields, and report
   `output_truncated = true` when the bridge could not retain all output.
8. If a running operation is no longer wanted, call `host.exec_cancel` with its
   `execution_id`, then continue status waiting until a terminal state confirms
   cleanup. Do not assume the immediate cancellation response is terminal.

## Handle approval and failure

- In `allowlist` mode Cloister requests execution approval for `host.exec`.
  In `on-demand` mode Cloister configures its Host tools for automatic approval;
  a client's own policies or user overrides may still deny or prompt.
  Let the client present any execution approval required for `host.exec`.
  State that a prompt appeared only when one was actually observed. Distinguish
  user denial, Profile denial, process-start failure, and a completed nonzero
  exit code. `host.exec_status` and `host.exec_cancel` should not require a
  second execution approval.
- If any Cloister Host MCP tool fails, report the error without using Bash, a
  terminal, another MCP server, or another execution tool as a fallback. Ask
  the user before changing to a different execution path.
- Do not call a command that is unavailable under the active mode merely to provoke a
  predictable denial. An explicit user request to test the rejection path is
  the only exception; report the denial and do not attempt a bypass.

## Preserve the security boundary

- Remember that an allowed command runs with the permissions of the macOS user
  who started Cloister. Host Exec is not contained by the guest VM.
- Treat `arguments = "any"` as broad authority. Interpreters, compilers, package
  managers, and build tools may execute additional host code.
- Treat environment mode `inherit-all` as complete inheritance from Cloister's
  trusted host environment. `host.list_commands` reveals variable names only;
  never claim that it reveals values.
- Do not use shell operators, substitutions, redirections, or quoting syntax in
  `args` expecting shell evaluation. The bridge passes the argument vector
  directly without a shell.
- Treat agent approval as an interaction policy, not the authorization
  boundary. The bridge must still validate every execution against its
  immutable policy mode and trusted Host startup environment. On-demand mode
  grants broad macOS-user execution authority, including shells, interpreters,
  and build tools. A fixed working directory does not contain their filesystem
  or network access. Discovery caching is not executable integrity enforcement.
