# GDS Nixes

GDS Nixes is a native GPUI application for flake-based configuration management.
The first milestone is a read-only Home Manager browser. It uses the shared GDS
window frame, sidebar, settings rows, theme, typography, and Fluent translations.

## Running

Run `just dev nixes`, then enter a local directory containing `flake.nix` and
choose **Open flake**. `just dev nixes --flake /path/to/configuration` opens a
configuration at startup. Nix is required; evaluation may fetch flake inputs.

Both `homeConfigurations.<name>` and
`nixosConfigurations.<host>.config.home-manager.users.<name>` are discovered.
For NixOS, an evaluation-only module captures options inside each configured
Home Manager user via `extendModules`. This preserves user imports, module
arguments, shared modules, and host context; no module is written to the flake.
An unconfigured `getSubOptions` tree cannot resolve config-dependent definitions.
This follows the [Home Manager flake interfaces](https://nix-community.github.io/home-manager/nix-flakes.html).
An existing locked configuration is recommended. The application does not create
or update lock files, build activation packages, or switch generations.

## Preview behavior

The sidebar lists users and enabled module groups. A module is included when its
`enable` option evaluates to `true`, including when that is the module's default.
Disabled branches are pruned in Nix before reading their option metadata or
values; this also excludes enabled descendants beneath a disabled parent.
Subgroups without their own switch inherit their parent's enabled state. Groups
without an enabled ancestor are omitted. Home keeps only username, home directory,
state version, packages, and session variables as basic configuration.
Search matches option paths and descriptions only within this reduced result. Rows derive from evaluated module definitions,
including user-defined options. Boolean values use disabled switches; other
scalars use value fields. Lists show their items or package names, and attribute
sets show their keys rather than serializing arbitrary Nix structures.
Short descriptions accompany values; full descriptions, option paths, types,
and available defaults appear on hover. An unset value
is distinct from an evaluation failure. Internal options are omitted. Disabled
modules are absent from both navigation and search; there is no full catalog view.

Evaluation runs on Tokio, outside the UI executor, with a three-minute timeout.
The selected configuration is cleared during loading; failed loads offer the
same open action for retry. Nix evaluation failures show a short message with expandable, scrollable
error details. Missing values and catchable per-option failures are isolated; Nix
`tryEval` cannot catch every evaluation error, so some failures still prevent a
configuration from loading.

## Boundaries and next milestone

This version does not edit, save, or activate configurations. Submodules and
attribute sets are summarized rather than recursively expanded into editable
forms. Module descriptions retain their original language. Remote flake URIs,
nix-darwin targets, and persisted application preferences are not included.

The application follows the existing FSD structure: `app` owns lifecycle,
`pages/home` owns navigation and presentation, `entities/home` owns Nix discovery
and option metadata, and `shared/i18n` owns application messages. Nix expressions
are embedded resources, not generated shell scripts. User paths and attribute
names are passed as escaped Nix strings, including literal interpolation syntax.

The editing milestone should introduce type-aware draft controls, validation,
and an explicit application-owned override module imported by the flake. It must
separate draft changes, saving, evaluation, and activation. Arbitrary existing
Nix source cannot be safely rewritten from its evaluated values alone.

## Validation

`cargo test --bin gds-nixes --features nixes` requires Nix on PATH. The integration
fixtures cover standalone outputs and real Nixpkgs submodule evaluation,
user-dependent option definitions, special arguments, custom user modules,
unusual user names, per-option failures, pruning disabled and ungated modules,
structured value summaries, and the absence of lockfile writes. Real Home Manager
modules should also be checked when changing the metadata exporter.
