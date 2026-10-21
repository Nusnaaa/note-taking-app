# STEM Notebook — Stage 5D Grill-me Owner Decision Delta, Q23–Q26

**Record date:** 19 October 2026  
**Source:** direct owner reply in this conversation  
**Status:** Q23–Q26 approved with explicit Q24 addition; NOT implementation authorisation  
**Provenance:** additive to Q13–Q17 and Q18–Q22 owner decisions; latest underlying specification v0.5.6 remains candidate.

## Exact owner wording (verbatim)

```text
23. i approve
24. i approve. deletion should also show a message stating that there are dependencies
25. yes. i approve
26. the basic chronological browsing would be useful in the first release
```

## Decisions and acceptance obligations

### Q23 — Exhaustive organisational containment for the agreed phase

Owner approved the preceding proposed hierarchy:

| Parent | Permitted direct children |
|---|---|
| Application root | Modules, Projects, Notebooks, Notes, Canvases; independent Journals at M1 |
| Module | A child Module only within a maximum of two total Module levels; Projects, Notebooks, Notes, Canvases; Journals at M1 |
| Project | Notebooks, Notes, Canvases; permitted Project Journals at M1 and subject to existing owner-approved Project-Journal rules |
| Notebook | Notes and Canvases, not other Notebooks |
| Note or Canvas | Its own content and embedded objects; no nested organisational containers |
| Journal (M1) | Journal Logs under existing Journal constraints |

No nested Notebooks, nested Projects or Journals within Notebooks. Root objects and links do not create implicit parents or permission inheritance. Validate destination rules in the backend, not only through unavailable/disabled H.I. actions.

### Q24 — Dependency-aware deletion, Trash and broken links

Owner approved Trash-based recoverable deletion: maintain the target's identity, eligible history and link metadata; references to trashed or missing targets present a non-disclosing unavailable state, and restore reconnects existing links. Permanently deleting a target leaves resolvable broken-reference markers until user intervention; removing a shortcut/link does not delete target. **Additional explicit requirement:** Before an operation deleting an object with dependencies, display a message stating that dependencies exist and explaining likely consequences. Display only dependencies authorised to be disclosed. A sample count/type and final dialog visual treatment are proposed H.I. detail, not independently owner-approved counts, wording or layouts.

### Q25 — Virtual Unfiled view

Owner approved a virtual Unfiled/Loose Notes area for truly independent root work, not a compulsory hidden Notebook/Project. Subsequent filing by Move preserves stable identity, original ink, history and eligible links. Exact label and visual layout are H.I. decisions.

### Q26 — Basic chronology within I1

Owner approved basic chronological browsing in the first release, such as sorting by creation date, last edited date and name and filtering by date, without creating Journal Logs or requiring M1 Journal infrastructure. Actual creation vs last edit must be accurate; retrospective journal research/event dating remains distinct and scheduled for M1.

## Test implications, NOT RUN

- Reject invalid parent-child insertion and moves; verify two Module tiers, unrestricted independent Module Journals *when M1 is active*, and no nested Notebook/Project.
- Deleting an object with incoming/dependent relationships must show an informative pre-delete warning and must not reveal restricted item names.
- Trash/restoration keeps stable identity and resumes valid backlinks; permanent delete leaves non-misleading unavailable link references; deletion of shortcut leaves target intact.
- Root Note/Canvas creation and Unfiled listing must not create hidden organisational parents; move preserves references and history.
- Date/name sorts and date filter work independently of Journal creation; no fabricated research/event timestamp.

No reported executed app tests. No software implementation approved. Existing I0+I1/M1/M2 releases and owner-approved architecture strategy remain unchanged except these explicit clarifications.
