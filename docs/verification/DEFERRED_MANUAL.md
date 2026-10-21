# Manual and device checks — not run

All entries remain **NOT RUN — deferred until the complete runnable application**.
They are not prerequisites for writing the remaining backend. The owner is not
asked for samples or benchmarks now. Samples stay owner-private and local.

| Check | Source | Run after | Evidence needed |
| --- | --- | --- | --- |
| Real maths/prose/numerals/code recognition, including wrong/incomplete notation | NB-REC-008, CF-30, ST5E-T-26 | Runnable app with eligible installed stroke recognisers | Actual editable outputs, failure/correction effort and coverage; real model/version/resource measurements |
| S Pen, resting/moving palm, cancellation, finger navigation and eraser | NB-PEN-001/003, NB-OPS-002, CF-29, ST5E-T-27 | Same app's provisional Android H.I. | Device/build, observed input errors and latency/resource measurements; no invented threshold |
| Local model switching/install/remove/recovery | NB-SUP-003, CF-24 | Runnable model manager | Offline usability, compatibility, failures and notes/source survival |
| Converter, clipboard and structured editing usability | NB-CORE-002, NB-REC-004, ST5E-T-11 | Runnable editor | Selection/copy/paste of editable characters, retained mistakes, readable warnings and actual render/export fidelity |
| PDF split reading, source relink and pen/touch routing | NB-PDF-001/002 | Runnable Android PDF host | Restart/revocation/permission-denial behaviour and no writing obstruction |
| Lock, portable recovery and complete backup usability | NB-SEC-001/002, NB-DATA-006 | Full backup and Android auth integrated | Both export branches reauthenticate, recovery on a clean installation, deliberate plaintext review and content isolation |

M1 Journals/resource catalogues/propagation and M2 LLM/media tests remain later-phase
work, not postponed I0/I1 defects. iPadOS compilation is not an Android release gate.
