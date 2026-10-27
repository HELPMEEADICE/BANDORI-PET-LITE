# 无损内存优化与复测

优化不修改 FPS、VSync、画质档位、渲染目标尺寸、贴图尺寸、采样方式、mask 尺寸或动画浮点精度。

## 改动

- 原来 `Role` 为点击检测深拷贝运行时的完整 RGBA 贴图。现在直接上传原始 RGBA，随后取走运行时的 CPU 贴图，逐张提取原分辨率的 8 位 alpha 并释放彩色数据。快照共享 alpha，不再保留两份 RGBA。alpha 不缩放、不二值化，配置阈值和半透明命中保持原样。创建与换装均使用这条路径。
- Neocari 的网格和变形器原来分别解码并保留 moc3 第 71 节的全部关键帧坐标。现在共享同一份不可变 `Arc<Vec<f32>>`，没有坐标压缩或精度变化。本次爱音模型少保留 19,487,040 字节，约 18.6 MiB。
- PNG 转 RGBA 时用消费式 `into_rgba8()`，原图已是 RGBA 时转移像素所有权，省去一次复制；moc3 原始文件解析结束后、PNG 解码之前释放，降低加载峰值。
- wgpu 设备使用 `MemoryHints::MemoryUsage`，减少分配器预留的空闲块。此选项不修改渲染资源规格。多角色仍共享设备。

此模型两张 2048×2048 贴图的 CPU 常驻量由两份 RGBA 共 64 MiB 变为 alpha 共 8 MiB，贴图与坐标合计确定性减少约 74.6 MiB。GPU 中仍保存完整原图。

## 实测

Windows、RTX 5080 Laptop、Vulkan、release 构建，爱音 `school_winter_hs_1st` 单角色。同一原始配置：120 FPS、VSync 开、balanced 原生分辨率、缩放 100%；窗口与渲染目标均为 930×1376。测量仅包括 `bandori-pet`，隔离目录不启动设置进程。

每个进程运行 16 秒，每 500 ms 采样，取第 5 秒之后的中位数。连续对照复测：

| 指标 | 原版 | 优化后 | 减少 |
|---|---:|---:|---:|
| 工作集 Working Set | 316.68 MiB | 242.17 MiB | 74.51 MiB，23.5% |
| 私有提交 Private Bytes | 549.98 MiB | 341.82 MiB | 208.16 MiB，37.8% |
| 采样到的峰值工作集 | 321.35 MiB | 262.21 MiB | 59.14 MiB，18.4% |

Private Bytes 是私有提交量，不等于全部驻留物理内存；工作集包含共享页面。初轮工作集为 366.37→244.80 MiB，受驱动缓存和系统页面驻留影响；上表采用后续连续对照结果。不同显卡、驱动、模型、动作缓存和角色数量会改变总量，不承诺所有 moc3 均低于 200 MiB。

GPU 分配策略另以相同模型、1395×2064 原生分辨率、不限帧、同一待机动作检查吞吐：Performance 185.9 FPS，MemoryUsage 242.8 FPS。两种策略均完成完整渲染且未触发熔断。这是一次本机回归，缓存和测量顺序会影响结果，不能视为普遍提速比例。

## 复现与回归

先关闭其他正在运行的桌宠。测量脚本使用独立配置和模型硬链接，保留逐次采样与日志，不改变源配置。原版 exe 需在重新构建前自行保存。

```powershell
pwsh -File scripts/measure-memory.ps1 -Executable C:\path\to\baseline.exe -Label baseline
pwsh -File scripts/measure-memory.ps1 -Executable .\target\release\bandori-pet.exe -Label optimized

cargo test --workspace
$env:BANDORI_PET_TEST_MODELS = "$PWD\run\models"
$env:BANDORI_PET_TEST_ROOT = "$PWD\run"
cargo test -p pet-app --test model_framing -- --nocapture
cargo test -p pet-app --test multi_role_runtime --test window_input -- --ignored
cargo test --release -p pet-app --test memory_throughput -- --ignored --nocapture

# 子模块测试：给出真实模型中 .moc3 文件的路径
$env:NEOCARI_TEST_MOC3 = 'C:\path\to\model.moc3'
cargo test --manifest-path vendor/neocari/Cargo.toml --lib shared_positions_match_the_standalone_parser -- --ignored --nocapture
```

