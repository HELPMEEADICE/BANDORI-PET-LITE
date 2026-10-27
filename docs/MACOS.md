# macOS 支持与验收

目标环境：macOS 12+，Apple Silicon（`aarch64-apple-darwin`）或 Intel（`x86_64-apple-darwin`），使用 wgpu Metal 后端。构建方式见 [README](../README.md#macos-构建与打包)。

## 平台实现

- 主进程和设置进程均使用 Accessory 激活策略，主进程通过菜单栏图标提供设置、显示/隐藏和退出操作。
- AppKit 提供屏幕可用区域、全局鼠标位置和按键状态；窗口位置、命中检测与鼠标位置采用同一 backing scale。工作区避开 Dock 和菜单栏。径向菜单跨进程传递逻辑坐标，并按目标屏幕缩放。
- 桌宠首次显示及显示/隐藏使用 `orderFrontRegardless` / `orderOut`；窗口配置为无阴影、置顶并加入桌面 Spaces。鼠标穿透由 winit 的 `set_cursor_hittest` 控制，径向菜单的透明间隙也按圆形区域轮询穿透。
- 配置、模型与日志位于 `~/Library/Application Support/BandoriPetLite/`，缓存位于 `~/Library/Caches/BandoriPetLite/cache/`。`BANDORI_PET_ROOT` 只覆盖数据目录，设置进程仍从桌宠可执行文件旁启动。内置目录从 `.app/Contents/Resources/assets/` 读取；数据目录的 `assets/` 可覆盖它。
- `flock` 提供用户级单实例保护；内核在进程崩溃后释放锁。IPC 使用 `/tmp/bandori-pet-<uid>/` 下的 Unix socket，限制目录权限，并在确认没有监听者后清理崩溃残留的 socket。
- 登录启动写入用户的 `Library/LaunchAgents/io.github.bandori-pet-lite.plist`，保留带空格的可执行文件路径和数据目录覆盖，在下一次登录时生效。取消登录启动删除该文件。
- 本地时间通过系统 `localtime_r` 获取，遵循用户时区和夏令时；测试环境变量仍可覆盖时间。

## 验证范围

Windows 本机检查不能证明 AppKit / Metal 的运行行为。macOS CI 分别在 Apple Silicon 和 Intel 上执行编译、Clippy、常规测试、文档测试及应用打包与签名校验。真实模型、GPU 和交互测试仍需真机执行；现有 Windows 专用的 `#[ignore]` GUI 测试不会在 macOS 上运行。

真机发布前应确认：

1. 将应用移动到「应用程序」后双击启动；首次无模型时能打开下载页，菜单栏入口可用。
2. moc 与 moc3 模型可加载，透明区域不出现底色；点击模型和拖动有效，透明区域能点击下面的应用。
3. 径向菜单可操作，透明间隙可穿透，点击外部会关闭；设置关闭后可从菜单栏重新打开。
4. Retina 与普通外接屏之间拖动、缩放和位置保存正确；重启恢复位置，拔掉外接屏后仍能看到角色。混合 DPI 屏幕尤其需要实机验证。
5. 显示/隐藏、切换桌面 Spaces、全屏应用及 Stage Manager 下的焦点与置顶行为符合预期。窗口点击后的焦点行为仍由 winit/AppKit 决定。
6. 重复启动不会出现第二个配置写入进程；强制退出后能再次启动并连接设置界面。
7. 启用登录启动后重新登录可启动，取消后重新登录不再启动；从菜单栏退出会同时结束设置进程。

## 平台差异

「OBS 采集兼容」仅修改 Windows 的原生窗口样式，在 macOS 上无效果。窗口捕获权限及全屏/Stage Manager 的最终叠放规则由 macOS 控制。脚本生成的是临时签名包；正式下载发布需要 Developer ID 签名及 Apple 公证。

相关原生接口：[winit macOS 扩展](https://docs.rs/winit/0.30.13/winit/platform/macos/index.html)、[NSWindow](https://developer.apple.com/documentation/appkit/nswindow)、[LaunchAgents](https://developer.apple.com/library/archive/documentation/MacOSX/Conceptual/BPSystemStartup/Chapters/CreatingLaunchdJobs.html)。
