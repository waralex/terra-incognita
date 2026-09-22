---
name: terra-checkpoint
description: Save the useful context of the current conversation into Terra before the user manually clears it. Update reusable project knowledge and a stable session checkpoint; never clear the conversation yourself.
argument-hint: "[existing session block ID]"
disable-model-invocation: true
---

# Save before clear

Use the connected terra-memory MCP. If unavailable, stop without claiming a checkpoint exists. This operation saves context; it does not clear, compact, restart or end the chat.

## Place and identity

Read the project root at default depth. Use its direct Section child named `Сессии` (or an existing equivalent `Sessions`). Create it with `write(parent: root, markdown: "# Сессии", reason: ...)` only if absent. Do not search the whole project for an arbitrary section with that name. If several candidates exist, ask which one to use.

Each conversation has one Section under this container, with a short descriptive title. Its block ID is the durable session ID. Prefer the ID supplied by the user or already established in this conversation (including by terra-resume). Verify its parent before updating. Do not infer session identity from a matching title or choose the most recent session from another chat. With no known ID, create a new session; titles need not be unique.

Root reads show only the `Сессии` heading. Keep checkpoint content inside the session, never directly on the project root. This is bounded navigation, not secrecy: deep reads and search can still find checkpoints.

## Distill and save

1. Review the available conversation. Save reusable discoveries in the appropriate project sections using the terra-memory workflow; search narrowly before duplicating a topic. Distinguish observations, user decisions and unverified ideas. Keep temporary hypotheses in the session checkpoint; promote them to reusable project knowledge only when they are useful beyond this task, with their evidence and scope explicit. Do not invent missing earlier context, save credentials, dump transcripts or copy large code/logs. Do not overwrite unrelated concurrent work.
2. Record the minimum state a fresh agent needs: goal; explicit user constraints and pending approvals; completed work and evidence; current conclusion and its reason; unresolved questions or failed approaches worth avoiding; repository/worktree, branch, HEAD commit and working-tree state; next action; links to durable knowledge. Keep temporary state out of reusable project knowledge. Use read-only Git checks where applicable; distinguish what was checked now from remembered information. Identify the revision and dirty state covered by saved test results, or explicitly say they are unknown. Preserve the scope and conditions of each constraint: “commit after review” must not become “never commit”. Separate task constraints from instructions for a temporary test or evaluation; do not carry test-only restrictions into the resumed task. An old permission is context, not blanket authorization for new actions.
3. Write the checkpoint **last**, after successful project-memory updates. Use one Text block inside the session, so replacing it is one atomic write with one expected revision. Format it as one Markdown paragraph with soft line breaks and bold labels, no blank lines, headings, lists or fences. Usually 200–500 words suffice; explicit constraints matter more than an arbitrary size target. This is a compact handoff, not private chain-of-thought.
4. First save: write `# Descriptive session title`, a blank line, then the checkpoint paragraph under `Сессии`; retain the returned Section and Text IDs. `write.blocks` is an array of ID strings (`blocks[0]`, not `blocks[0].id`); `written` contains metadata objects. After a partial failure, reread and reuse successful creations instead of restarting the workflow or duplicating sections. Later saves: read the existing session and compare its Text revision with the one retained at the last resume/save. If it changed, reconcile the intervening checkpoint before replacing it; a freshly read revision is not permission to discard another chat’s work. If no baseline revision is known, explicitly reconcile the stored content against this conversation. Replace only that Text via `write(target, expected, markdown, reason)`. Preserve the IDs and let Terra retain history. If there is no unique checkpoint Text or it has unexpected structure, inspect rather than replace a container. A conflict requires rereading and reconciling; never silently retry with a fresh revision.
5. Read the session back with sufficient depth. Check for truncation, required constraints, next action and valid memory links. Verify `written` contains the expected structure. If saving or verification fails, report that it is not ready for clearing. Do not remove the previous checkpoint to make a replacement.

## Receipt for the user

Briefly list what project knowledge changed and what the checkpoint contains. Provide the session ID, its live and pinned links from the verified read, and the exact restore command `/terra-resume SESSION_ID`. Mention any omissions. Only after verification say it is saved and ready for the user's manual `/clear`. Never execute that command. Keep the session ID and verified checkpoint Text revision for subsequent saves in this conversation.
