# Media browser refactoring

Implemented on `codex/image-support`; the user tested it and reported that it works.

- `mediaModel.ts` shares native file/listing/Trash reply types across the browser, workspace search and move window. Existing IPC field names remain compatible with the catalog.
- `useMediaBrowser.ts` owns date/type/tag filters, workspace scope and displayed results. `mediaResults.ts` provides the common folder filtering pipeline. Workspace search now takes a typed options object.
- `useMediaSelection` owns selected paths and the range anchor. `mediaSelection.ts` defines modifier/range selection against the displayed sequence. Select all and stale-selection reconciliation use that same sequence.
- `trashWorkflow.ts` freezes the selected paths/filter values for validation, confirmation and execution. It handles cancellation, confirmation details and result messages/neighbor selection. `App.tsx` retains UI effects and native calls.
- Native `trash_media.rs` validates every file before creating an executable batch; execution accepts only a validated batch. Tests inject a simulated Trash destination. Catalog journaling, preview cleanup and partial failure reporting remain in the native command.
- Backend date matching lives in `media_filters.rs`, shared by workspace queries and Trash validation.

Checks: frontend build/lint/tests; 65 Rust tests; Clippy with warnings denied.

Regression coverage:

- Today → displayed results → select all → confirmation → injected Trash leaves yesterday's image and folders intact.
- The native validator rejects a batch containing yesterday's file before execution; a validated batch moves only today's file to an owned temporary test directory.
- Cancellation never calls deletion. Stale/hidden selections are rejected. Changing the caller's selection after preparing a request does not change its paths.
- Partial failures retain failed selections; date/type/tag combinations and Ctrl/Shift selection share the filtered order.
- Tests do not touch the real Recycle Bin or user media.

Manual checkpoint: mixed List/Grid browsing, Today/Yesterday and persisted filters, Ctrl/Shift/select-all selection, cancelled Trash, and disposable-file Trash with yesterday's files retained. Also check workspace paging and video playback.

No schema/version changes or installer. Commit/push only on request.
