# 规格 §7 验收清单 × 实测证据

对应 `../BANDORI-PET-REV/docs/BANDORI-PET-LITE.md` §7。每条给出「怎么验的」和「实测值」。
测试机器：Windows 11，RTX 5080 Laptop，Vulkan 后端，显示器 DPR 1.5，主屏工作区 2532×1536 物理像素。

| # | 验收项 | 结果 | 证据 |
|---|---|---|---|
| 1 | 冷启动 ≤1.5s 出画面 | ✅ | release：进程内 `role ready ms=1101`，外部轮询 `IsWindowVisible` 首帧可见 **1540ms**（含 PowerShell `Start-Process` 约 300–400ms 的建进程开销）。debug 版因 300MB 调试符号页入会多 1s 左右。首帧在 `Role::create` 内同步呈现后才 `set_visible(true)`；启动时不再扫描 64 个归档（改为 `ensure_catalog` 惰性扫描），托盘也延后到首帧之后创建。 |
| 2 | 多屏位置持久化：拖到别处→重启回到原位；拔掉副屏回主屏中央 | ✅（前半实测，后半单测） | 拖动后 `config.json`：`window_placement x=1862 y=699 w=410 h=750 screen=\\.\DISPLAY1 dpr=1.5`；重启日志 `decision=Absolute x=1862 y=699`，`GetWindowRect` 一致。换屏/分辨率变化走 `relative_x/y` 重算、显示器消失回落主屏中心：`pet-window/src/placement.rs` 的 4 个单测（`changed_geometry_uses_relative_position`、`missing_monitor_falls_back_to_primary` 等）。 |
| 3 | 按在头发上拖动跟手；划过透明区点到桌面 | ✅ | 原拖拽实测窗口位移 (120,−40) 与注入位移一致。新增 `window_input` 真实窗口回归对 moc/moc3 均验证 `WindowFromPoint` 在透明区命中下层窗口、移到模型上命中桌宠，再回透明区命中下层；拖拽经过透明区保持接收鼠标，松手后恢复穿透。 |
| 4 | 缩放 25% / 500% 后窗口重建、命中仍准 | ⚠️ 部分 | 尺寸推导与命中都在同一内容包围盒/同一矩阵上做（`fit_matrix` 与 `hit_test` 共用 `self.matrix`），`sizing.rs` 有 25%/500% 尺寸单测；两个极端值的联调未跑（会占满屏幕）。 |
| 5 | 7 区分别配动作、逐区点击；`不触发` 区无反应 | ✅ | 7 区判定与预设展开有 22 个单测（含阈值 0.38/0.64/0.62、`__none__` 直接返回空命令、`genki`/`tsundere` 展开结果）。联调点击窗口 10% / 50% / 90% 高度：日志分别是 `region="head"` / `"upper_body_left"` / `"lower_body_center"`，动作按 AUTO 桶命中 `surprised01` / `shame01`。 |
| 6 | 双击专用反馈（已移除） | — | 双击现在按两次普通点击处理；戳一戳动作、抖动、气泡及设置入口均已删除。旧配置中的 `poke_motion` / `poke_expression` 在加载时丢弃。 |
| 7 | 换角色 ≤1s 热重载，位置缩放保持 | ✅ | 双角色启动日志：kasumi `ms=1025`、anon（moc3，复用同一 GPU）`ms=145`；`swap_model` 不重建窗口，位置取自 `window_placement`。 |
| 8 | 画质切省电 → 渲染分辨率 0.75× | ✅ | `SceneRenderer::render_size()` 在 0.75× 时返回 `(307,562)`（410×750 的 0.75）；`scale_dim` 单测覆盖 0.75/1.0/0.25。 |
| 9 | 改 vsync / OBS 兼容后仍透明、仍能穿透 | ✅（OBS 原生窗口回归；vsync 重配未另做联调） | `apply_obs_capture` 更新样式子类回调的 OBS 标志，切换 `WS_EX_TOOLWINDOW ↔ WS_EX_APPWINDOW`，保留 `NOACTIVATE/LAYERED` 并通过 winit 隐藏任务栏按钮。`window_input` 验证切换后穿透仍可开关；vsync 走 `Surface::reconfigure`。 |
| 10 | 删掉 config.json 启动 → 扫描模型并打开设置 + 横幅，不崩 | ✅ | 无可用模型时发送 `OPEN_SETTINGS{page:Downloads}`，已有模型时发送 `page:Model`；UI 的 `first-run` 横幅给出相应指引。`missing_or_invalid_models_route_to_downloads_and_installed_models_to_picker` 覆盖缺失、损坏和安装后刷新。 |

