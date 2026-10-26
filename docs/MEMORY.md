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
