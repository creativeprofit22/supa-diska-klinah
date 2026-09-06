# Product interface direction

## Surface and audience

Supa Diska Klinah is a Windows desktop utility for developers managing local disk use. The Cleanup flow must make the safety boundary obvious before users grant repeatable process-launch authority or allow artifact quarantine.

## Design thesis

Use the existing flat, bordered utility surfaces and shared 52rem Cleanup rail. Present profile registration as a precise native form, saved profiles as an operational list, and budget evidence as totals followed by selected and protected generations. The primary action is native review and registration; destructive-looking actions always explain their real effect.

## Reuse map

- Existing page headers, buttons, danger buttons, status/error treatments, accounting lists, fields, spacing, colors, focus styles, forced-colors rules, and responsive breakpoint remain authoritative.
- Native inputs, selects, fieldsets, lists, headings, and status regions preserve platform semantics.
- No new icon, font, animation, elevation, gradient, or decorative card system is introduced.

## States and behavior

- Loading, empty, error, disabled, queued, running, cancelled, failed, analysis-failed, selected, and protected-floor states remain in normal reading order.
- Arguments are separate repeatable fields; artifact rows pair a relative path with a role.
- Running profiles expose Cancel. Forget explains that it removes authority without deleting artifacts.
- Settings name every unit and keep automatic enforcement off by default.
- At narrow widths, grids collapse to one column and actions become full width.

## Accessibility scope

Changed scope covers Cleanup profile registration, saved runs, budget preview, and Settings budget controls. Native controls provide names, roles, values, keyboard behavior, and validation. Async outcomes use status or alert regions. Focus remains visible through existing `:focus-visible` rules. The layout supports 320 CSS-pixel reflow, text expansion, reduced motion, and forced colors. Screen-reader, 200% zoom, and Windows high-contrast manual release evidence remain required before any conformance claim.