新增模型下载与更新验证：下载源与 BANDORI-PET-REV 一致；`pet-assets::download` 的本地 HTTP 回归覆盖下载、覆盖更新、404、空响应、长度不符、归档截断、取消及临时文件清理。联网测试 `downloads_public_model_and_discovers_its_costumes` 已实际下载并校验 kasumi 模型包，发现 117 套服装。下载页的 Slint 回归覆盖首次安装目录、乐队/角色搜索、下载回调，以及 1080×620 / 1180×680 / 150% DPI 下的按钮布局；明暗主题截图位于 `run/download-qa/`。

## 其它硬指标

| 指标 | 实测 |
|---|---|
| 透明是真的逐像素 | 同一块 410×750 屏幕区域，宠物在/不在两张截图：四角 40×40 像素 **diff=0**；模型区域 **45.2%** 像素变化（均值 RGB 196,160,152 = 肤色）。moc3（258×750）同样：四角 diff=0，40.4% 变化。 |
| 渲染后端 | `adapter=NVIDIA GeForce RTX 5080 Laptop GPU backend=Vulkan`，`surface format=Bgra8Unorm alpha=PreMultiplied present=Fifo`。 |
| moc / moc3 都能跑 | kasumi(moc): 28 动作 / 18 表情；anon(moc3): 66 动作组 / 36 表情，实际播放 `mtn_smile01_R`（标签 `smile` 正确命中 moc3 命名）。 |
| 解压缓存 | 首次解压 kasumi/live_default 112ms（tar.zst 流式 + 只落该服装目录），moc3 服装 651ms；缓存命中不再解压。 |
| 双角色 | 两窗口同屏，第二个角色复用首个 `Arc<Gpu>`（只出现一行 `renderer initialised`），创建 145ms。 |
| 测试 | `cargo test --workspace` → **119 passed, 0 failed, 1 ignored**（含 5 个配置/IPC 集成测试、22 个命中与匹配单测、8 个窗口位置与尺寸单测；ignored 的是需要本地模型 + GPU 的离屏预览测试）。 |
| 设置窗 | `bandori-ui` 通过命名管道连上 pet（日志 `settings process connected`），窗口 `responsive=True`（`WM_NULL` + `SendMessageTimeout` 2s 内应答），管道往返 `<100ms`。 |
| 崩溃守护 | 杀掉 `bandori-ui` 后：`settings process disconnected` → `restarting settings process attempt=1` → 新进程 `pid=16368`，从断开到重新连上 **~130ms**（3s 节流 + 最多 3 次，防重启风暴）。pet 退出时先原子写 config 再广播 `QUIT`。 |

## 已知偏差

1. **`WM_NCHITTEST → HTTRANSPARENT` 安全网未做**：它只支持同线程窗口间转发，桌宠的跨进程穿透由 `WS_EX_LAYERED | WS_EX_TRANSPARENT` 控制，径向菜单用 `SetWindowRgn` 排除按钮以外的区域。窗口使用 `SetWindowSubclass` 保留必要样式并处理 `WM_MOUSEACTIVATE`，其余消息交回 winit。
2. **渲染在主线程**：规格建议 main/render/io 三线程；实现是「事件循环里 `WaitUntil(1/fps)` 节流 + 同线程渲染」，命中是 CPU 计算不阻塞 GPU 提交，实测 60fps 稳定。要拆线程时 `FrameSnapshot` 已经是不可变快照，接口不用改。
3. **像素模式、i18n 完整化、点击动作配置的导入/导出 UI** 未做（规格标明像素模式 v0 不做 UI；i18n 目前是中/英两套硬编码字符串）。
4. **`live2d_active_profile`（自定义命名的 profile 保存/选择）** 只支持内置 7 套预设展开；自定义 profile 的数据结构（`click_motion_profiles`）已进配置契约，UI 未提供保存入口。

