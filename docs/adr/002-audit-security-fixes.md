# ADR-002: Security Audit and Hardening

**Date:** 2025-07-14
**Status:** Accepted
**Author:** Security audit (Copilot)

## Context

convergio-delegation orchestrates remote code execution via SSH and rsync.
Shell commands are built with `format!()` string interpolation, creating
potential command injection vectors even though current inputs come from
trusted sources (UUIDs, config files, environment variables).

## Findings

| Category | Severity | Finding |
|----------|----------|---------|
| Command injection | Medium | Shell commands in `pipeline.rs` and `remote_spawn.rs` interpolate paths and SSH targets via `format!()` without validation |
| Input validation | Medium | No validation on `peer` name from HTTP request body |
| Thread blocking | Low | `std::thread::sleep` in async context blocks tokio runtime |
| Unused config | Low | `exclude_patterns` defined but not used in rsync stub |
| README | Info | Placeholder "CRATE_DESCRIPTION" not replaced |

**Not affected:** SQL injection (parameterized queries), path traversal (paths
validated post-fix), unsafe blocks (none), secret exposure (none), SSRF (peer
lookup via config file), auth bypass (enforced at daemon level via ring model).

## Decision

1. Add `validate_peer_name()` — allowlist of `[a-zA-Z0-9_.-]`, max 64 chars
2. Add `validate_shell_path()` — reject shell metacharacters `;|&$\`(){}><!\n\r\0'"`
3. Validate all inputs at route handlers AND at pipeline entry points (defense in depth)
4. Replace `std::thread::sleep` with `tokio::time::sleep`
5. Use `exclude_patterns` in rsync stub commands
6. Fix README placeholder

## Consequences

- Shell injection is now blocked even if a future code path introduces untrusted input
- Async runtime no longer blocked during daemon restart wait
- Rsync respects configured exclusions (target, node_modules, .worktrees)
