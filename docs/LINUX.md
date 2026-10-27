# Linux（X11）支持与验收

使用 X11 桌面会话、支持 Vulkan 或 OpenGL 的显卡驱动，以及启用透明合成的窗口管理器。当前主进程与 Slint 设置进程均明确选择 X11 后端，不提供原生 Wayland 支持。建议在登录界面选择 Xorg/X11 会话。GNOME 用户需要启用 AppIndicator 托盘扩展，才能看到托盘入口。

## 构建和运行

以 Ubuntu 24.04 / Debian 系列为例，先安装 Rust stable 和系统依赖：

```bash
sudo apt-get update
sudo apt-get install -y build-essential pkg-config libgtk-3-dev \
  libayatana-appindicator3-dev libxdo-dev libx11-dev libxrandr-dev \
  libxi-dev libxcursor-dev libxinerama-dev libxkbcommon-dev \
  libxkbcommon-x11-dev libvulkan-dev libegl1-mesa-dev libfontconfig1-dev
git submodule update --init --recursive
cargo build --release --bin bandori-pet --bin bandori-ui --locked
./target/release/bandori-pet
```

两个可执行文件需要放在同一目录。不需要以 root 运行。运行已有构建时仍需 GTK 3、Ayatana AppIndicator（或 libappindicator）、libxdo、X11/xkbcommon 与显卡驱动的运行库。依赖包名称因发行版而异。

```bash
bash scripts/package-linux.sh --skip-build
tar -xzf dist/linux-x86_64-unknown-linux-gnu-release/BANDORI-PET-LITE.tar.gz
./BANDORI-PET-LITE/bandori-pet
```

打包脚本生成包含两个可执行文件、内置目录、图标与许可说明的 tar.gz，不捆绑系统动态库。默认构建 release；`--debug --skip-build` 可打包现有 debug 程序。应在与目标发行版兼容的环境中构建，较新发行版的二进制可能需要较新的 glibc。

## 数据和平台实现

- 配置、模型、日志默认位于 `$XDG_DATA_HOME/BandoriPetLite/`，未设置时使用 `~/.local/share/BandoriPetLite/`。解压缓存位于 `$XDG_CACHE_HOME/BandoriPetLite/cache/`，默认 `~/.cache/BandoriPetLite/cache/`。空或相对 XDG 路径按规范忽略。
- `BANDORI_PET_ROOT` 可覆盖配置、模型和日志目录；设置程序始终从主程序旁启动。内置目录从可执行文件旁的 `assets/` 读取，数据目录下的 `assets/` 可以覆盖它。
- X11 查询提供全局鼠标坐标和按钮状态，供拖拽、视线追踪、透明区域穿透及径向菜单外部点击关闭使用。坐标保持为 X 服务器物理像素，UI 缩放只作用于窗口内容。
- 显示器优先通过 XRandR 1.5 查询，旧服务器回退到输出/CRTC 或实际屏幕尺寸。工作区取 EWMH `_NET_WORKAREA` 与各显示器的交集；未公布工作区的窗口管理器回退到完整屏幕。复杂的多屏独立面板布局取决于窗口管理器提供的工作区信息。
- 桌宠使用无边框、透明、置顶的 Utility 窗口，并设置跳过任务栏和分页器的提示。鼠标穿透由 winit 的 X11 输入区域实现，径向菜单根据圆形按钮区域轮询穿透。最终焦点与叠放规则由窗口管理器决定。
- GTK/AppIndicator 在主事件线程创建，并在 winit 的帧循环中派发 GTK 事件，托盘可打开设置、显示/隐藏、调整透明度和退出。
- `flock` 提供用户级单实例保护，崩溃后由内核释放。Linux IPC 使用带用户 uid 前缀的 abstract Unix domain socket，避免不同用户占用同一名称，退出后没有 socket 文件残留。
- 登录自启动写入 `$XDG_CONFIG_HOME/autostart/io.github.bandori-pet-lite.desktop`，默认在 `~/.config/autostart/`；保留程序路径和 `BANDORI_PET_ROOT`，不依赖启动时的工作目录，取消后删除。请在启用前将程序放到固定位置。
- 本地时间通过 `localtime_r` 获取，遵循系统时区和夏令时。「OBS 采集兼容」仅修改 Windows 窗口样式，在 Linux 上不改变行为。

## 验证范围

Linux CI 执行 workspace 编译、Clippy、普通测试和文档测试，构建两个程序并生成 debug tar.gz 供验证。Xvfb 检查全局鼠标、显示器和服务端输入穿透区域；另一项检查启动真实的主进程与设置进程，确认 IPC 连接和正常退出。

Windows 本地回归和 X11 模块的单独编译不能证明 Linux 动态链接与窗口运行行为；Xvfb 也不能验证真实 GPU、透明合成、桌面托盘或多屏 DPI。发布前需在真实 X11 桌面确认：

1. 无模型首次启动可进入下载页，托盘设置与退出有效；关闭设置后可重新打开。
2. moc 与 moc3 模型透明显示，无黑色背景；模型点击、拖动和视线追踪有效，透明区域可点击下面的其他应用。
3. 径向菜单动画、按钮、透明间隙穿透与外部点击关闭有效。
4. 不同分辨率与缩放的多屏可拖动、缩放及恢复位置，拔掉外接屏后角色仍在可见范围内。
5. 隐藏/显示、置顶以及重启恢复位置正常，桌面应用的焦点行为符合预期。
6. 重复启动只有一个配置写入者，强制结束后可重新启动并连接设置进程。
7. 自启动启用/禁用在下次登录生效，包含空格等字符的安装路径与自定义数据目录可正常启动。

原生接口参考：[winit X11 扩展](https://docs.rs/winit/0.30.13/winit/platform/x11/index.html)、[EWMH](https://specifications.freedesktop.org/wm/latest/)、[XDG 自启动](https://specifications.freedesktop.org/autostart/latest/)、[Desktop Entry Exec](https://specifications.freedesktop.org/desktop-entry/latest/exec-variables.html)、[tray-icon 平台说明](https://docs.rs/tray-icon/0.26.1/tray_icon/)。