## 复现方式

```powershell
cargo test --workspace                                  # 144 个测试（另有 6 个 ignored）
pwsh -File scripts/run-dev.ps1 -ModelsFrom <你的 models 目录>
$env:BANDORI_PET_ROOT = "$PWD\run"; $env:RUST_LOG = "info,pet_app=debug"
.\run\bandori-pet.exe
```

* 冷启动/热重载耗时：看日志 `role ready ms=…`。
* 透明与绘制：宠物运行前后对同一屏幕矩形各截一张图做像素差（四角应为 0，模型区域应显著变化）。
* 穿透：`GetWindowLongPtrW(hwnd, GWL_EXSTYLE) & 0x20` 随光标在模型/透明区移动而切换。
* 位置持久化：拖动后等 2s 让防抖落盘，读 `config.json` 的 `models[0].window_placement`，重启后看日志 `decision=Absolute`。


## 修复记录

### 2026-10-26 · Cubism 5 服装的空裁剪槽（-1）让整件服装加载失败

现象：切到 `hotaru` / `casual_spring_01` 时角色窗口建不起来，设置窗收到：

```
无法加载角色 hotaru 的服装 casual_spring_01：render error: clipping setup failed: InvalidMaskDrawableIndex { drawable_index: -1 }
```

根因：这件服装是 Cubism 5 的 `.moc3`（版本 5，356 个 art mesh）。被多重遮罩裁剪的三个 drawable 的 mask 列表带着 `-1` 空槽（导出/转换留下的占位），全模型 100 个 mask 槽里有 9 个 `-1`：

| drawable | 原始 mask 槽 | 有效槽 |
|---|---|---|
| ArtMesh549 | `[-1,-1,68,-1,-1,105,-1,-1,90,91,92,93,94]` | `[68,105,90,91,92,93,94]` |
| ArtMesh572 | `[68,-1,105,-1,90,91,92,93,94]` | `[68,105,90,91,92,93,94]` |
| ArtMesh433 | `[-1,189]` | `[189]` |

`vendor/neocari` 把这串下标原样交给裁剪计划，`create_clipping_resources` 用 `usize::try_from(-1)` 转换失败，整件服装在 `ModelGpu::new` 阶段被拒绝。只有这一件服装受影响：同一档案的 `live_01`、`child_casual_spring_01_still` 没有空槽。

参考实现（`../Project/Rust/Live2D-v2-Lua/live2d/cubism3/opengl_renderer.lua` 的 `draw_clipped_mesh_group`）用 `meshes[mask_idx + 1]` 取遮罩网格：`-1` 落到 Lua 的 0 下标得到 `nil`，被直接跳过，其余遮罩照常进 stencil。

修复（`vendor/neocari`）：

* 新增 `moc3::drawable::renderable_masks()`：丢掉负数槽与 `>= drawable 数` 的槽，保留原顺序；
* `build_moc3_drawable_mesh` 用它清洗 `art_mesh_masks`；只剩空槽的 drawable 变成无遮罩（空列表），不再进入裁剪计划；
* `Moc3DrawableMesh::from_parts_with_render_order` 同样丢负数槽，手工建网格也不会把 `-1` 带进裁剪；
* 全部下标有效的模型，mask 列表逐项不变；moc2 路径未改。

验证：

* `moc3::drawable::tests::drawable_mesh_drops_negative_mask_slots`、`moc3_art_meshes::moc3_drawable_mesh::drops_empty_and_out_of_range_mask_slots` 锁定 `-1`/越界槽被丢弃、空槽 drawable 渲染为无遮罩；
* `cargo test --workspace`：144 passed、0 failed、6 ignored；`vendor/neocari` 自身 `cargo test`：192 passed、0 failed；
* `hotaru` / `casual_spring_01` 离屏预览（384×768，RTX 5080 / Vulkan）正常出图：可见像素 164,165（55.7%），包围盒 (14,21)–(370,747)，四角 alpha=0；修复前同一路径直接返回上述错误；
* `bandori-pet.exe` 实机（`target/release/config.json` 原本就停在 `hotaru` / `casual_spring_01`）：`role ready ms≈1.2s format=Moc3 motions=66 expressions=46`，259×750 窗口，两次 6–8 秒运行均无 `frame failed`；同一档案的另外两套服装（`live_01`、`child_casual_spring_01_still`）的 moc3 没有空槽。

