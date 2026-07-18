# L4 M3c/d/e structured-log contract (CR-L4-004)

> **Status**: Active (opt-in L4 library / shadow host).  
> **Scope**: structured **logs only** — no Prometheus / Grafana entry gate.  
> **Code**: `agent::candidate_dag::m3_metrics` + emitters in `run_candidate_or_fallback` / `maybe_run_candidate_shadow`.

## Events

| Event target | When |
|--------------|------|
| `candidate_dag_run` | Candidate DAG run succeeds without L2 fallback (**M3c** pass) |
| `candidate_dag_fallback` | L4→L2 fallback taken (**M3e**); may carry schema **M3d** category |
| `candidate_dag_schema_fail` | Schema/graph validation failed and fallback was **disabled** (**M3d** only) |
| `candidate_dag_shadow_run` | Opt-in shadow host observe (CR-L4-003); includes the same M3 fields |

## Fields

| Field | Type | Meaning |
|-------|------|---------|
| `m3c_pass` | bool | `true` when candidate path succeeded without L2 fallback |
| `m3d_category` | string | Fail/pass category (see below); pass path uses `ok` |
| `m3e_fallback` | bool | `true` when L4→L2 fallback ran |
| `fallback_reason` | string | Short reason (`schema_fail`, `run_abort`, or reject message) |
| `dag_id` | string | DAG id of the run that completed (candidate or fallback template) |
| `m2_steps` | u64/usize | Template shell step count (M2) |
| `shadow` | bool | Present on shadow observe events |

## M3d categories (stable)

| Category | Meaning |
|----------|---------|
| `ok` | Schema + graph checks passed |
| `parse_error` | Not JSON / not an object |
| `schema_validation` | Generic schema/parse failure beyond dedicated buckets |
| `unknown_capability` | Capability tag outside allowlist |
| `forbidden_source` | Illegal `source` shape |
| `graph_integrity` | entry/next graph errors |

These strings match `CandidateFailCategory::as_str()` and must not drift without a docs bump.

## Non-goals

- Default-on AI-DAG in the live chat loop  
- Prometheus / Grafana as an L4 exit gate  
- Catalog meat / public-manifest inverted index  

## Related commands

```bash
velaclaw doctor candidate-dag --candidate <path> [--fallback <path>]
```

See [commands-reference.md](commands-reference.md) (`candidate_dag_shadow`, CR-L4-002/003).
