# BANDORI-PET-LITE

桌面上站着一个 Live2D 角色：会呼吸、会看向鼠标、点哪动哪、能换装能缩放；右键一个圆盘菜单，托盘一个设置入口，所有参数都在一个四页的设置窗里改。

实现规格见 `../BANDORI-PET-REV/docs/BANDORI-PET-LITE.md`（下称「规格」）。本仓库是该规格的 Rust 实现：模型解析与绘制全部交给纯 Rust 的 [Neocari](https://github.com/HELPMEEADICE/Neocari)（Cubism 2 `.moc` + Cubism 3/4/5 `.moc3`，内置 wgpu 后端），不引入 LuaJIT、不引入 Cubism Core。

```
bandori-pet.exe   常驻：桌宠窗口（每角色一个）、wgpu surface、托盘、config.json 唯一写者
bandori-ui.exe    常驻隐藏：Slint 设置窗（4 页）+ 径向菜单 + 戳一戳气泡
```

两个进程用命名管道交换 `CONFIG_SNAPSHOT` / `SETTINGS_PATCH` 等消息，见 `crates/pet-ipc`。

## 已实现

| 规格 | 实现 | 位置 |
|---|---|---|
| §2.1 窗口：`WS_POPUP` + 透明 + 不抢焦点 + 不进任务栏 + Win11 去圆角/去白边 | winit 建窗 + `SetWindowLongPtr`/`DwmSetWindowAttribute` | `crates/pet-window/src/win32.rs` |
| §2.1 DPI：per-monitor v2，尺寸按内容 AABB × `live2d_scale%` | 内容包围盒 → 物理尺寸 → 拟合矩阵 | `crates/pet-window/src/sizing.rs` |
| §2.2 `.zst` 归档 → 解压缓存 → Neocari 路径制加载；LRU 8 套；拒绝 `..`/绝对路径/软链 | zstd+tar 流式解压，按归档名+服装落地 | `crates/pet-assets` |
| §2.2 moc / moc3 双入口 | `ModelHandle::{Moc2,Moc3}` 适配器 | `crates/pet-live2d` |
| §2.3 mask pass + main pass，预乘 alpha，`PreMultiplied` surface | Neocari 管线 + 中间纹理 blit（画质缩放 + 不透明度） | `crates/pet-render` |
| §2.3 画质档位 = 渲染分辨率缩放（省电 0.75×） | 中间 target 缩小后线性放大 | `crates/pet-render` |
| §2.3 熔断：连续 3 帧失败停渲染 | `FAILURE_FUSE = 3` | `crates/pet-render` |
| §2.4 CPU 命中：AABB → 三角形 → 纹理 alpha>8；120ms/12px 稳定性补偿 | 纯 CPU，零 GPU 回读 | `crates/pet-input/src/hit.rs` |
| §2.4 穿透：动态 `WS_EX_TRANSPARENT`，16ms/125ms 双档轮询，80ms/12px 补偿 | 轮询 + 样式切换 | `crates/pet-app`、`crates/pet-window` |
| §2.5 拖拽（<4px 视为单击）、单击、双击 poke、右键径向菜单、托盘 | 全局坐标拖拽（避免窗口移动回授） | `crates/pet-app/src/role.rs` |
| §2.6 7 区判定 + 4 级名称匹配 + 7 套预设 + 标签→表情表 + `""`/`__random__`/`__none__` | 逐条复刻自 Python 原版（含 `thinking` 候选扩展与 `f01` 兜底） | `crates/pet-input/src/{region,matching}.rs`、`crates/pet-config/src/presets.rs` |
| §2.7 待机/随机/时段/靠近/对视/poke/情绪位移 | 5s tick、180s 冷却、7min 发呆、18min 睡眠、600px 视线上限、6 种窗口位移 | `crates/pet-behavior` |
| §2.8 多角色：一模型一窗口、共享 GPU、一起拖动、位置 14 字段持久化 | `models[]` → 多 `Role` | `crates/pet-app`、`crates/pet-window/src/placement.rs` |
| §3 设置窗 4 页 + 生效分级（Instant/ReloadModel/RebuildWindow/RestartApp） | Slint + 补丁路由 | `crates/pet-ui`、`crates/pet-config/src/patch.rs` |
| §3.6 单写者、原子写、校验回落、未知键原样回写、首启动开设置页 | `serde_json::Value` overlay + 临时文件 + `rename` | `crates/pet-config/src/store.rs` |
| §5 IPC | 命名管道 + 长度前缀 JSON | `crates/pet-ipc` |

尚未完成/替代实现（都在 `docs/ACCEPTANCE.md` 里写明）：径向菜单「点击外部关闭」、Slint 窗口的逐像素透明、`WM_NCHITTEST → HTTRANSPARENT` 安全网、像素模式、i18n 仅中/英两套字符串、点击动作配置的导入/导出 UI。

## 目录

```
Cargo.toml                  workspace（10 个 crate + vendor/neocari）
crates/
  pet-config/               配置 schema、默认值、校验、原子写、补丁分级、预设标签表
  pet-assets/               .zst 读取、解压缓存（LRU）、模型索引、band/outfit 目录
  pet-live2d/               Neocari 适配：moc2/moc3、moc3 的 motion/expression/眨眼/呼吸装配
  pet-render/               wgpu 设备、surface、mask+main+blit 三 pass、画质与不透明度
  pet-window/               Win32 样式/穿透/DWM、显示器枚举、位置持久化、自启动
  pet-input/                CPU 命中、区域判定、名称匹配、AUTO 桶、预设展开
  pet-behavior/             待机/随机/时段/视线/对视/poke/情绪位移
  pet-ipc/                  命名管道协议
  pet-ui/                   Slint 设置窗 + 径向菜单 + 气泡（`ui/settings.slint`）
  pet-app/                  事件循环、角色窗口、托盘、IPC 宿主、UI 进程守护、两个 bin
  pet-util/                 RNG、缓动、时段分档
vendor/neocari/             Neocari @ ba9de37 的 vendored 源码（见 PATCHES.md）
assets/                     band.json、outfit.json（角色/服装中文名）
docs/ACCEPTANCE.md          规格 §7 验收清单 × 实测证据
run/                        本地联调目录（可随时删；scripts/run-dev.ps1 会重建）
```

## 构建

依赖：Rust 1.85+、Windows 10/11、可用 GPU（Vulkan/DX12，wgpu 30）。不需要 SDL2/LuaJIT/C++ 工具链。

```powershell
cargo build --release --bin bandori-pet --bin bandori-ui
cargo test --workspace          # 114 个单元/集成测试
```

产物：`target/release/bandori-pet.exe`、`target/release/bandori-ui.exe`。

## 运行

1. 建一个运行目录，把两个 exe 放进去；
2. `models/` 放 `kasumi.zst` 等归档（格式：tar.zst，每个服装一个顶层目录，含 `model.json` 或 `*.model3.json`）；
3. `assets/` 放 `band.json`、`outfit.json`；
4. 直接运行 `bandori-pet.exe`：首次启动没有 `config.json` 时会自动挑第一个可用角色并写默认配置，同时拉起隐藏的 `bandori-ui.exe`。

开发联调脚本（会从已有的 Bandori 资源目录拷贝 3 个归档并生成 `run/`）：

```powershell
pwsh -File scripts/run-dev.ps1 -ModelsFrom C:\oss\BANDORI-PET-REV\models
$env:BANDORI_PET_ROOT = "$PWD\run"; .\run\bandori-pet.exe
```

环境变量：`BANDORI_PET_ROOT`（安装根目录）、`BANDORI_PET_EXIT_AFTER_SECS`（N 秒后自动退出，CI 用）、`BANDORI_PET_HEADLESS`（不建角色窗口）、`RUST_LOG`。

## 选队、选角与换装

设置的「切换角色/服装」和「添加 Live2D 模型」进入三级选择界面：乐队 Logo 卡片 → 角色立绘卡片 → 服装列表。仅展示当前 `models/` 中已安装的角色；顺序与参考项目一致。选择服装后立即切换并回到详情页，添加模型时保留其他已配置角色。

角色可按名称、乐队、key 搜索，服装可按名称、id 搜索；两页均支持最近使用和收藏筛选。服装右侧的眼睛按钮固定/关闭静态预览，按住 Shift 悬停可临时预览。预览在后台离屏生成，不创建桌面窗口。右侧设置通过「应用」保存，收藏与最近使用随配置保存。

「导入自定义模型」支持填写 `.zst` 模型归档的完整路径，导入后立即刷新选择列表；同名归档不会被覆盖。

## 关键实现说明

**冷启动只做一件事：出画面。** 启动时若 `config.models` 已有角色，就不扫描 `models/*.zst`（模型索引改为设置窗首次需要时惰性扫描），托盘图标也在首帧之后再创建。release 实测 `role ready 1.10s`，首帧可见 1.54s（含建进程开销）。

**只有 pet 写配置。** 设置窗发出 `SETTINGS_PATCH`（键可为点路径，如 `click_motion_actions.head.motion`），pet 反序列化合并后的完整文档 → 校验/夹紧 → 原子写 → 按最高生效级别执行 → 回 `PATCH_APPLIED` + 新快照。未知键（例如老版本写的 LLM 配置）留在 `extras` 原样回写，不会被吞掉。

**帧序固定**（抄 Neocari `examples/show_model.rs`）：mask pass → main pass（`LoadOp::Clear(alpha=0)`）→ blit 到 swapchain。moc3 的每帧顺序是 `reset_parameters → reset_part_opacities → 覆盖参数（视线/眨眼/呼吸/口型）→ motion tick/apply → expression tick/apply → apply_parameter_overrides → apply_physics → apply_pose → update_meshes`；moc 全部塞在 `Moc2RuntimeModel::update(dt)` 里。

**Y 轴方向按格式区分。** Cubism 2 的 `.moc` 坐标是画布像素、原点在左上、y 向下；Cubism 3/4/5 是 y 向上的模型空间。Neocari 的 `moc2::layout` 和 `render_moc2` 示例都写明了这一点，所以 `fit_matrix(..., flip_y)` 对 moc 取负的 y 缩放、对 moc3 保持正向。判错的后果是角色上下颠倒，且 7 区点击区域一起错。回归测试 `crates/pet-app/tests/orientation.rs` 会检查真实模型落在哪种坐标系里。

**命中是 CPU 算的。** 逆变换把光标映射回模型空间 → AABB → 三角形重心坐标 → 用插值 UV 在内存里的 PNG RGBA 上取 alpha，`> threshold` 才算命中；120ms/12px 内保持上一次命中，避免动画帧让窗口在「可点」和「穿透」之间抖。因此交互完全不碰 GPU 回读。

**区域比例在 clip 空间算。** 7 区判定用「模型点 → 同一个矩阵 → clip → 比例」，这样 y 向上/向下两种模型空间都不用分支，moc 翻转后 `head` 仍然落在窗口顶部。

**穿透轮询与渲染解耦。** 主线程既渲染又轮询（16ms 近 / 125ms 远），每帧只做一次 AABB 扫描，成本远小于一次 GPU 提交。

**换装是热重载。** 解压新服装 → 重建纹理/网格/裁剪资源 → 首帧，不重启进程；窗口尺寸按新模型内容包围盒重新计算，右下角锚定，避免跑出屏幕。

**拖拽用全局光标。** 用窗口内局部坐标算位移会形成回授（窗口一动，局部坐标就变小），实测只会跟上 60%；改成「按下时的屏幕坐标 + 当前屏幕坐标」后 1:1 跟手。

## 许可

- 本项目：GPL-3.0-only。
- `vendor/neocari`：Neocari，GPL-3.0-only（与本项目同协议），改动见 `vendor/neocari/PATCHES.md`。
- Live2D 模型与角色素材版权属于 Craft Egg / Bushiroad，本仓库不包含任何模型文件。


## 参考资料

- `docs/reference/BANDORI-PET-REV-behavior-spec.md`：从 `BANDORI-PET-REV` 的 Python 源码里逐条挖出来的行为数据（7 区预设表、4 级名称匹配、标签→表情表、AUTO 桶、情绪位移关键帧、托盘/径向菜单几何、所有常量）。实现时按它对齐；两处与规格不同的实测结论（径向菜单无长按、视线死区是 4px）也记在里面。

