# Persona host

Personas are born in the log and live through this host.

A persona is a `persona_created` signal in a task's thread: the envelope's `to` names the
persona's actor id, its `description` is the charter, and whoever posted it is the creator -
which is how AN AGENT CAN CREATE AN AGENT: a persona's engine may answer with `newPersonas`,
and the host posts the birth signed by the creator, then a first hello from the newborn. The
log is the registry; the host keeps no state a restart could lose.

Personas answer only what is addressed `to` them (schema 1.1.0 addressing), reply addressed
back (`to` the asker, `replyTo` the message's signal id), and stop after `-ReplyBudget`
persona-authored messages without a human one - saying so in the thread once, never silently.
Parallel rooms need nothing special: every execution is a room, the host watches all of them,
and a subgroup is just a new task with its own chartered personas.

```powershell
./persona-host.ps1 -TokenFile <events-dir-sibling>.token
```

Deletable per CHAT_SURFACE_SPEC §7: everything it does is the public API plus `claude -p`; the
host composes prompts and posts envelopes, so an engine can never forge a signal type, an
actor, or a severity.

## Known limits, found by the board's own security persona

The first audit of this host was performed IN the system by the `seguranca` persona
(signal `persona-seguranca-re-pergunta-seguranca-1`, 2026-08-30). Its finding and status:

- **Fixed - identity collision on birth.** A persona could charter a newborn named after a real
  actor (`claude-code`, the human's session) and the host would answer as that name. Every
  actor id the log has seen is now reserved; a colliding birth is refused out loud in the
  thread, never silently.
- **Open - charter injection.** A charter written by one persona's engine becomes part of the
  next persona's prompt verbatim. The blast radius is bounded (text-only engine, host-pinned
  envelopes, reply budget), but a hostile charter can still steer the newborn's words. Treat
  charters as untrusted input when reading the thread; a lineage field in the envelope is the
  named follow-up.
- **Lineage is derivable, not stamped.** Who chartered whom is the `persona_created` signal's
  own actor - in the log, signed, but not summarized anywhere. The identity guard persona
  (`guarda-do-registo`, chartered by `seguranca` in the same exchange) exists to watch this.
