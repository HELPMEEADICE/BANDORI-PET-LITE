# 自动构建与发布安装包

[Build installers 工作流](../.github/workflows/release.yml) 构建两个应用进程及内置资源，不包含 Live2D 模型。

| 系统 | 架构 | 产物 |
| --- | --- | --- |
| Windows | x86_64、aarch64 | `.msi`、`.zip`、`-setup.exe`（Inno Setup 6） |
| Linux | x86_64、aarch64、riscv64（RV64GC） | `.AppImage`、`.deb`、`.rpm`、`.tar.gz` |
| macOS | aarch64、x86_64 | 包含 `.app` 的 `.zip`、带 Applications 快捷入口的 `.dmg` |

完整构建共生成 22 个安装包，文件名包含项目版本、系统和架构，不同平台之间不会覆盖。所有安装包均包含 `bandori-pet` 和 `bandori-ui`，两者始终在同一目录。

## 手动构建和下载

1. 将工作流和打包文件提交、推送到仓库默认分支。
2. 打开仓库 **Actions → Build installers → Run workflow**，选择需要构建的分支或标签。
3. 构建完成后，在本次运行的 **Artifacts** 区域下载 `BANDORI-PET-LITE-<版本>-all-installers`。该归档包含全部 22 个安装包和 `SHA256SUMS.txt`。

也可以分别下载 `installers-windows-*`、`installers-linux-*`、`installers-macos-*`。各平台完成后即上传各自的产物；如果其他平台失败，已经成功上传的产物仍然可以下载。产物保留 30 天，手动触发只生成 Actions artifacts。

## Release 自动发布

在 GitHub 上发布 Release 或预发布版本时，`release.published` 事件会从对应标签的源码构建，生成同样的 Actions artifacts，并将全部安装包及 SHA256 校验文件上传到该 Release。草稿保存不会触发构建。只有全部平台构建成功、确认 22 个安装包齐全后，才执行 Release 上传；重新运行会覆盖该 Release 中的同名文件。

版本号取自 `Cargo.toml` 的 `[workspace.package].version`，与应用本身版本保持一致。发布前更新版本并提交，再创建相应标签，例如 `v0.1.0`；标签所指向的提交必须包含工作流及打包文件。工作流也接受 `0.2.0-rc.1` 这样的预发布版本。MSI 使用其中的三段数字版本，受 Windows Installer 的 `255.255.65535` 上限约束；同版本重建和预发布转正式版允许覆盖升级。

不需要配置额外的发布密钥，收集上传任务使用 GitHub 提供的 `GITHUB_TOKEN`，仅该任务授予 `contents: write`。仓库或组织策略需要允许工作流使用这一权限。

## 构建方式和运行要求

- **Windows**：在 Windows x64 runner 上分别构建原生 x64、ARM64 MSVC 程序，并静态链接 MSVC CRT。MSI 由 WiX 4 生成并执行 MSI 校验；EXE 固定使用 SHA256 校验的 Inno Setup **6.4.3**。Inno 6 的安装引导程序自身为 x86，其 ARM64 安装包内的两个应用程序为原生 ARM64。两个安装器均按用户安装至 `%LOCALAPPDATA%\Programs\BANDORI-PET-LITE-<架构>`，应用可在其中保存配置、模型和日志，卸载不递归删除用户数据。同一安装请选择 MSI 或 EXE 中的一种，后续沿用同种格式升级。
- **Linux**：x86_64 与 aarch64 使用 Ubuntu 22.04 容器，glibc 基线为 2.35；RISC-V 使用 Debian 13 容器和 GCC 交叉工具链，glibc 基线为 2.41。交叉编译使用 RISC-V GTK/X11 开发库及专属 pkg-config 搜索路径。DEB/RPM 通过固定版本 nFPM 生成，架构及运行依赖写入包元数据。DEB 的 GTK 包名随构建环境选择普通版或 `t64` 版。
- **AppImage**：使用固定版本、SHA256 校验的 [uruntime](https://github.com/VHSgunzo/uruntime/tree/v0.8.1) 与 SquashFS，支持三个目标架构。打包脚本通过读取 ELF 依赖收集 GTK、托盘及其他动态库，并附上库的版权说明。glibc、动态加载器、显卡驱动仍由系统提供，glibc 最低版本与上述构建环境一致；需要 X11 和可用的图形驱动。原生构建任务还会实际提取 AppImage 并检查两个程序。没有 FUSE 时可以使用 `./文件名.AppImage --appimage-extract-and-run`。
- **tarball**：沿用 `scripts/package-linux.sh`，包含程序、内置资源及许可证；不捆绑系统动态库，依赖见 [Linux 支持说明](LINUX.md)。
- **macOS**：Apple Silicon 与 Intel 分别在原生 runner 上打包，最低系统版本为 macOS 12；复用现有 `.app` 脚本完成临时签名及签名验证，再生成 ZIP 和 DMG。当前未配置 Developer ID 签名或 Apple 公证。

Linux ARM64 使用 GitHub 托管的 `ubuntu-24.04-arm` runner。公开仓库可直接使用；私有仓库需要账号支持对应 ARM runner，或将该矩阵项换成自己的 ARM64 runner。RISC-V 在 x64 runner 中交叉构建，不要求 RISC-V runner，也不运行 RISC-V 程序；实际 GPU、托盘和桌面行为仍需在目标设备验收。

## 本地打包

Windows 安装 Rust MSVC 工具链、Visual Studio C++ 构建工具、WiX 4 和 Inno Setup 6 后：

```powershell
dotnet tool install --global wix --version 4.0.6
pwsh -File scripts/package-windows.ps1 -Version 0.1.0
# 已按对应 target 构建时，可以添加 -SkipBuild。
# INNO_SETUP_COMPILER 环境变量或 -InnoCompiler 可指定 Inno 6 的 ISCC.exe。
```

完整 Linux 发布包需要在对应构建环境安装开发依赖，以及 Python 3、binutils、SquashFS 工具、curl；脚本会下载并校验 nFPM 和 uruntime：

```bash
cargo build --release --locked --target x86_64-unknown-linux-gnu --bin bandori-pet --bin bandori-ui
bash scripts/package-linux-release.sh --target=x86_64-unknown-linux-gnu --version=0.1.0
```

Windows/Linux 产物分别位于 `dist/windows-<target>-release/packages/`、`dist/linux-<target>-release/packages/`。macOS 本地 `.app`/ZIP 打包仍使用 `bash scripts/package-macos.sh --target=<target>`；DMG 由工作流中的 `hdiutil` 步骤生成。
