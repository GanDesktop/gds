# GDS Nixes

Native, flake-based Home Manager configuration browsing.

```sh
just dev nixes
just dev nixes --flake /path/to/configuration
```

Open a local flake directory, select a user, and browse or search enabled modules.
Disabled modules are excluded during evaluation; Home retains its basic fields.
The initial version previews evaluated values and defaults without changing files
or activating a configuration. Nix must be available on PATH.

See [implementation notes](../../docs/apps/nixes/README.md).