### 2026-10-26 · 透明区阻挡鼠标、任务栏按钮与桌宠夺焦

根因：winit 在可见性、鼠标命中和窗口层级等标志变化时，根据内部缓存重写整个 `GWL_EXSTYLE`，覆盖此前直接写入的 `WS_EX_NOACTIVATE`、`WS_EX_TOOLWINDOW` 和 `WS_EX_LAYERED`。原穿透代码只改原生样式，且缓存布尔值会阻止再次修正。两个二进制默认使用控制台子系统，独立启动还会产生控制台窗口。

修复：

* 用 `Window::set_cursor_hittest` 同步 winit 的穿透标志；用 `SetWindowSubclass` 在 `WM_STYLECHANGING` 中保留必要原生样式，在 `WM_MOUSEACTIVATE` 中返回 `MA_NOACTIVATE`，销毁时移除子类回调。
* 创建窗口时即设置 `with_skip_taskbar(true)`，显示/恢复及 OBS 切换时保持任务栏隐藏；两个 exe 改为 Windows GUI 子系统。
* 首帧呈现后才按全局隐藏设置和角色隐藏状态决定是否显示，移除后续无条件显示。近距离轮询依据全局鼠标位置，穿透状态下也能及时恢复模型交互。
* 移除右键菜单打开后 10 秒内强制所有角色不穿透的逻辑；拖拽期间保持鼠标接收，锁定拖拽时透明区域继续穿透。

验证：

* `cargo test --workspace`：130 passed、0 failed、3 ignored。新增两个原生隐藏窗口测试，模拟完整样式覆盖及 OBS 模式切换，检查穿透可切换且不激活/分层/任务栏样式得到保留。
* `BANDORI_PET_TEST_ROOT=run` 下运行 `cargo test -p pet-app --test window_input -- --ignored --nocapture`：真实 kasumi（moc）和 anon（moc3）窗口验证 `WindowFromPoint` 在透明点返回下层窗口、在模型点返回桌宠窗口；核对显示/隐藏、层级变化、缩放、OBS 切换后样式及 `GetForegroundWindow`，并验证启动隐藏与拖拽保持。

### 2026-10-26 · moc3 模型被 Screen 混合洗白（表现为"渲染错位"）

现象：所有 `.moc3` 服装渲染成半透明的白化幽灵——脸是一块没有眉眼嘴的白色椭圆，肩、胯、膝盖上各有一块白斑，设置窗的文字能透过模型看到；`.moc`（香澄）不受影响。

根因在 `vendor/neocari` 的 moc3 常量标志位解码：内部 flag 字节把 `1 << 2` 定义为 Cubism 2 的 `Screen` 混合，但 **moc3 art mesh 的 constant flag bit 2 不是混合模式**——Cubism 3/4/5 导出器几乎给每个 art mesh 都置了这一位（非合成常量位）。于是 365 个 drawable 全部走 `One/OneMinusSrc` 的 Screen 混合，在透明目标上逐层叠加，颜色饱和到白、alpha 被摊薄：

| 模型 | drawable | flag 直方图（含 `0x04` 的比例） |
|---|---|---|
| anon / school_winter_hs_1st | 365 | `0x04` ×333、`0x06` ×23、`0x0c` ×8、`0x0e` ×1（100%） |
| anon / casual_spring_01 | 352 | `0x04` ×317、`0x06` ×26、`0x0c` ×7、`0x0e` ×2（100%） |

