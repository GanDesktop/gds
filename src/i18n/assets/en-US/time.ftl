## Timestamp presentation shared by every GDS application.

time-placeholder = —
time-epoch = Unix epoch
time-just-now = Just now
time-minutes-ago = { $count ->
    [one] { $count } minute ago
   *[other] { $count } minutes ago
    }
time-today = Today { $time }
time-yesterday = Yesterday { $time }
time-days-ago = { $count ->
    [one] { $count } day ago
   *[other] { $count } days ago
    }
time-weeks-ago = { $count ->
    [one] { $count } week ago
   *[other] { $count } weeks ago
    }
time-months-ago = { $count ->
    [one] { $count } month ago
   *[other] { $count } months ago
    }
time-years-ago = { $count ->
    [one] { $count } year ago
   *[other] { $count } years ago
    }

## Settings

time-format-label = Time display
time-format-relative = Relative time
time-format-absolute = Full date and time