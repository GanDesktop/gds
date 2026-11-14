# GPUI 与字体冷启动后续优化规划（P1 / P2.2）

本文归档 `GDS Files` 启动性能排查中识别但**暂不纳入当前本地改动的上游依赖优化项**（P1 与 P2.2），用于后续排期或在对应上游仓库实施。P0 阶段的应用内优化（去除 GNOME Python 主题子进程、延迟初始化设置视图、消除重复主题重载、缓存 Linux Fontconfig 别名解析、`FileManager1` 注册延后）已直接在 `gds` 仓库完成。

参考基线测量（`gds-files --release`，提交 `9e0fd04da2036fcb3bf10e0e954c6f3b33fe902f`，Ryzen 7 8745H + RTX 4060 / Weston）：

- 已有 Fontconfig 缓存时，非 GNOME 内置主题首帧提交中位数为 **313.4 ms**，其中窗口 surface 创建前阶段中位数为 **54.5 ms**，GPU 适配器选择与初始化里程碑为 **275.5 ms**。
- 无 Fontconfig 缓存（冷目录扫描）时，非 GNOME 内置主题首帧中位数为 **1526.7 ms**（比热缓存慢 923.2 ms 以上）。
- 在开启独立显卡运行期电源管理（PCIe `D3cold` 挂起）的双显卡笔记本上，初始化枚举全部 GPU 后端可能额外触发约 600 ms 的独显唤醒延迟。

---

## P1：`gpui-fast` 双显卡唤醒与重复 Wayland 连接优化

### 问题根因（已定位源码）

1. **全后端枚举导致双显卡设备在仅用核显时唤醒独显**
   - 在 `gpui-fast-wgpu 0.1.13`（`src/wgpu_context.rs`）中，`WgpuContext::instance` 固定以 `wgpu::Backends::VULKAN | wgpu::Backends::GL` 创建 `wgpu::Instance`。
   - `WgpuContext::select_adapter_and_device` 随后调用 `instance.enumerate_adapters(wgpu::Backends::all()).await`，再调用 `Self::sort_adapters(&mut adapters, device_id_filter, compositor_gpu)` 按用户覆盖（`ZED_DEVICE_ID`）、合成器 GPU（`CompositorGpuHint`）、设备类型与后端排序，并逐个测试 `try_adapter_with_surface`。
   - 在双显卡系统（如 AMD 核显驱动显示器 + NVIDIA 独显处于 PCIe `D3cold` 休眠）下，先向系统枚举所有 Vulkan ICD 与 GL/EGL 适配器会打开并查询所有物理设备节点，促使内核唤醒处于休眠态的独显，即使最终选中的仍然是合成器所在的核显。

2. **Wayland 初始化阶段建立多余连接执行第二次同步 roundtrip**
   - 在 `gpui-fast-linux`（锁定的提交 `2270030d3e0cf75def11e44e5cc0f6c9991fae49`，`src/linux/wayland/client.rs`）中，`WaylandClient::new` 在主连接完成一次 `event_queue.roundtrip` 后，立即调用 `detect_compositor_gpu()`。
   - `detect_compositor_gpu()` 单独调用 `Connection::connect_to_env()` 打开第二条 Wayland socket，执行独立的 `registry_queue_init::<DmabufProbeState>`、绑定 `zwp_linux_dmabuf_v1`（v4）、发送 `get_default_feedback` 并再次阻塞等待 `event_queue.roundtrip`，随后经 `crate::linux::compositor_gpu_hint_from_dev_t` 读取 `/sys/dev/char/{major}:{minor}/device/{vendor,device}` 并立即断开该临时连接。

### 后续实施方案（目标仓库：`GanDesktop/gpui-fast`、`GanDesktop/gpui-fast-linux`）

1. **合并 `zwp_linux_dmabuf_v1` 主设备反馈至主 Wayland 握手流程**
   - 在主 Wayland 连接的 `WlRegistry` / `Globals` 初始化期间直接绑定 `zwp_linux_dmabuf_v1` 并发送 `get_default_feedback`，与主连接既有 roundtrip 复用同一次往返拿到 `main_device`，消除独立连接建立与第二次阻塞 roundtrip。

2. **两阶段定向 GPU 选择，避免无谓唤醒 `D3cold` 独显**
   - 当未设置强制覆盖（如 `ZED_DEVICE_ID`），且合成器已通过 `CompositorGpuHint { vendor_id, device_id }` 给出目标 GPU 时，先走快速通道：仅使用 `wgpu::Backends::VULKAN` 创建实例，并在 Vulkan 加载层将枚举范围收窄到匹配合成器 `vendor_id`/`device_id` 的驱动（例如通过受控环境或驱动筛选，前提是不污染同进程全局状态并正确回退），尝试匹配并测试 `surface` 配置。
   - 仅当快速通道未找到兼容适配器、禁用硬件加速或触发设备丢失回退时，才回退到完整的 `Backends::VULKAN | Backends::GL` 与 `enumerate_adapters(Backends::all())` 全量探测。

