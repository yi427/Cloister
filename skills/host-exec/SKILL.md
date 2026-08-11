---
name: host-exec
description: Use the authenticated Cloister Host MCP bridge to discover and run Profile-allowed commands on the macOS host from a Cloister guest. Use when a task needs an available host-native toolchain, build command, or other executable exposed through `cloister_host`, and when reporting Host Exec approvals, denials, output, or policy is required. Do not use for reading, writing, listing, or patching files in the shared Guest `/workspace`; use Guest file tools there.
---

# Cloister Host Exec

Use only the `cloister_host` MCP tools for host execution. Treat the immutable
Profile returned by the bridge as the authority for the entire bridge session.

## Choose Guest or Host

- Use Guest file tools to read, write, list, and patch files under `/workspace`.
  It is a live read-write mount of the selected Host workspace, so Guest changes
  already appear in the corresponding Host project.
- Do not use `host.exec` merely to move workspace content across the Guest/Host
  boundary. In particular, do not Base64-encode a workspace file or invoke an
  allowed interpreter such as `python -c` to recreate it on the Host.
- Use Host Exec only when the task requires a Profile-allowed macOS executable.
  When that executable consumes workspace files, prefer paths relative to its
  fixed Host workspace working directory.
- If a requested file is outside `/workspace`, report that it is outside the
  shared workspace. Do not treat an allowed interpreter as a Host file API.

## Execute a host command

1. Call `host.list_commands` before the first host operation in a bridge
   session. Call it again after a reconnect or when the bridge reports that the
   session changed.
2. Read the returned DSL `version`, command names, descriptions, argument
   policies, environment mode, environment variable names, and audit status.
   Never infer an executable from `PATH` or from a command that is absent from
   this response.
3. Select only a returned command name. If the requested command is absent,
   report that it is unavailable. Do not substitute another command,
   interpreter, build tool, or execution mechanism to bypass the policy.
4. Call `host.exec` with exactly these model-supplied fields:

   ```json
   {
     "version": 1,
     "command": "allowed-name",
     "args": ["literal", "arguments"]
   }
   ```

   Use the version reported by `host.list_commands`; the value above is only an
   example. Pass every argument as a separate literal string. Do not provide an
   executable path, working directory, environment, shell command string, or
   any extra field.
5. Read the returned `execution_id`, `state`, output `chunks`, `next_cursor`,
   byte counts, truncation flag, `exit_code`, and `duration_ms`. Preserve each
   chunk's `stdout` or `stderr` stream when reporting it.
6. While `state` is `running`, call `host.exec_status` with the returned
   `execution_id` and the last `next_cursor`. Use bounded backoff between polls:
   start near 250 ms and cap at 1 second. Process only chunks after the supplied
   cursor so output is not duplicated.
7. Stop polling at `completed`, `failed`, or `cancelled`. Treat a nonzero exit
   code in `completed` as a completed command result, not an MCP transport
   failure. Keep empty streams distinct from unavailable fields, and report
   `output_truncated = true` when the bridge could not retain all output.
8. If a running operation is no longer wanted, call `host.exec_cancel` with its
   `execution_id`, then continue status polling until a terminal state confirms
   cleanup. Do not assume the immediate cancellation response is terminal.

## Handle approval and failure

- Let the client present any execution approval required for `host.exec`.
  State that a prompt appeared only when one was actually observed. Distinguish
  user denial, Profile denial, process-start failure, and a completed nonzero
  exit code. `host.exec_status` and `host.exec_cancel` should not require a
  second execution approval.
- If any Cloister Host MCP tool fails, report the error without using Bash, a
  terminal, another MCP server, or another execution tool as a fallback. Ask
  the user before changing to a different execution path.
- Do not call a command that is absent from discovery merely to provoke a
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
  immutable Profile snapshot.
