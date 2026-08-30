# Licensing and source boundaries

## Project license

New Supa Diska Klinah source is available under the MIT License in [`LICENSE`](../LICENSE). Dependency licenses remain their owners' licenses and must be reviewed when dependencies change.

## Kudu attribution

The parity inventory was derived from Advent Development Inc's Kudu v2.4.0 IPC registry at commit `db09e051d0615121e659db187e3799438acbc9e6`. Kudu is MIT licensed. Its copyright and complete MIT notice are preserved in [`THIRD_PARTY_NOTICES.md`](../THIRD_PARTY_NOTICES.md).

The parity table uses upstream module names, source paths, and behavioral categories as a compatibility reference. It does not claim that Kudu behavior has been copied or implemented.

## MangoDisk boundary

MangoDisk at commit `260131beab07b9bb82f176a267b23657f783d0df` is GPL-3.0-only. Its cleanup behavior was reviewed only to identify future design constraints: canonical path containment and rejection of links or Windows reparse points before deletion.

Do not copy, translate, or adapt MangoDisk source into this MIT project. The cleanup rule and scan engine are an original implementation based on project requirements, Rust standard-library behavior, and Windows API contracts. MangoDisk supplied behavioral research only: inspect without following links, prove containment, and deduplicate parent-first. Its source and tests did not enter this repository. Choosing to incorporate GPL-covered implementation would require an explicit project licensing decision before any code enters the repository.

## Contributions and dependencies

Contributors must have the right to submit their changes under MIT. Every new dependency requires a license review and an updated notice when its terms require attribution. Generated lockfile content is not permission to use an incompatible dependency.
