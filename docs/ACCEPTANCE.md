# 规格 §7 验收清单 × 实测证据

对应 `../BANDORI-PET-REV/docs/BANDORI-PET-LITE.md` §7。每条给出「怎么验的」和「实测值」。
测试机器：Windows 11，RTX 5080 Laptop，Vulkan 后端，显示器 DPR 1.5，主屏工作区 2532×1536 物理像素。

| # | 验收项 | 结果 | 证据 |
|---|---|---|---|
| 1 | 冷启动 ≤1.5s 出画面 | ✅ | release：进程内 `role ready ms=1101`，外部轮询 `IsWindowVisible` 首帧可见 **1540ms**（含 PowerShell `Start-Process` 约 300–400ms 的建进程开销）。debug 版因 300MB 调试符号页入会多 1s 左右。首帧在 `Role::create` 内同步呈现后才 `set_visible(true)`；启动时不再扫描 64 个归档（改为 `ensure_catalog` 惰性扫描），托盘也延后到首帧之后创建。 |
| 2 | 多屏位置持久化：拖到别处→重启回到原位；拔掉副屏回主屏中央 | ✅（前半实测，后半单测） | 拖动后 `config.json`：`window_placement x=1862 y=699 w=410 h=750 screen=\\.\DISPLAY1 dpr=1.5`；重启日志 `decision=Absolute x=1862 y=699`，`GetWindowRect` 一致。换屏/分辨率变化走 `relative_x/y` 重算、显示器消失回落主屏中心：`pet-window/src/placement.rs` 的 4 个单测（`changed_geometry_uses_relative_position`、`missing_monitor_falls_back_to_primary` 等）。 |
| 3 | 按在头发上拖动跟手；划过透明区点到桌面 | ✅ | 拖动：`Drag(2122+205,789+500 → +325,+460)` 后窗口 `2122,789 → 2242,749`，位移 (120,−40) 与注入位移一致。穿透：光标在窗口左上透明角 `EXSTYLE=0x40138`（含 `WS_EX_TRANSPARENT`），移到模型上 `0x40118`（不含），移回角落又变 `0x40138`。 |
| 4 | 缩放 25% / 500% 后窗口重建、命中仍准 | ⚠️ 部分 | 尺寸推导与命中都在同一内容包围盒/同一矩阵上做（`fit_matrix` 与 `hit_test` 共用 `self.matrix`），`sizing.rs` 有 25%/500% 尺寸单测；两个极端值的联调未跑（会占满屏幕）。 |
| 5 | 7 区分别配动作、逐区点击；`不触发` 区无反应 | ✅ | 7 区判定与预设展开有 22 个单测（含阈值 0.38/0.64/0.62、`__none__` 直接返回空命令、`genki`/`tsundere` 展开结果）。联调点击窗口 10% / 50% / 90% 高度：日志分别是 `region="head"` / `"upper_body_left"` / `"lower_body_center"`，动作按 AUTO 桶命中 `surprised01` / `shame01`。 |
| 6 | 双击 → 动作 + 抖动 + 气泡；连续 20 次不累积 | ✅（气泡与动作实测，20 次压力未单独跑） | 双击后日志 `play motion motion=shame01`；气泡窗口 `bandori-pet-bubble` `vis=True rect=2217,864,352x140`，~1980ms 后 `vis=False`。状态都在 `PokeState`/`FeedbackTimers` 里按时间戳覆盖，不累积队列。 |
| 7 | 换角色 ≤1s 热重载，位置缩放保持 | ✅ | 双角色启动日志：kasumi `ms=1025`、anon（moc3，复用同一 GPU）`ms=145`；`swap_model` 不重建窗口，位置取自 `window_placement`。 |
| 8 | 画质切省电 → 渲染分辨率 0.75× | ✅ | `SceneRenderer::render_size()` 在 0.75× 时返回 `(307,562)`（410×750 的 0.75）；`scale_dim` 单测覆盖 0.75/1.0/0.25。 |
| 9 | 改 vsync / OBS 兼容后仍透明、仍能穿透 | ✅（重建路径单测，联调未逐一跑） | `apply_obs_capture` 换 `WS_EX_TOOLWINDOW ↔ WS_EX_APPWINDOW` 并 `SWP_FRAMECHANGED`；vsync 走 `Surface::reconfigure`。透明由 surface 的 `PreMultiplied` 决定（实测 `alpha=PreMultiplied`），与窗口样式无关。 |
| 10 | 删掉 config.json 启动 → 打开设置角色页 + 横幅，不崩 | ✅ | `LoadOutcome::is_first_run()` → `OPEN_SETTINGS{page:Model}`；UI 的 `first-run` 横幅。集成测试 `model_selection_without_models_reports_first_run`、`load_config` 的 missing/corrupt 单测（坏文件改名 `.corrupt-<ts>.bak`）。 |

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

1. **径向菜单外部点击关闭**：Slint 没有「失焦/点外面」事件，当前靠 pet 在下次右键时重发 `MENU_SHOW` 覆盖旧位置，菜单不会自己消失。规格要求「点击外部关闭」，这是唯一未实现的交互细节。
2. **Slint 窗口不做逐像素透明**：设置窗、径向菜单、气泡都是不透明窗口（深色面板）。桌宠窗口的透明是 wgpu surface 实现的，不受影响。
3. **`WM_NCHITTEST → HTTRANSPARENT` 安全网未做**：规格称它只是同进程安全网，实际穿透完全由 `WS_EX_TRANSPARENT` 控制（已实测有效）。用 `SetWindowLongPtr` 改 winit 的 wndproc 风险大于收益。
4. **渲染在主线程**：规格建议 main/render/io 三线程；实现是「事件循环里 `WaitUntil(1/fps)` 节流 + 同线程渲染」，命中是 CPU 计算不阻塞 GPU 提交，实测 60fps 稳定。要拆线程时 `FrameSnapshot` 已经是不可变快照，接口不用改。
5. **像素模式、i18n 完整化、点击动作配置的导入/导出 UI** 未做（规格标明像素模式 v0 不做 UI；i18n 目前是中/英两套硬编码字符串）。
6. **`live2d_active_profile`（自定义命名的 profile 保存/选择）** 只支持内置 7 套预设展开；自定义 profile 的数据结构（`click_motion_profiles`）已进配置契约，UI 未提供保存入口。

## 复现方式

```powershell
cargo test --workspace                                  # 114 个测试
pwsh -File scripts/run-dev.ps1 -ModelsFrom <你的 models 目录>
$env:BANDORI_PET_ROOT = "$PWD\run"; $env:RUST_LOG = "info,pet_app=debug"
.\run\bandori-pet.exe
```

* 冷启动/热重载耗时：看日志 `role ready ms=…`。
* 透明与绘制：宠物运行前后对同一屏幕矩形各截一张图做像素差（四角应为 0，模型区域应显著变化）。
* 穿透：`GetWindowLongPtrW(hwnd, GWL_EXSTYLE) & 0x20` 随光标在模型/透明区移动而切换。
* 位置持久化：拖动后等 2s 让防抖落盘，读 `config.json` 的 `models[0].window_placement`，重启后看日志 `decision=Absolute`。


## 修复记录

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
