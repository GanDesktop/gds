# Derived Configuration Editors (`ConfigEditor`)

`#[derive(gds::ConfigEditor)]` (implemented in `macros/gds-settings-derive` over `gds::ui::settings::form`) generates native `zgui` settings forms directly from typed Rust configuration structs and enums.

```rust,ignore
#[derive(Clone, Default, PartialEq, serde::Serialize, serde::Deserialize, gds::ConfigEditor)]
#[config(validate = "Self::validate")]
struct Preferences {
    #[setting(label = "settings-enabled")]
    enabled: bool,
    #[setting(label = "form-font-family", kind = "font")]
    family: Option<String>,
    #[setting(label = "form-language")]
    language: Option<gds::i18n::Language>,
    #[setting(
        label = "settings-opacity",
        min = 0., max = 1., step = 0.01,
        scale = 100., unit = "%", default = 0.85
    )]
    opacity: Option<f32>,
    // Unannotated fields are preserved untouched without generating UI rows.
    recent_paths: Vec<String>,
}

impl Preferences {
    fn validate(&self) -> Result<(), &'static str> {
        Ok(())
    }
}
```

## Lifecycle & Event Wiring

1. **Instantiate**: `#[derive(gds::ConfigEditor)]` implements `form::Model` and defines `type <StructName>Editor = form::Editor<StructName>` in the same module (`PreferencesEditor::new(value)` accepting any `Into<Binding<Preferences>>`, such as `RwSignal<Preferences, LocalStorage>`).
2. **Observe**: Wire `.on_change(UnsyncCallback::new(|event: form::Changed<Preferences>| ...))` to receive the complete updated struct (`event.value`) and changed field ID (`event.field`), then run the application's apply and persistence pipeline.
3. **Sync**: Updating the bound `RwSignal<Preferences, LocalStorage>` after external reloads or rollbacks updates controls reactively without emitting `Changed` events. Generated editors never read or write files directly.

## Supported Field Types & Attributes

| Rust Type | Generated Control |
| --- | --- |
| `bool` | Toggle switch |
| `String`, `PathBuf` | Single-line text input |
| `f32`, `f64`, `i8..i64`, `u8..u64`, `isize`, `usize` | Slider with numeric readout and keyboard stepping (`f32` display precision) |
| `gds::i18n::Language` | Searchable dropdown of registered Fluent languages |
| `String` with `kind = "font"` | Searchable system font dropdown (retaining uninstalled saved fonts with a badge) |
| `Vec<String>` with `kind = "string_list"` | Multi-line input (one item per non-empty line) |
| Unit `enum` deriving `gds::ConfigEditor` | Searchable dropdown (`form::Choices` via Serde representation) |

Wrapping a supported type in `Option<T>` adds a *"Follow system"* (`None`) entry to dropdowns, maps empty text to `None`, and uses `default = ...` as the inherited display value on sliders/switches while keeping per-field Reset wired to `Struct::default()`.

### `#[setting(...)]` & `#[config(...)]` Reference

- `label = "fluent-id"` *(required on editable fields and enum variants)*: Fluent message ID for the row or option label.
- `help = "fluent-id"` / `group = "fluent-id"`: Optional hover tooltip and shared section heading for consecutive fields.
- `id = "custom_id"`: Stable field identifier (defaults to Rust field name; duplicate IDs fail at compile time).
- `min`, `max`, `step`, `scale`, `unit`, `default`: Slider bounds, display multiplier (e.g., `0.85` → `85%`), suffix, and `Option<T>` fallback display value.
- `kind = "font" | "language" | "string_list" | "text"` / `skip`: Explicit control override or variant exclusion.
- `#[config(validate = "path::to::fn")]` / `#[config(crate = "::custom_gds")]`: Candidate validation before emitting `Changed` (displaying errors inline below the offending field) and custom crate path override.
