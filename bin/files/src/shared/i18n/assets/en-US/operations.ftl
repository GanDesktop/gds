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

paste-cancelled = Paste cancelled. No files were changed.
paste-cancelled-partial = Paste cancelled. { $completed } items were already completed; the rest are unchanged.
paste-aborted-partial = The paste stopped after { $completed } items; those files are complete and the rest are unchanged.

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