新增测试覆盖全部 256 种 alpha、256 个阈值、多档 drawable opacity、UV 边界及采样，验证命中结果一致。真实 moc/moc3 回归在取走贴图后遍历动作；共享坐标与独立解析器逐个浮点值完全相同。原生窗口回归覆盖多角色增删、换装、配置恢复、透明区穿透与缩放。

本次同时修改主仓库与 `vendor/neocari` 子模块源码；保存或提交改动时需一并保留子模块修改。

## 第二轮：moc 固定开销与动态缓冲区（2026-10-27）

以下结果以本轮开始时的版本为基线，已经包含上面的第一轮优化，不能将两轮数字直接相加。

- Windows 原来在选择 Vulkan 显卡时，还初始化 DX12/GL 并枚举随后丢弃的适配器。现在先尝试 Vulkan；只有找到原有排序中优先级最高的独显（软件模式则为 CPU）才提前结束。其他设备类型、后端不可用、窗口不兼容或设备创建失败仍回到完整后端搜索。保留原来的显卡优先级、设备功能和渲染配置。本机最大收益来自不初始化未使用后端及其驱动资源。
- 动画网格共用一块顶点缓冲区和一块索引缓冲区，按原始索引、顶点偏移绘制。保留每个网格的变动检测、更新、排序、遮罩和浮点精度。超过单块缓冲区上限时仍支持逐网格分配。
- CPU alpha 使用 16×16 无损块存储：同值块只记录原始 alpha 值，其他块保留全部原始字节，采样直接寻址。压缩无收益时使用原始 alpha。香澄的 alpha 从 1,048,576 降到 549,632 字节；爱音从 8,388,608 降到 2,994,944 字节。GPU RGBA 贴图未修改。
- 复用最终合成的 bind group，尺寸未变时复用中间纹理，避免每帧或重复 resize 时创建相同资源。

同一台 Windows / RTX 5080 Laptop / Vulkan，release；120 FPS、VSync 开、balanced 原生分辨率，`live2d_scale=0`（本机 DPR 对应 150%）。香澄 `live_default` 为 1410×1477，爱音 `school_winter_hs_1st` 为 1395×2064，前后日志中的窗口与渲染尺寸一致。每次运行 24 秒、每 500 ms 采样，取第 5 秒后的中位数；每次一个角色，只统计桌宠进程。

| 模型 | 指标 | 本轮基线 | 本轮优化后 | 减少 |
|---|---|---:|---:|---:|
| 香澄 `.moc` | 工作集 | 218.54 MiB | 182.93 MiB | 35.61 MiB（16.3%） |
| 香澄 `.moc` | 私有提交 | 323.41 MiB | 305.77 MiB | 17.64 MiB |
| 香澄 `.moc` | 峰值工作集 | 226.14 MiB | 191.84 MiB | 34.30 MiB |
| 爱音 `.moc3` | 工作集 | 253.93 MiB | 213.15 MiB | 40.78 MiB（16.1%） |
| 爱音 `.moc3` | 私有提交 | 420.91 MiB | 397.26 MiB | 23.65 MiB |
| 爱音 `.moc3` | 峰值工作集 | 275.15 MiB | 239.51 MiB | 35.64 MiB |

香澄另一次连续对照为 218.41→183.38 MiB。逐次采样、配置和日志保存在 `run/memory-qa/*-verify-*/`，以及 `moc-before-final-*` / `moc-after-backend-*`。工作集包含共享页面，驱动、窗口大小和系统驻留状况会影响结果，本轮没有承诺所有模型低于 150 MiB。

回归包括原有 workspace 测试、全部 256 种 alpha 的保真检查、混合块/边缘块/不可压缩数据，以及真实 GPU 上与旧逐网格分配路径的像素逐字节对比。GPU 对比覆盖多网格偏移、非四字节对齐的索引范围、透明度、UV、排序、遮罩以及隐藏后重新显示。

真实 moc/moc3 的动作遍历、多角色增删、换装恢复、透明区点击穿透也均通过。同一份本轮代码在原生分辨率、不限帧的吞吐检查中，全后端初始化 / 按需初始化分别为：香澄 586.2 / 608.0 FPS，爱音 260.1 / 262.4 FPS，未观察到后端优化引起的吞吐下降。这只比较初始化方式，不是旧版完整程序与新版的提速比例；原始输出保存在 `run/memory-throughput.log`。

```powershell
cargo test --manifest-path vendor/neocari/Cargo.toml --features wgpu --lib packed_tests -- --include-ignored --nocapture
cargo test --manifest-path vendor/neocari/Cargo.toml --features wgpu --test wgpu_renderer
```
