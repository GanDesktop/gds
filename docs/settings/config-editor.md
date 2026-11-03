# 从配置类型派生原生编辑器

`#[derive(gds::ConfigEditor)]` 根据 Rust 字段类型与 `#[setting(...)]` 标注生成公共配置编辑组件。宏位于 `macros/gds-settings-derive`，原生控件与状态位于 `gds::ui::settings::form`。Files 的常规设置和共享字体设置已使用这套组件。

```rust
#[derive(Clone, Default, PartialEq, serde::Serialize, serde::Deserialize, gds::ConfigEditor)]
#[config(validate = "Self::validate")]
struct Preferences {
    #[setting(label = "settings-enabled")]
    enabled: bool,
    #[setting(label = "form-font-family", kind = "font")]
    family: Option<String>,
    #[setting(label = "form-language")]
    language: Option<gds::i18n::Language>,
    #[setting(label = "settings-opacity", min = 0., max = 1., step = 0.01,
              scale = 100., unit = "%", default = 0.85)]
    opacity: Option<f32>,
    // 未标注的字段保持原值，不需要编辑控件。
    recent_paths: Vec<String>,
}

impl Preferences {
    fn validate(&self) -> Result<(), &'static str> {
        Ok(())
    }
}
```

宏为结构体实现 `form::Model`，并在相同模块生成 `PreferencesEditor` 类型别名。在 GPUI 上下文中使用 `cx.new(|cx| PreferencesEditor::new(value, window, cx))` 创建实体，直接将实体放入页面。

订阅 `form::Changed<Preferences>`，由应用接收完整的类型化新值 `event.value` 及字段标识 `event.field`，执行原有的应用、保存与错误反馈流程。应用接受或回退后，调用 `editor.set_value(value, window, cx)` 同步控件；程序同步不发出修改事件。生成的控件不自行访问配置文件。

## 字段类型与标注

| 类型 | 控件 |
| --- | --- |
| `bool` | 开关 |
| `String`、`PathBuf` | 文本输入 |
| `f32`、`f64`、8–64 位整数、`isize`、`usize` | 带读数、键盘步进的滑块 |
| `gds::i18n::Language` | 当前支持语言的可搜索下拉框 |
| `String` 加 `kind = "font"` | 系统字体目录的可搜索下拉框 |
| `Vec<String>` 加 `kind = "string_list"` | 每行一项的多行文本；空行忽略 |
| 实现 `form::Choices` 的枚举 | 可搜索下拉框 |

这些类型可包裹在 `Option<T>` 中。可选的下拉字段增加“跟随系统”项，对应 `None`；可选文本的空值对应 `None`。可选数值/布尔字段的 `default` 只决定 `None` 时显示的继承值，交互后写入 `Some`；恢复默认始终读取结构体的 `Default`，可恢复为 `None`。

所有可编辑字段都必须标注 Fluent 消息 ID `label`。其他标注：

- `id`：稳定字段标识，默认是 Rust 字段名。
- `help`：悬停说明的 Fluent 消息 ID。
- `group`：分组标题的 Fluent 消息 ID；连续同组字段共用标题。
- `min`、`max`、`step`：数值范围和步长，可引用常量，默认 `0`、`100`、`1`。
- `scale`、`unit`：显示比例与后缀；例如配置 `0.85` 显示为 `85%`，写回仍为 `0.85`。
- `default`：可选字段的继承显示值。
- `kind`：特殊处理 `font`、`language`、`string_list` 或 `text`；常规类型自动推断。
- `skip`：显式跳过字段或枚举项。

每行使用公共 `SettingRow`、输入框、下拉框、滑块和恢复默认按钮。字体下拉保留未安装的已配置字体并标记“未安装”；语言变化更新标签与目录。恢复默认只修改当前字段，未标注的数据保留原值。

## 枚举与校验

```rust
#[derive(Clone, Default, PartialEq, serde::Serialize, serde::Deserialize, gds::ConfigEditor)]
enum TimeFormat {
    #[default]
    #[setting(label = "form-time-relative")]
    Relative,
    #[setting(label = "form-time-absolute")]
    Absolute,
}
```

枚举派生 `form::Choices`，使用 Serde 的实际枚举值作为选项值。结构体需要 `Clone + Default + PartialEq`；每个可编辑字段需要支持 Serde 序列化与反序列化。只对单个字段转换值，配置文件的整体格式和读写仍由应用管理。

可选的 `#[config(validate = "Self::validate")]` 在字段解码后校验候选配置，错误显示在该字段下方，不发送修改事件；校验函数返回错误类型需实现 `Display`。库重命名时可通过 `#[config(crate = "::renamed_gds")]` 指定路径。

当前支持具体的具名结构体与无数据枚举项。集合、嵌套配置和复杂业务操作保留专用编辑界面；滑块使用 GPUI 的 `f32` 显示精度，不适合编辑任意精度或完整 64 位整数范围。宏对未知标注、重复 ID 和不支持的已标注字段报告编译错误。
