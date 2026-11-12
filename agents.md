## 个性
请你在写代码的时候，能够有整体性的审美，确保项目整体向一个好的方向发展，而不是为了单次任务的通过增加了系统整体的复杂性，最后在整体上得到了更坏的结果。

## 项目目标
本项目的目标是在构建一套基于GPUI的，原生桌面应用套件。

## 兼容性支持
首要支持 Linux + Wayland，次要支持 Linux + X11，再次支持 Windows / macOS。内部优先在 NixOS / Niri 验证；面向用户的文档统一描述 Linux + Wayland 支持目标，不强调发行版或合成器的验证优先级。

## 技术栈
1. 使用zed的GPUI
2. 长桥的[gpui-kit](https://github.com/longbridge/gpui-kit)
3. ↓请不要无视下一条，很重要↓
4. **使用**[gds-uim](https://github.com/GanDesktop/gds-uim)这个库来组织ui，具体例子可以看这个库的docs文件夹和example。对于需要重复使用的组件，请以解剖的形式声明。可以参考公共组建库中的`ContentPage`。默认使用`Default::detault`来创建组件。请不要无视这一条！！！！！
5. ↑请不要无视上一条，很重要↑
6. i18n 使用mozilla的fluent

## 依赖与补丁

第三方源码不复制进本仓库的 vendor 或其他目录维护。需要修改依赖时，在 `GanDesktop` 的对应仓库按上游版本建立补丁分支，提交并推送修改；GDS 通过 Cargo Git 分支引用，由 `Cargo.lock` 锁定具体提交，同时更新 Nix 的源码 hash。补丁说明保留上游版本、来源和修改范围；上游发布修复后再切回发布版。

公共库默认 feature 为空。依赖声明为 optional，模块与依赖按同一能力 feature 启用；`files`、`agents`、`portal` 只组装各自需要的能力。所有应用在根 `Cargo.toml` 中声明 `[[bin]]` 与 `required-features`，不为普通应用另建 package 或 manifest。共享的纯协议通过 `desktop-protocol` 独立使用；Portal 通过 `portal` feature 使用本库，不能因此带入 GPUI、SQLite、终端或远程文件后端。

应用自有的异步 I/O 统一用 Tokio，包括进程、信号、计时和 zbus 后端；不再引入 async-io、async-process、async-signal 等另一套应用 I/O 运行时。GPUI 的界面执行器与 Tokio 的职责保持分离。图形应用由 `service::task_manager::Supervisor` 持有 Tokio Runtime；服务只接收 TM 作用域，通过作用域派生任务，不直接访问 Runtime Handle。应用决定配置与服务启动顺序，TM 管理准入、取消和完成，服务负责资源清理。

## UI设计
1. 不要过多描述一个东西有什么用，让icon与UI自己去说话
2. 但是在鼠标hover上去的时候，也要有一个非常简单的说明。
3. 注重视觉语言，也就是说，一个同功能的东西，在不同的应用上，一个应用内的不同地方，要看起来是差不多的，减少用户的学习成本。然后我们这个项目的所有应用要有看起来大差不多的视觉风格。把视觉语言固化到`docs/vistual-language`

## 可复用性
因为我们这个项目肯定有很多需要复用的东西，所以做时候不要仅仅为了某一个应用考虑，而是要考虑到可复用性。

## 项目组织
使用一个大 lib，公共库入口在 `src/lib.rs`，应用放在根目录 `bin/<应用>/`，代码放在各自的 `src/` 下，例如 `bin/files/src/main.rs`。所有 binary target、feature 与依赖仍由根 `Cargo.toml` 管理。

公共代码放在根 `src/`，应用专属代码放在 `bin/<应用>/src/`。应用目录可放 README 作为开发入口，详细文档仍放在 `docs/`。项目自有代码统一使用根 `LICENSE`，应用目录不重复放 LICENSE 或 NOTICE。

构建、打包与安装集成统一放在 `packaging/`：Nix 定义位于 `packaging/nix/`，Fedora RPM 定义位于 `packaging/fedora/`。根目录的 `flake.nix`、`flake.lock` 和 `shell.nix` 保留为 Nix 入口。

过程宏 crate 放在根 `macros/`，例如 `macros/gds-settings-derive`。应用身份图标统一使用 `assets/<应用>/icon-solid.svg`；桌面入口使用 `icon-desktop.svg`。桌面文件、窗口 app ID 和相关匹配规则使用 `io.github.GanDesktop` 命名空间。

## 文档规范
1. 保持`README.md`清洁，不要随意修改其内容
2. 文档放在`docs`下面
3. 文档不是日记，只记录当前事实。更新时直接修订相关内容，删除过时描述；不要追加按日期或轮次组织的工作记录、调试过程、验证流水账或对话经过。

## 测试
1. 对于一次性的，验证性的工作，测试代码不应该保留在项目中。
2. 不要事无巨细的测试，只测试关键的工作流，或者代码中不显而易见的结果。
3. 实现功能优先。避免在开发途中事无巨细的测试，避免为了小问题钻牛角尖。多和开发者交流，尽量在一批工程完成后让开发者去测试。

## Rust 编码规范

以下规范仅限定本项目的代码，不针对项目生成的代码。

### 代码组织

#### 子模块

子模块组织按照如下结构，**不要**采用`mod.rs`的组织方式

```text
🦀<module>.rs
  +--📂<submodule>
      +--📂assets
          +--🦀hard-coded-config.json
          +--🦀some-long-text.txt
      +--🦀<submodule a>.rs
      +--🦀<submodule b>.rs
      +--🧪tests.rs
  +--🦀<submodule>.rs
```

#### 白箱测试

其中，白箱测试放在各个模块的`tests.rs`中，不要和普通代码混在一起。写白箱测试不要随意写，只有本仓库中实现，的确需要测试的代码，才要写白箱测试。

管理后台的测试应覆盖业务结果、鉴权与数据安全、协议兼容和实际交互回归。不为锁定颜色、CSS 类名、固定文案、展示精度或源码实现细节单独写测试；简单界面调整不要求补测试。防止脚本转义、编码破坏等实际缺陷的回归检查应保留。相同契约避免重复覆盖，优先验证可观察的行为，避免照抄实现做断言。

#### 资源文件

有时候我们会需要一些硬编码的资源文件，如json或者长文本，这些文件放在模块下的assets目录下。

### 异步与控制流

优先使用原生异步模式（`impl Future` 或 `BoxFuture`），不要引入 `async_trait`，除非有充分且明确的理由。

使用 Rust 2024 的惯用写法，善用 `let ... else`、循环返回值和提前返回减少嵌套；在 `select!` 中先返回本地 `Event` 枚举变体，再在选择之后处理。

### 错误处理

对于内部的错误，请使用`thiserror::Error`来声明错误。`anyhow::Error`类型用于不受控的外部系统，比如网络请求，命令行调用的结果等。

对于返回错误的函数，需要在文档中有一个`# Errors`部分来描述可能的错误。

不要忽略错误：一个错误要么被明确处理，要么向上传播，避免隐式合并或吞掉错误。

对于一定要`unwrap()`的情况，应该使用`expect("reason")`来替代，并且给出清晰的理由。

### 治理魔法值

对于将要写入的魔法值，可以考虑如下情况

1. 我只是写写测试，并且只在这里用一次，就不需要有太多的顾虑。
2. 这个值是一个显而易见的平凡值，如0，1，一天有**24**小时这样的常定的数字, 可以直接写入。
3. 这个值可能是一个可配置的值，则应该从配置中读取，通过设定配置的默认值来达到设定其的目的。
4. 这个值确实是一个不可配置并且需要在这里硬编码的值，应该用`const`或者`static`来定义。

### 可维护性

- 将长函数拆分为聚焦的辅助函数
- 为函数和变量使用有意义的名称
- 每个函数只做一件事

#### 打包导出

对于一个相对独立的模块，应该统一打包导出。使用方可以使用通配符`*`引入，这样在导入的代码部分，就可以避免大量的变动。在git提交记录上看起来非常干净。

#### 组织测试

一个测试文件应当对应一个具体的领域或者功能，避免随意添加测试文件。

测试应当遵循如下原则
- 冗余测试是有害的。
- 变更检测测试是有害的。
- 没有真正的行为盲区，就不要为错误修复写回归测试。
- 优先将端到端测试作为唯一的测试机制，并在测试结束时产出可验证、可复现的工件。
- 除了无法控制的第三方外部服务，禁止使用 Mock。
- 测试之间必须完全隔离，且禁止使用硬编码的时间等待。
- 测试代码中禁止出现条件分支和循环等控制流逻辑。
- 只有无法通过端到端测试覆盖的复杂纯计算逻辑或极端异常分支，才允许编写单元测试。
- 不要测试框架、第三方库或没有业务逻辑的简单代码。

### 注释

#### 简洁

注释是为了在不同的维护者之间传递最关键的信息，因此需要谨慎的使用注释。仅为非显而易见的逻辑添加简洁注释，避免连篇累牍的注释。

#### 意图清晰

注释应该是面向未来修改此代码的人，解释此处代码的行为，而不是解释过去。

**反例 🚫**：

- 我做了A，然后我发现做错了，于是我采用了B。（如果B是对的，且A的代码以被消除，为什么要去提原来的A？）
- 这里不要做A, 基于讨论的1,3点，使用B。（后来的维护者并不知道你的讨论）

**正例 ✅**：

- 参见 [＃32](https://github.com/...)的讨论 （有可追溯的讨论）
- 这里是一个妥协的做法，需要后续优化，可能可以采用xxxx的思路 （指出妥协，异常这些真正需要后来维护者人理解的地方）

#### 权威来源

解释自己的行为的时候，一篇权威链接（如规范，文档，论文引用）的信息量要胜过1000行自顾自的解释。

### 相信经过验证的代码

#### 使用经过验证的库函数

比如字符串处理，优先使用 `str::split`、`str::trim`、`str::strip_prefix` 等库方法，而不是通过索引切片。例如，用 `s.strip_prefix("xxx")` 代替 `s.starts_with("xxx").then_some(&s[0..3])`，更健壮、意图更清晰。

#### 避免造轮子

如果社区对于一些东西已经有了成熟的实现，那就不要自己写一个半成品。如果不知道有没有，可以想想需要类似功能的流行库，它们是依赖什么实现的。如果它们是自己实现的，考虑能否直接复用它们的代码？

### 语义正确性

我们从可维护性与AI友好性的高度出发，高度重视代码的语义正确性。

**正例 ✅**：
从概念与逻辑出发组织树状的结构

```text
🦀payment.rs
  +--📂impls
    +--🦀<impl a>.rs
    +--🦀<impl b>.rs
  +--🦀impls.rs
  +--🦀audit.rs
  +--🦀model.rs
  +--📂model
    +--🦀bill.rs
    +--🦀credits.rs
  +--🧪tests.rs
```

**反例 🚫**：
缺少层级，大量不平级的代码在目录上平级。

```text
🦀payment.rs
  +--🦀<impl a>.rs
  +--🦀<impl b>.rs
  +--🦀audit.rs
  +--🦀bill.rs
  +--🦀credits.rs
  +--🧪tests.rs
```

#### 响应兼容

HTTP 响应结构（`*Json`）优先复用现有结构，非必要不新建。新增字段一律用 `Option` 类型（配合 `#[skip_serializing_none]`），不填充时序列化省略，避免破坏旧客户端。

#### 考究的命名

应该在一个事项的命名上就能够体现出它的职责和功能，让阅读代码的人看到这个名字就知道它可能大概会有什么样的作用。

#### 不要滥用Tuple

不要滥用Tuple作为函数的返回值或参数，应该使用具名结构体代替。

#### 尽量少的参数

具有较多参数的函数应该折叠到结构体中
**正例 ✅**：
把主要的参数，可选项，上下文，对象描述符分别打包

```rust
fn make_some_big_news(&self, target: &TargetDescriptor, request: Request, context: &Ctx, options: &Options) -> Result<Response, Error>;
```

**反例 🚫**：
太多的参数，无论是交换位置还是添加新的参数对于维护来说都是灾难性的。

```rust
fn make_some_big_news(&self, target_id: &str, by: &str, content: &str, timestamp: &str, notify: &[&str], max_numebr: Option<usize>, timeout: Option<Duration>) -> Result<Response, Error>;
```

#### 少写游离函数
如果一个函数在语义上明显是操作或者归属于某个结构，就把这个函数放在`impl Block`内，不然对于未来代码和文档的阅读者来说将会是一个巨大的挑战

#### 避免冗余信息

1. 合理利用命名空间，比如一个结构`Request`，已经在模块声明，如已经定义在`system`模块，那么如果它再叫`SystemRequest`可能就有一些冗余，应该直接使用`Request`。并且在使用的时候使用`system::Request`来引用它。但这并不总是成立，比如很多模型都会被统一从`model`模块导出，那么命名的时候加上具体的领域就是有意义的，因为调用方已经不能获得其所在的具体子模块了。
2. 不要再结构体名称和字段中提供冗余的信息，除非是序列化需要，比如`ClientProfile`这样一个结构，就不要再用`client_name`来表示客户端名称了，应该直接用`name`。相反的，如果我们取出它作为独立的变量，可能在一些情况下，为了避免混淆，就要用`client_name`来表示客户端名称。

## 项目额外的代码规范
### 关于GPUI的使用
1. `on_click`的handler尽量少些闭包，通过成员函数或者提前写好的闭包提供，提升代码可读性
2. 在使用处导入 UIM 和翻译宏，使用 `el!`、`m!`、`tr!`，不写完整模块路径的宏调用。
3. UIM 中元素 ID 使用 `# expression`，如 `# format!("audio-{id}")`；常量路径、变量和宏调用直接作为 DSL 值，不加多余的 `$()`。仅在需要 Rust 字段访问、方法链或运算等语义时使用 Rust 转义。
4. UI 二进制采用 FSD：`app → pages → widgets → features → entities → shared`，只建立实际需要的层。业务差异集中在二级切片；`app` 负责启动、组装与生命周期，`shared/i18n` 提供翻译能力。跨切片只访问 `<slice>.rs` 的显式公开入口；同层切片不互相依赖，实体关系通过 `x/<consumer>`（Rust 对 FSD `@x` 的适配）明确暴露。`app` 和 `shared` 使用职责分段。具体边界和标准链接见 `docs/development.md`。

5. 公共 `gds` 库的根能力为 `app`、`appearance`、`service`、`ui`、`persistence`、`i18n`。`app` 通过唯一扩展 trait `AppExt` 提供 `cx.gds()`（可变）与 `cx.gds_ro()`（只读）访问器，下挂 `tasks()`、`appearance()`、`i18n()`、`ui()` 等固有子访问器，统一承载 GPUI App 上下文的全局读写、启动组装与服务生命周期；应用专属能力同样通过单一本地扩展 trait（如 `FilesExt` 的 `cx.gds().files()` / `cx.gds_ro().files()`）挂载固有子访问器（如 `preferences()`、`volumes()`），trait 只声明访问入口，初始化和操作放在访问器的固有方法中；访问器只借用上下文，配置作为输入；`appearance` 保存主题、字体与视觉 tokens；`ui` 只保存组件及其专属状态、事件和辅助实现，不转导出外观基础能力。`service/task_manager`、`service/terminal`、`service/desktop`、`service/logs` 提供服务，服务代码不反向引用 `ui` 或 GPUI 类型；终端呈现、焦点、字体、IME 与输入类型适配位于 `ui/terminal`，日志查看器位于 `ui/logs`。应用组装层安装服务退出清理钩子与本进程日志；配置结构与保存策略留在应用，公共 `persistence` 只提供存储机制。
6. 图形应用使用 clap 管理启动参数；内存日志与日志面板仅通过 `--enable-logs` 显式开启。关闭时不得隐式创建日志队列，也不显示日志入口或注册日志窗口快捷键；普通通知与 stderr 诊断独立于此开关。

## 文档与i18n
对外统一使用 `GDS`、`GDS Files`、`GDS Agents` 和 `GDS Portal`等，不使用预留中文名。

桌面入口显示名称使用 `Files` / `Agents`，不加 GDS 前缀；开发版追加 `(Dev)`。应用 ID、命令名与包名保持稳定。

桌面入口的 `Name`、`GenericName` 和 `Comment` 及译文统一放在 `assets/<应用>/desktop.toml`，由 Nix 与开发版安装器共用，生成标准 `.desktop` 本地化字段。桌面语言由系统 locale 选择，与应用内 Fluent 独立。

GDS Portal 的软件包与服务模块位于 `packaging/nix/portal/`，通过 `services.gds-portal.enable` 单独启用；图形应用模块不隐式安装或启用 Portal。Portal 包不绑定具体图形应用的安装路径。

项目所有的**非开发**文档一律使用英文，对应的i18n文档放在对应的docs目录下面的i18n/<language>下面，使用和原英文文档同样的文件名。

项目的 Portal 后端统一称为 `GDS Portal`；`FileChooser Portal` 仅指桌面通用的文件选择接口，不作为项目产品名。
