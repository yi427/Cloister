---
name: host-exec
description: Use the authenticated Cloister Host MCP bridge to discover and run Profile-allowed commands on the macOS host from a Cloister guest. Use when a task needs an available host-native toolchain, build command, interpreter, or other executable exposed through `cloister_host`, and when reporting Host Exec approvals, denials, output, or policy is required.
---

# Cloister Host Exec

Use only the `cloister_host` MCP tools for host execution. Treat the immutable
Profile returned by the bridge as the authority for the entire bridge session.

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
5. Wait for the synchronous result. Report `stdout`, `stderr`, `exit_code`, and
   `duration_ms` accurately. Keep empty streams distinct from unavailable
   fields. Treat a nonzero exit code as a completed command result, not as an
   MCP transport failure.

## Handle approval and failure

- Let the client present any execution approval required for `host.exec`.
  State that a prompt appeared only when one was actually observed. Distinguish
  user denial, Profile denial, process-start failure, and a completed nonzero
  exit code.
- If `host.list_commands` or `host.exec` fails, report the error without using
  Bash, a terminal, another MCP server, or another execution tool as a fallback.
  Ask the user before changing to a different execution path.
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
