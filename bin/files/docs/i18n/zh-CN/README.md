> 本项目仍处于早期开发阶段，功能与界面会持续调整。
> 首要支持 **Linux + Wayland**。X11、Windows 和 macOS 的适配仍在完善。

# <img src="../../../../../assets/files/icon-desktop.svg" alt="GDS Files 图标" width="32" height="32" align="middle"> GDS Files

GDS Files 是基于GPUI开发的原生文件管理器。专注于为Wayland用户提供一个美观易用的文件管理体验。

![概览](../../assets/overview.png)

## 关键功能
### 大猩猩也会用！
完整的文件浏览器功能，类似Windows文件浏览器的体验，非专业人士也能快速上手。

![浏览文件夹、切换视图与筛选文件](../../assets/file-browsing.gif)

### 我去！内置终端！
的确，在linux的环境上，终端或许比文件浏览器更不可或缺，为了更方便专业用户的体验，我们内置了一个简单的终端。
![终端展示](../../assets/terminal.png)


### 半透明的窗口，跟随我的系统主题变动。
这下你终于可以看到你的二次元美少女壁纸了，我知道这很重要。除此之外，你还可以调节字号和字体，获得最适合你的体验。

![浅色模式下，GDS Files 随 16 张经典壁纸同步改变配色](../../assets/theme-sync.gif)

### 全键盘操作！
也许你的确没有鼠标呢？我们非常理解您的处境！

### 还有最炸裂的功能：**复制路径！！！**
天哪，为什么这么多文件浏览器都不支持复制文件路径，简直让人感觉不可思议。
![复制文件地址](../../assets/copy-path.png)

## 安装与启动
### 让您的agent帮您安装
```
帮我安装这个文件浏览器：https://github.com/GanDesktop/gds/blob/main/bin/files/README.md
```

### 通过包管理器
<!--包管理器安装还没有做-->

### Nix安装

**Profile 安装**：

```sh
nix profile install github:GanDesktop/gds#gds-files
```

**NixOS 系统安装**：在 `flake.nix` 添加输入，并将 `gds` 加入 `outputs` 参数：

```nix
inputs.gds.url = "github:GanDesktop/gds";
```

在系统的 `modules` 列表中加入：

```nix
gds.nixosModules.gds-files
gds.nixosModules.gds-portal
{
  programs.gds-files = {
    enable = true;              # 安装 Files，默认作为文件夹打开程序
    fileManager1.enable = true; # 支持其他应用的“在文件夹中显示”等请求
  };
  services.gds-portal.enable = true; # 单独安装 GDS Portal，提供系统文件选择接口
}
```

在系统配置目录重建：

```sh
sudo nixos-rebuild switch --flake .
```

## 设置与数据

从主菜单进入“设置”。设置更改立即生效并自动保存。

Linux 下的数据位置如下；设置了 XDG 环境变量时，使用对应的目录：

| 内容 | 默认位置 |
| --- | --- |
| 应用设置 | `~/.config/gds/files/settings.toml` |
| 收藏、最近位置与各位置的视图偏好 | `~/.local/state/gds/files/state.sqlite` |
| 标准缩略图缓存 | `~/.cache/thumbnails/` |


## 文件选择器

GDS Portal 是 GDS 的桌面 Portal 后端，目前提供文件选择接口。启用 GDS Portal 后，采用该接口的应用可以唤起 Files 的原生选择器，指定打开或保存模式、文件类型、初始位置和其他选择项。

## Debug

使用以下命令启用本进程的内存日志与日志面板；默认不开启此功能：

```sh
gds-files --enable-logs
```

## 许可证

GDS Files 的自有代码采用 [Apache-2.0 许可证](../../../../../LICENSE)，第三方依赖保留各自的许可证。

## 开发与集成

在仓库根目录构建开发版，或启用日志启动：

```sh
just build files
just dev files --enable-logs
```

开发环境与构建方法见[开发指南](../../../../../docs/development.md)。

集成文件选择器时，也可以通过命令行调用：

```sh
gds-files --pick-file --multiple --filter '图片:*.png,*.jpg,*.webp'
gds-files --pick-directory "$HOME/Documents"
```

命令行选择器确认后向标准输出写入 JSON，再退出；取消时不会返回选中文件。参数和返回格式见[文件选择器说明](../../../../../docs/apps/files/picker.md)。