修复（`vendor/neocari/PATCHES.md` 有完整清单）：
* 新增 `Moc3DrawableBlendMode::from_moc3_flags()`：只认 bit 0（additive）、bit 1（multiplicative），bit 2 不参与混合；`Moc3DrawableMesh` 显式保存解析出的 `blend_mode`；
* `build_moc3_drawable_mesh` / `build_moc3_drawable_mesh_for_pose` 都写入 moc3 解码结果，逐帧重建沿用基础网格的混合模式；
* moc2 合成出来的 `Screen` 仍走 `from_flags` 的 `0x04`，`.moc` 行为零改动；
* 单测 `moc3::drawable::tests::{moc3_constant_flag_is_not_decoded_as_screen_blend, cubism2_screen_flag_still_maps_to_screen_blend}` 锁定两套解码；
* `pet-ui` 的 `renders_local_costume_offscreen` 增加回归断言：头部（上 1/3）暗色线稿像素 > 200（修复前只有 40）。

验证（同一套离屏预览，384×768，RTX 5080 / Vulkan）：

| 指标 | 修复前 | 修复后 |
|---|---|---|
| 近白像素（RGB ≥252）占比 | 19 076 / 112 331（17.0%） | 28 / 112 331（0.02%） |
| 头部暗色线稿像素（luma<90） | 40 | 1 679 |
| kasumi（moc2）预览 | — | 与修复前逐像素一致（384×701，max diff=0） |

预览取 `model.update(1/60)` 的单帧，自动眨眼/呼吸带随机种子，相邻两次渲染的模型像素总数会有 ±0.2% 波动；上表的前后对比取自同一模型、同一尺寸的两次测量。

宠物窗实机确认（`bandori-pet.exe`，264×750 物理像素 @dpr 1.5，Windows 桌面截图）：脸、领带、格子裙、袜鞋全部正常，背景仍逐像素透明，设置窗文字不再透过模型。

### 2026-10-26 · moc 角色上下颠倒

`.moc`（Cubism 2）的模型坐标是**画布像素、原点左上、y 向下**（`vendor/neocari/src/moc2/layout.rs` 的模块注释、上游 `examples/render_moc2.rs` 的 `matrix.scale(scale_x, -scale_y)` 都写明了）；Cubism 3+ 是 y 向上。此前两种格式统一用正向 y，导致 moc 角色整体上下颠倒，7 区点击区域也一起错。

修复：
* `pet_window::sizing::fit_matrix(bounds, view_fill, flip_y)`，`ModelHandle::flips_y()` 对 `Moc2` 返回 `true`；
* 区域比例改为「模型点 → 渲染矩阵 → clip → 比例」，两种坐标系都不再需要分支；
* `Moc2Adapter::hit_test_area`：Neocari 映射的是 SDK `hit_areas` 数组，而 Bandori 用的是 `hit_areas_custom`，返回的是绘制件 id（如 `D_PSD.124`）。现在只透传 `head`/`face`/`body`/`hit`，其余按规格 §2.4 的坐标兜底处理。

验证：
* `cargo test -p pet-app --test orientation`（需 `BANDORI_PET_TEST_MODELS`）：moc 包围盒 `(299.8,366.2)..(1703.6,2934.9)`、画布 2000×2500 → 画布像素空间；翻转后 y=366.2 映射到 clip +1.000（顶部），y=2934.9 映射到 −1.000（底部）；moc3 包围盒 `(-0.22,-0.73)..(0.22,0.55)` 居中、不翻转。
* 同一窗口区域像素 A/B：发色簇 y 中心 **0.758 → 0.241**（从底部两成移到顶部两成），剪影覆盖率由上到下 `0.64→0.23`（腿在最下方变窄）。

### 2026-10-26 · config.json 与运行中多角色放置

修复了新增 `models[]` 项却没有创建窗口、保存运行状态时用成功加载的窗口覆盖整个模型列表、保存时把当前选择强制切回首个角色的问题。模型列表更新现在在 winit 活动事件循环中创建或移除窗口，共享 GPU；同角色换装保留窗口，新增和重置位置优先避开已有角色。角色列表支持点选当前编辑角色，切换和移除均作用于对应条目。

设置及每个角色的独立位置写入运行目录的 `config.json`。读取兼容 UTF-8 BOM 和旧版坐标字段，保留顶层及角色条目的未知设置；模型缺失或加载失败的条目不会被后续保存删除。设置窗口会保留保存失败或模型加载失败的反馈。

验证：

