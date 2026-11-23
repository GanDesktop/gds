# Strings owned by file-operation naming, paste conflicts and folder creation.

new-folder = New folder
new-folder-title = New folder in “{ $folder }”
new-folder-message = Enter a name. Existing items are never overwritten.
new-folder-placeholder = Folder name…
new-folder-confirm = Create
new-folder-default-name = New folder
new-folder-working = Creating folder…
new-folder-done = Created folder “{ $name }”
new-folder-failed = Could not create the folder: { $error }
new-folder-exists = An item named “{ $name }” already exists.

transfer-name-empty = Enter a name.
transfer-name-separator = The name cannot contain a path separator.
transfer-name-reserved = “.” and “..” cannot be used as names.
transfer-name-invalid = The name contains an invalid character.

transfer-conflict-title = “{ $name }” already exists in this folder
transfer-conflict-message = Choose a different name. Nothing is overwritten or merged.
transfer-conflict-placeholder = New name…
transfer-conflict-replace-title = Replace file “{ $name }”?
transfer-conflict-merge-title = Merge folder “{ $name }”?
transfer-conflict-replace-message = An item with this name already exists here. Replacing overwrites the existing file and cannot be undone.
transfer-conflict-merge-message = A folder with this name already exists here. Merging keeps files from both folders and prompts only for conflicting files inside.
transfer-conflict-mismatch-dir-over-file = The source is a folder while the destination has a file with the same name. Rename or skip this item.
transfer-conflict-mismatch-file-over-dir = The destination already has a folder with this name. Rename or skip this item instead of replacing the folder.
transfer-conflict-same-message = This target collides with the source item. Choose a different name or skip it.
transfer-conflict-location = In { $location }
transfer-conflict-remaining = { $count ->
    [one] { $count } more item remaining
   *[other] { $count } more items remaining
    }
transfer-conflict-from-folder = From { $folder }
transfer-conflict-in-folder = In { $folder }
transfer-conflict-arrow-replace = Will overwrite existing file
transfer-conflict-arrow-merge = Will merge into existing folder
transfer-conflict-arrow-clash = Conflicts with existing item
transfer-conflict-existing = Existing in destination
transfer-conflict-incoming-copy = Copying
transfer-conflict-incoming-move = Moving
transfer-conflict-badge-newer = Newer
transfer-conflict-badge-larger = Larger
transfer-conflict-badge-identical = Same size & time
transfer-conflict-rename-label = New name
transfer-conflict-rename-reset = Use suggestion
transfer-conflict-rename-exists = An item named “{ $name }” already exists in this folder.
transfer-conflict-apply-all = Apply to remaining conflicts
transfer-conflict-apply-all-hint-replace = Replace affects conflicting files only; folders and type mismatches will still prompt.
transfer-conflict-apply-all-hint-merge = Merge applies to remaining colliding folders (conflicting files inside still prompt); Skip and Auto-rename apply to all remaining conflicts.
transfer-conflict-apply-all-hint-mismatch = Skip and Auto-rename apply to all remaining conflicts.
transfer-conflict-auto-rename-preview = Remaining conflicts will be numbered automatically, starting with “{ $name }”.
transfer-conflict-replace = Replace
transfer-conflict-replace-all = Replace all
transfer-conflict-merge = Merge
transfer-conflict-merge-all = Merge all
transfer-conflict-rename = Rename…
transfer-conflict-rename-confirm = Apply name
transfer-conflict-auto-rename = Auto-rename
transfer-conflict-back = Back
transfer-conflict-skip = Skip
transfer-conflict-skip-all = Skip all
transfer-conflict-abort = Abort

paste-cancelled = Paste cancelled. No files were changed.
paste-cancelled-partial = Paste cancelled. { $completed } items were already completed; the rest are unchanged.
paste-aborted-partial = The paste stopped after { $completed } items; those files are complete and the rest are unchanged.
paste-skipped = { $count ->
    [one] Skipped { $count } item
   *[other] Skipped { $count } items
    }
paste-merged = { $count ->
    [one] Merged { $count } folder
   *[other] Merged { $count } folders
    }
copy-done-skipped = Copied { $succeeded } items; skipped { $skipped }.
move-done-skipped = Moved { $succeeded } items; skipped { $skipped }.
drop-into-self = Cannot move or copy a folder into itself or a subfolder.
drop-unsupported-destination = Files can only be dropped into a local folder.
drop-no-local-files = Could not read local files from the dropped item.
file-operation-busy = Finish or cancel the current file operation first.

undo = Undo
redo = Redo
undo-operation = Undo { $operation }
redo-operation = Redo { $operation }
history-copy = Copy
history-move = Move
history-rename = Rename
history-new-folder = New Folder
history-trash = Move to Trash
history-restore = Restore
help-history = Undo and redo
undo-done = Undo complete
redo-done = Redo complete
undo-failed = Could not undo all items
redo-failed = Could not redo all items

tasks-empty = No running tasks
tasks-count = { $count } tasks
tasks-cancelling = Cancelling…

tasks-title = Background tasks
selection-title = Selection
selection-empty = No selected items
selection-remove-item = Deselect
clipboard-title = Clipboard
clipboard-empty = Clipboard is empty
clipboard-remove-item = Remove from clipboard

permissions-title = Set permissions
permissions-owner = Owner
permissions-group = Group
permissions-others = Others
permissions-read = Read
permissions-write = Write
permissions-execute = Execute / enter
permissions-save = Save
permissions-loading = Loading permissions…
permissions-saving = Saving permissions…
permissions-saved = Permissions saved.
permissions-symlink = Open the link target to edit its permissions.
permissions-recursive = Apply to files and folders inside
permissions-recursive-hint = Child files keep their execute permissions. Symbolic links are skipped.
permissions-partial = Updated { $succeeded } items; { $failed } errors. See the notification for details.