### 回归与验收标准

- 在双显卡桌面/Wayland 环境启动前检查 `/sys/bus/pci/devices/*/power/runtime_status`，确认在合成器运行于核显、未指定独显的情况下，冷启动 `gds-files` 至首帧提交期间独显保持 `suspended` 状态，GPU 初始化阶段延迟明显下降。
- 验证 Vulkan 正常、核显异常强制回退 GL、软件渲染以及多窗口共用 `GpuContext` 等路径均不回归。

---

## P2.2：`cosmic-text` / `fontdb` 字体扫描与首帧 `mmap` 缓存（暂缓维护）

跟踪 Issue：[perf(appearance): 消除 GPUI fontdb 与 GDS Fontconfig 的重复字体发现及首帧全库字重探测](https://github.com/GanDesktop/gds/issues/8)。

### 问题根因（已定位源码）

1. **冷缓存场景缺少系统 Fontconfig 二进制索引复用**
   - `cosmic-text 0.19.0`（`src/font/system.rs`）在 `FontSystem::load_fonts` 中调用 `db.load_system_fonts()`。
   - `fontdb 0.23.0`（`src/lib.rs`）在 Linux 启用 `fontconfig` 特性时，通过 `load_fontconfig()` 解析 `fonts.conf` XML 中的路径与别名规则并自行递归扫描字体文件，而不是直接复用系统 `libfontconfig` 维护的 `/var/cache/fontconfig/*.cache-*` 二进制索引。当操作系统页缓存/Fontconfig 索引冷启动或目录包含大量字体文件时，字体数据库构建耗时可膨胀至数百毫秒乃至超过 1 秒。

2. **首帧 `get_font_matches` 触发全库可变字重 `mmap` 检查**
   - 在 `cosmic-text 0.19.0`（`src/font/system.rs`）中，`FontSystem::get_font_matches` 对 `self.db.faces()` 中的每个字体面构造 `FontMatchKey::new(attrs, face, &self.db)`。
   - 当 `attrs.weight.0.abs_diff(face.weight.0) != 0` 时，`FontMatchKey::new` 会调用 `db.with_face_data(face.id, ...)` 与 `skrifa::FontRef::from_index(...)` 检查是否存在 `wght` 可变字体轴（`variable_weight_match`），导致首个含非 400 字重或常规排版的渲染帧触发大量字体文件读取/`mmap`。

### 暂缓原因

- **投入产出比与维护成本失衡**：在用户存在正常 Fontconfig 缓存的常规桌面环境下，`gds-files` 在进入 GPUI surface 前的总耗时中位数已仅 **54.5 ms**；将 `cosmic-text` 或 `fontdb` 长期 fork 并在 `[patch.crates-io]`、`Cargo.lock` 与 `packaging/nix/cargo-lock.nix` 中维护补丁，会持续增加上游版本升级与 Nix 锁定同步复杂度。
- **跨语言排版兼容风险**：自定义 `fontdb` 扫描过滤或缓存键逻辑直接影响 CJK、Emoji 与缺字回退（fallback）行为，需要独立维持完整的多语言回归矩阵。
- **优先核查打包侧缓存可用性**：相比修改 `cosmic-text`，先在发行版包（Nix / Debian / Fedora / Arch）中确认应用能稳定访问宿主或包内预生成 Fontconfig 缓存，能更低成本避免终端用户频繁落入冷目录扫描路径。

### 后续重开条件与实施方向

仅当满足以下任一条件时再考虑重新评估或推进上游 PR：

- 实测各发行版真实安装包仍频繁出现因无法命中 Fontconfig 缓存导致的 500 ms 以上冷扫描，且无法通过部署侧缓存配置解决；或
- 首帧非常规字重 `get_font_matches` 触发的 `with_face_data` 成为交互前主要阻塞。

若实施，优先向 `pop-os/cosmic-text` 上游或可控补丁推进两项低侵入改动：
1. 在 `FontSystem` 初始化阶段为每个 `face.id` 预缓存或按需记忆 `wght` 轴范围（`variable_weight_ranges`），避免每次新 `Attrs` 组合在 `get_font_matches` 中重复打开全部非同字重字体数据。
2. 评估通过 `FcFontList` / 序列化轻量字体元数据缓存替代启动期全量磁盘文件扫描。
