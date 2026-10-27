# BANDORI-PET-LITE

把 BanG Dream! 的 Live2D 角色放到 Windows、macOS 或 Linux（X11）桌面上。你可以拖动角色、点击不同部位触发动作，也可以同时放上多个角色，给他们各自换装。

项目使用 Rust 和 [Neocari](https://github.com/HELPMEEADICE/Neocari) 构建，支持 Cubism 2 的 `.moc` 和 Cubism 3/4/5 的 `.moc3` 模型。本仓库不包含模型文件；可以在程序里下载，也可以导入已有的模型包。

## 开始使用

Windows 需要 Windows 10 或 11，以及支持 Vulkan 或 DirectX 12 的显卡。将 `bandori-pet.exe` 和 `bandori-ui.exe` 放在同一个目录，运行 `bandori-pet.exe` 即可。

macOS 需要 macOS 12 或更新版本和支持 Metal 的显卡，支持 Apple Silicon 与 Intel。将 `BANDORI-PET-LITE.app` 移到「应用程序」后打开，通过菜单栏图标进入设置或退出。源码打包的应用使用临时签名，没有 Apple 公证；若系统拦截，按系统提示在「系统设置 → 隐私与安全性」中允许打开。

Linux 暂时支持 X11，需要启用透明合成的桌面环境、支持 Vulkan 或 OpenGL 的显卡驱动，以及 GTK 3 和 AppIndicator 运行库。将 `bandori-pet` 和 `bandori-ui` 放在同一目录，运行 `./bandori-pet`；GNOME 需要 AppIndicator 扩展才能显示托盘入口。依赖安装、打包和验收范围见 [Linux 支持说明](docs/LINUX.md)。

首次启动时，如果没有找到模型，程序会打开「模型下载与更新」页面；下载喜欢的角色后就可以选择服装了。

也可以提前把模型包放进数据目录下的 `models/`（Windows 默认位于运行目录）。模型包为 `.zst` 格式的 tar 归档，每套服装占一个顶层目录，包含 `model.json` 或 `*.model3.json`。角色和服装名称使用内置目录；如有需要，也可以在数据目录的 `assets/` 下提供 `band.json` 和 `outfit.json`。

## 桌面上的操作

- 拖动角色可以调整位置；右键角色打开径向菜单，锁定后可以避免误拖。
- 点击角色的不同部位可以触发动作和表情。在「角色行为」中可以调整点击反馈、视线追踪和待机动作等设置。
- 从托盘打开设置，在「角色列表」里切换角色或服装，也可以添加更多桌面角色。每个角色的位置会单独保存，重启后恢复。
- 「显示设置」可以调整缩放、透明度和帧率等；右侧还有置顶、OBS 采集兼容、隐藏角色和开机自启动选项。

选择角色和服装时可以搜索、收藏和查看最近使用的项目。服装列表支持静态预览；已有的自定义 `.zst` 归档也可以从选择界面导入，同名归档不会被覆盖。

## 下载和更新模型

在设置中打开「模型下载与更新」，可以按角色、乐队或 key 搜索，单独下载或更新角色，也可以批量下载缺失的模型或更新已安装的模型。下载过程中可以查看进度和取消任务。

模型来自 ModelScope 的 `HELPMEEADICE/BanG-Dream-Live2D` 数据集。程序会先校验下载的归档，再替换已有文件；下载失败或取消时会保留原来的模型。完成后列表会自动刷新，正在使用的角色也会重新加载。

Windows 设置保存在运行目录的 `config.json` 中。macOS 的配置、模型和日志保存在 `~/Library/Application Support/BandoriPetLite/`，解压缓存保存在 `~/Library/Caches/BandoriPetLite/cache/`；应用包内只放程序和内置资源，升级应用不会覆盖这些数据。模型暂时缺失或加载失败时，程序会保留对应的角色配置，方便之后恢复。

Linux 的配置、模型和日志默认保存在 `~/.local/share/BandoriPetLite/`，缓存位于 `~/.cache/BandoriPetLite/cache/`，支持 `XDG_DATA_HOME` 和 `XDG_CACHE_HOME` 覆盖。`BANDORI_PET_ROOT` 可指定数据目录，两个可执行文件仍从同一目录启动。

## 从源码构建

需要 Rust 1.85 或更新版本。项目使用仓库内的 Neocari 源码，不需要 LuaJIT 或 Cubism Core。

```powershell
cargo build --release --bin bandori-pet --bin bandori-ui
cargo test --workspace
```

构建后的程序位于 `target/release/`。Windows 将两个 exe 放到同一个运行目录即可启动；其他平台的打包方式见下文。

### macOS 构建与打包

安装 Rust stable 和 Xcode 命令行工具（`xcode-select --install`），然后运行：

```bash
git submodule update --init --recursive
cargo build --release --bin bandori-pet --bin bandori-ui --locked
cargo test --workspace --locked
bash scripts/package-macos.sh --skip-build
```

脚本会在 `dist/macos-<架构>-release/` 中生成 `.app` 和可分享的 `.zip`；也可以直接运行 `bash scripts/package-macos.sh` 完成构建和打包。脚本包含两份可执行文件、内置目录、应用图标和许可文件，并执行临时签名及签名校验。正式对外发布时需要另外使用 Developer ID 签名并提交 Apple 公证。

开发时可直接运行 `cargo run --bin bandori-pet`，同时确保同一构建目录中已有 `bandori-ui`。`BANDORI_PET_ROOT` 可以覆盖数据目录，两个程序仍从同一可执行文件目录启动。macOS 的登录启动通过用户 `~/Library/LaunchAgents/io.github.bandori-pet-lite.plist` 配置，在下一次登录时生效；启用前应先将应用移动到固定位置。

macOS 使用 Metal、AppKit 窗口和菜单栏入口；「OBS 采集兼容」是 Windows 的窗口样式选项，在 macOS 上不改变行为。macOS 的真机验收项目见 [macOS 支持说明](docs/MACOS.md)。

### Windows 开发运行

如果本地已经有兼容的模型目录，可以用开发脚本准备 `run/` 并启动：

```powershell
pwsh -File scripts/run-dev.ps1 -ModelsFrom C:\path\to\models
$env:BANDORI_PET_ROOT = "$PWD\run"
.\run\bandori-pet.exe
```

脚本默认复制 `kasumi`、`anon` 和 `asuka` 的归档；可以通过 `-Characters` 指定其他角色，或使用 `-Release` 构建 release 版本。`BANDORI_PET_ROOT` 用于指定运行目录；开发和测试还可以使用 `RUST_LOG`、`BANDORI_PET_HEADLESS` 和 `BANDORI_PET_EXIT_AFTER_SECS`。

### Linux 构建与打包

安装 [Linux 支持说明](docs/LINUX.md#构建和运行) 中的开发依赖后运行：

```bash
git submodule update --init --recursive
cargo build --release --bin bandori-pet --bin bandori-ui --locked
cargo test --workspace --locked
bash scripts/package-linux.sh --skip-build
./target/release/bandori-pet
```

打包产物位于 `dist/linux-<target>-release/BANDORI-PET-LITE.tar.gz`，包含程序与内置资源。Linux 的登录自启动通过用户 XDG autostart `.desktop` 文件配置，在下次图形登录时生效。当前两个进程均选择 X11，不提供原生 Wayland 支持。

### 持续集成

每次 push、Pull Request 和手动触发都会运行 [GitHub Actions CI](.github/workflows/ci.yml)：

- Rust：`cargo fmt` 格式检查，以及 Windows 上整个 workspace 的 `cargo check`、Clippy（告警视为错误）、两个程序的构建、单元与集成测试、文档测试和文档生成检查。
- macOS：Apple Silicon（`macos-15`）和 Intel（`macos-15-intel`）分别执行 workspace 检查、Clippy、单元与集成测试、文档测试，以及 `.app` 构建、打包和签名校验；产物以 ZIP 上传到 Actions artifacts。
- Linux：Ubuntu 24.04 执行 workspace 检查、Clippy、构建、单元与集成测试和文档测试；通过 Xvfb 检查 X11 鼠标/穿透以及两个真实进程的启动、IPC 和退出，上传 debug tar.gz 供验证。
- Slint：使用与 `Cargo.lock` 中编译器版本一致的 `slint-lsp` 检查所有已跟踪 `.slint` 文件的语法与格式；Windows 构建还会通过 `pet-ui/build.rs` 检查导入、类型、绑定和资源。
- 工作流：使用 actionlint 检查 GitHub Actions 配置。

本地提交前可运行：

```powershell
cargo fmt --all -- --check
cargo check --workspace --all-targets --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo build --workspace --bins --locked
cargo test --workspace --all-targets --locked
cargo test --workspace --doc --locked
```

如需整理 Slint 格式，安装与锁文件版本一致的 `slint-lsp`，然后运行：

```powershell
$slintFiles = @(git ls-files -- '*.slint')
slint-lsp format -i @slintFiles
```

首次克隆后需要运行 `git submodule update --init --recursive` 拉取 Neocari；CI 会自动完成。需要本地模型、真实窗口或 GPU 的 `#[ignore]` 测试仍需手动运行。

## 项目结构与开发资料

桌宠和设置窗口分别运行在 `bandori-pet` 与 `bandori-ui` 两个进程中，通过本地 IPC 通信（Windows 命名管道、macOS/Linux Unix domain socket）。主要代码位于 `crates/`：`pet-app` 负责桌宠和托盘，`pet-ui` 负责设置界面，其他 crate 分别处理模型、渲染、交互、行为和配置。

- [验收清单](docs/ACCEPTANCE.md)：功能覆盖情况、验证方式与已知限制。
- [内存测量记录](docs/MEMORY.md)：优化前后的测量条件和结果。
- [行为参考](docs/reference/BANDORI-PET-REV-behavior-spec.md)：动作匹配、区域和行为参数的参考记录。

部分功能仍有局限：目前只提供中英文字符串，点击动作配置还没有导入和导出界面；其他实现差异见验收清单。

## 许可与素材

本项目采用 GPL-3.0-only 许可，详见 [LICENSE](LICENSE)。随仓库提供的 Neocari 同样采用 GPL-3.0-only，相关说明见 [NOTICE.md](NOTICE.md)。Live2D 模型与角色素材版权属于 Craft Egg / Bushiroad；本仓库不包含模型文件。