* `cargo test --workspace` 覆盖配置/IPC、旧配置读取、错开放置和角色列表选择；真实模型/GPU 回归默认忽略。
* `BANDORI_PET_TEST_ROOT=run` 下执行 `cargo test -p pet-app --test multi_role_runtime -- --ignored --nocapture`，Windows / RTX 5080 / Vulkan 实测通过：kasumi `live_default`（moc）与 anon `casual_spring_01`（moc3）由 1 个窗口增加至 2 个，移除后为 1 个；anon 换装 `school_winter_hs_1st` 复用窗口；组合模型/缩放补丁保持窗口列表；模拟缺失模型后保存仍保留条目；重新创建应用后按 `Absolute` 恢复独立位置和不透明度。
* 本地 anon `birthday_2024_ssr` 的旧 `.moc` 在现有解析器中会报 `expected an id string`，属于已有模型兼容性限制；此次确认加载失败条目仍能保留在配置中。

### 2026-10-26 · 设置窗口的显示与退出

设置进程改用 `run_event_loop_until_quit` 隐藏常驻，只在收到 `OPEN_SETTINGS` 时显示；关闭设置窗口仅隐藏并发送 `SETTINGS_CLOSED`，后续可从托盘或径向菜单重新打开。连接建立前的打开请求暂存，连接后发送，保留首次启动及缺少模型时的角色选择引导。

收到 `QUIT` 后隐藏设置和径向菜单，退出 Slint 事件循环，IPC 客户端停止重连。主进程在退出回调中保存配置、发送 `QUIT` 并回收设置子进程；超过 1 秒仍未退出则终止，避免未连接或失去响应的设置进程残留。

验证：

* `cargo test --workspace`：131 passed、0 failed、4 ignored。
* `cargo test -p pet-app --test ui_lifecycle -- --ignored --nocapture`：真实 Windows 设置进程验证启动及配置快照均不弹窗，关闭后继续运行并可重开，设置和径向菜单同时显示时收到 `QUIT` 后正常退出。
* `settings_open_request_survives_a_delayed_ui_connection`：打开请求早于 IPC 连接时，连接后收到最新请求；连接后的新请求也正常送达。
* release 双进程联调：已有 anon 模型配置时设置保持隐藏；无配置且无模型时自动打开设置；两种场景主进程正常退出后均无设置进程残留。启动后立即退出、UI 尚未连接时，1 秒兜底终止也通过验证。

### 2026-10-26 · 灰黑色透明径向菜单与即时锁图标

径向菜单保留 LITE 的灰黑色圆形按钮，移除标题栏及矩形底板。右键模型后按角色中心定位，180ms OutBack 动画向外展开，选择动作或点击外部后以 120ms 动画收回。空白区域通过随动画更新的 Win32 圆形联合区域排除，可直接点击其他进程的下层窗口；原生窗口不激活、不进任务栏。显示位置按显示器 DPI 换算并限制在工作区，支持负坐标显示器。

锁按钮先在当前事件中更新 🔒 / 🔓，再发送 `MENU_LOCKED`，无需等待 IPC 返回或重新打开菜单。主进程回传配置快照同步设置页；等待确认时忽略旧锁状态，发送失败则回退图标。

验证：

* Slint 图像回归覆盖 100% / 150% / 200% DPI，逐点检查角落、按钮间隙、圆角外区域的 alpha=0，按钮保持灰黑色；连续点击锁按钮验证状态与图标像素立即变化。
* 动画回归检查起点、途中和终点；放置回归覆盖负坐标屏幕、工作区边缘及小于菜单的工作区。
* `cargo test -p pet-app --test ui_lifecycle -- --ignored --nocapture --test-threads=1`：两个真实 Windows 窗口回归通过。菜单测试在 150% DPI 下验证客户区覆盖整个窗口、没有标题栏和调整大小边框、不夺焦、圆形按钮接收点击、透明点的 `WindowFromPoint` 命中测试进程的下层窗口，连续切锁、外部点击收起、动作派发及多次重新打开均通过。
* 原生菜单测试分别以 `winit-software` 和默认 `winit-femtovg` 运行通过；记录到展开区域宽度从 0 经 263 / 355 / 421 到 463 物理像素的中间帧。
* `cargo build --release --bin bandori-pet --bin bandori-ui` 通过，两个程序同步到 `run/`。通过 `BANDORI_UI_TEST_EXE=run/bandori-ui.exe` 对实际 release 程序重跑两个原生窗口回归，均通过。

