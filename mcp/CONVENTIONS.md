# Memory store conventions

Conventions for using terra as Claude's cross-session work memory. The
entity model is open (no enforced types), so consistency is a matter of
discipline — this file is that discipline. The schema-enforced part lives
in `schema.yaml`.

## What the store is for

Claims Claude accumulates about the user's work — across all projects,
across sessions. The rule of thumb: **store what is not already in the
code or git history.** Don't mirror the repo; remember what the repo
can't tell you — who the user is, why things are the way they are, what's
been decided, what's still unknown, what to do next.

## Scope: everything in `main`, no project branches

One shared `main` branch holds all projects. Scope is expressed by slug
namespace and a `project` property, not by branching. This keeps
cross-project links alive (`cube` ↔ `cubestore`) and shared facts always
current — a project branch would freeze its view of `main` at fork time
(no merge/pull exists) and couldn't see sibling projects.

## Branches = worktree-scale exploration

A terra branch maps to a unit of in-progress work (a git worktree /
feature branch), not to a project. Branches are cheap (no data copy).

```
main  (durable, cross-project memory)
 └─ checkout <git-branch-name>      ← fork for the duration of the work
      • observations / hypotheses while working
      • inherits main as of fork time
   work lands → distil durable conclusions back into main (status: fact)
   branch stays as a trace of how we got there
```

Two write lanes; the agent chooses per write:

| Lane | Typical status | When |
|---|---|---|
| worktree branch | `observation` / `hypothesis` | default while working |
| `main` | `fact` | deliberately committing to shared truth |

**Default: when in doubt, write to the branch.** Cost asymmetry — a claim
wrongly left on a branch is cheaply re-asserted into `main` later; a claim
wrongly written to `main` is append-only (can't be deleted, only
superseded) and is immediately inherited as noise by every other worktree.

`status` (epistemic) and write lane (visibility) are orthogonal — don't
conflate them. An `observation` read straight from `main`'s code can go to
`main`; a confident `fact` inside an experiment can stay on the branch.

## Entities

### Slug namespace

Dotted, hierarchical, grep-friendly:

- `cube`, `cubestore`, `terra` — projects
- `cube.sql-api`, `cube.cubestore.rollout` — components (stop at
  subsystem level; don't go down to files/functions — the code already
  documents itself)
- `person.alex`, `person.<name>` — people
- `infra.<name>` — clusters, dashboards, services

Project boundary = `entities.grep ^cube\.` (or filter on the `project`
property).

### `kind` property

Every entity carries a `kind` for filtering:

| kind | examples |
|---|---|
| `project` | cube, cubestore, terra |
| `component` | rollout pipeline, SQL API, scheduler |
| `person` | the user, teammates |
| `service` | clusters, dashboards, infra |
| `concept` | domain terms, glossary entries |

### Standard properties

Free-form, but prefer these names for consistency:

- `kind` — see above
- `project` — owning project slug (fast scope filter)
- `description` — one-line what-it-is (also the entity's native description)
- `repo`, `language`, `owner` — for projects/components
- `role`, `team` — for people

### Relations

Relations are property assertions whose value is another entity's slug:

- `part_of` — component → project (`cube.sql-api` → `cube`)
- `depends_on` — cross-component / cross-project dependency
- `owned_by` — entity → person

## Managed records

See `schema.yaml` for fields and lifecycles. Boundaries between types:

| Type | Holds | Intent |
|---|---|---|
| `convention` | how the user likes things done | preference / style |
| `decision` | a choice made + why (ADR) | committed choice |
| `open_question` | a known unknown | something to **learn** |
| `task` | something to do (`horizon: next \| someday`) | something to **do** |

- `task` vs `open_question`: do vs learn. A question often spawns a task.
- `task.horizon`: `next` = committed next action; `someday` = would-be-good
  backlog. Promotion someday → next is just a field change; both stay
  `open` until done.
- `convention` vs `decision`: a convention is a standing preference ("uses
  X style"); a decision is a point-in-time choice ("chose X over Y on
  date, because Z").

## Provenance

Every entity-change carries `source` (where the knowledge came from) and
`reasoning` (why it's believed). `source` distinguishes authoritative,
durable knowledge (`user`) from knowledge that can go stale
(`code:<path>`, `inference`).

Both `reasoning` and `source` surface per-property on read (in `TxMeta` /
`context`). `source` must be declared in `entity_change_meta` — the validator
rejects undeclared meta fields (`UnexpectedField`).