### 2026-10-27 · 右键菜单不可见但仍能点击

菜单错误复用了模型窗口的 `WS_EX_LAYERED` 样式。Slint 的 WGL/GDI 呈现与模型的透明 wgpu surface 不同；在真实模型进程与 UI 进程同时运行时，菜单的原生命中区域有效，屏幕上却没有按钮。原来的 `IsWindowVisible`、窗口区域和点击回调检查不能发现这一问题。

新增 `apply_popup_style`，保留不激活、工具窗口和去边框处理，移除菜单的 `WS_EX_LAYERED` / `WS_EX_TRANSPARENT`。按钮间穿透仍由 `SetWindowRgn` 提供，模型窗口继续使用原来的分层样式。菜单在动画改变窗口区域后显式请求重绘。

回归增加实际桌面像素检查，覆盖四个按钮及中心锁；另用独立临时目录启动真实的 `bandori-pet` 与 `bandori-ui`，加载本地 hotaru / casual_spring_01，通过模型右键事件连续呼出三次。旧程序在这一场景取到下层模型/桌面像素，修复后灰黑按钮实际呈现。测试使用临时配置和归档硬链接，不修改原安装目录的设置。

验证：`cargo test --workspace` 为 145 passed、0 failed、7 ignored；软件后端的三个原生 UI 回归全部通过，默认 GPU 后端的菜单像素检查和真实双进程右键回归通过。新增窗口样式回归验证 winit 覆盖扩展样式后，菜单仍不携带分层标志，模型窗口仍保留自己的分层和 OBS 行为。

重新构建 release 后，以 `BANDORI_PET_TEST_ROOT=target/release`、`BANDORI_UI_TEST_EXE=target/release/bandori-ui.exe`、`BANDORI_UI_TEST_BACKEND=winit-femtovg` 重跑全部三个原生 UI 回归，3 passed、0 failed；实际 release 程序也同步到 `run/`。

### 2026-10-27 · 扩大动作空间并统一 moc / moc3 观感

原先窗口和投影紧贴模型加载时的内容包围盒，动作伸出该范围就会被裁切；两种格式都按总高度 500 逻辑像素拟合，又让全身 moc3 的脸部和上半身显得更小。

新增固定的 `ModelLayout`，将角色比例和窗口范围分别计算。moc 从 FACE 部件测量中立脸部高度，moc3 从 `.cdi3.json` 定位脸部轮廓后测量对应网格，两者在 100% 缩放、100% DPI 下统一为约 120 像素。缺少脸部元数据时采用格式比例回退；moc3 保留全身。窗口左右各增加半身参考高度 35%、上下各增加 15% 的透明留白，动作不改变缩放。创建、缩放、换装共用同一套计算，纹理尺寸上限也保持横纵比例一致。

渲染与命中继续共享矩阵；七区点击改为相对模型内容包围盒计算，避免新增留白改变区域。新增原生回归在 moc / moc3 的启动和缩放后检查全部七区。

验证：

* `cargo test --workspace`：148 passed、0 failed、7 ignored，其中真实模型回归设置了 `BANDORI_PET_TEST_MODELS=run/models`。
* `model_framing`：kasumi / live_default（moc）窗口 627×657，脸部 120.07 像素；anon / casual_spring_01（moc3）窗口 619×932，脸部 120.01 像素。94 组动作各采样 6 秒、20 FPS，共 11,280 帧，同时切换极限视线方向，可见网格均位于窗口内。离屏图像检查确认两者上半身比例一致，moc3 全身完整显示，四周留白充足。
* `BANDORI_PET_TEST_ROOT=run` 下执行 `cargo test -p pet-app --test window_input --test multi_role_runtime -- --ignored --nocapture --test-threads=1`：2 passed。真实 Windows / Vulkan 窗口验证启动、动态缩放、换装、添加/移除、位置恢复、透明区穿透和不夺焦；全部七区在增加留白和缩放后仍匹配原来的内容比例。
* `cargo build --release --bin bandori-pet --bin bandori-ui` 通过，两个程序同步到 `run/`。
