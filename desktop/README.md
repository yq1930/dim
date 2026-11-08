# Lacus for macOS

Apple Silicon 自用桌面入口。Tauri 窗口复用现有 Web 工作区，通过随应用携带的 `dimcode web --no-open` 启动本地服务；不要求用户先启动终端或安装全局 npm 包。

## 构建

在 macOS 14 或更新版本的 Apple Silicon Mac 上安装 Xcode Command Line Tools、仓库指定的 Rust 工具链和 Node.js，然后在仓库根目录执行：

```sh
git fetch origin master
git switch master
git merge --ff-only origin/master
node scripts/build-macos-app.mjs
```

安装包始终从远程 `origin/master` 的最新提交构建。构建入口会重新获取远程主线，并要求本地位于 `master`、工作区干净且提交完全一致；功能分支、未提交改动，以及未推送或落后的提交都会被拒绝。构建结束时再次核对本地代码和远程主线，主线在构建期间变化时须重新构建。`--debug` 同样遵守这些来源约束。

产物是 `dist/macos/Lacus.app`，可以拖入用户的 `Applications` 目录后双击打开。`--debug` 可生成同样结构的开发包。构建会安装锁定的前端与桌面依赖，编译 Rust 后端，打包 Web 资源、默认插件和应用图标，并验证两个可执行文件的 ARM64 架构及应用签名。

这是使用 ad-hoc 签名的本机构建，不执行公证，也不发布 npm 包。构建本身不上传安装包；需要时用下文的发布脚本把它发布为 GitHub Release，供已安装的 Lacus 在应用内更新。正式对外分发仍沿用仓库的发布准备流程，并补齐桌面依赖声明、Developer ID 签名和公证。

桌面壳使用独立 Cargo workspace，锁文件位于 `desktop/src-tauri/Cargo.lock`。它通过现有 CLI 管理本地服务，不改变 Agent 的组装或协议。桌面应用版本与 `npm/package.json` 保持一致。

`Contents/Resources/build-info.json` 记录 `sourceRemote`、`sourceRef`、`sourceBranch`、`sourceCommit` 和 `sourceDirty`。
正常安装包必须标记为 `origin`、`refs/heads/master`、`master`，且 `sourceDirty=false`。
这些字段描述实际构建时的代码来源；之后合并分支不会改变已安装应用的构建信息。

## 使用

- 首次打开后，在设置中配置模型服务，再新建会话。左侧“打开工作区”和工作区菜单中的“选择项目文件夹…”打开 macOS 原生选择窗口，也可手输绝对路径。取消选择保留原工作区。最近打开的目录保留在侧栏，没有会话也可以切换进入。
- 输入框右下角的模型菜单列出当前服务商的模型，打开后按数字键 1–9 可直接切换；“思考强度”子菜单选择该模型支持的思考档位，模型名旁同时显示当前档位；“更多模型”子菜单可搜索其他服务商的模型。新会话可预先选择；已有会话显示当前配置，并在重新打开后恢复。
- 左侧会话行的“…”菜单支持重命名、归档、复制工作区路径和永久删除。归档保留完整历史，从左侧底部或设置页可恢复；永久删除需要再次确认。标题和归档状态在重启后保留，正在运行的会话完成后可归档或删除。
- 输入框可添加图片、PDF 和 UTF-8 文本/代码文件，首条消息也支持附件。支持 PNG、JPEG、WebP、GIF，单文件不超过 10 MiB；PDF 只提取文字，扫描件没有文字层时会明确提示；暂不支持 Office 等其他二进制文档。已有会话可通过“引用项目文件”搜索文件并插入引用。
- 输入框右下角显示当前上下文用量，接近自动压缩阈值时变红。输入 `/` 可选择技能，选中后插入 `$skill:` 引用；内置的 `/init` 技能根据仓库内容创建或更新 `AGENTS.md`。
- 输入框左下角的模式菜单（或 `/permissions`）可选：手动（改文件、运行命令前都先询问）、接受编辑（工作区内直接修改，运行命令等先询问）、自动（全部直接执行，切换时需确认）、规划和只读。新会话页选过的模式会记住，下次新建会话沿用，默认是接受编辑。在输入框中按 Shift+Tab 可在手动、接受编辑和规划之间切换。
- 运行中按 Esc 中断；输入框为空时按 ↑/↓ 可调出本会话之前发过的消息再编辑发送。审批时可选“拒绝并说明…”，Agent 会在收到拒绝的同时读到说明。
- 会话改过文件后，顶部出现“变更”按钮（或输入 `/diff`），在右侧面板查看本会话改过的每个文件和差异，覆盖写入的文件同样显示被替换的行。点任意一行可以写评论，写完点“放入输入框”，所有评论会带着文件、行号和原文整理成一条消息，确认后再发送。
- 回复里指向文件或文件夹的链接，点击后用默认应用打开该文件（相对路径按会话目录解析，末尾的 `:行号` 会去掉）；按住 Cmd 点击则在 Finder 中定位。文件夹、应用包，以及可执行文件、`.command` 等打开时会运行或安装的类型，一律只在 Finder 中定位，不会直接打开。默认应用打不开的文件也改为在 Finder 中定位。
- 输入框里的 `/review` 让 Agent 评审当前未提交的改动（可在后面写关注点），`/status` 查看模型、模式、上下文用量和工作区，`/copy` 复制最后一条回复。
- 左侧会话行的圆点：琥珀色表示在等你审批、回答或确认计划，蓝色表示在别的页面时已跑完、尚未查看，绿色闪动表示正在运行。
- 选择规划（或 `/plan`）开启规划模式：会话临时变为只读，Agent 只调查并用 `exit_plan_mode` 提交计划；批准后恢复原模式并开始执行，“继续规划”可附修改意见退回。新会话可先选规划再发送首条消息。Agent 用 `todo_write` 维护的任务清单显示在输入框上方。
- 窗口不在前台时，任务结束、等待审批或等待回答或等待确认计划会发送系统通知并让 Dock 图标跳动一次，Dock 图标上的数字是尚未查看的提醒数，回到窗口后清零。首次启用时如未显示，请在“系统设置 > 通知”中允许 Lacus。
- 设置中的“网络搜索”用于配置 `web_search`：可选 Dim 账号、DeepSeek、Tavily 或 Brave Search，后三者需要各自的 API Key，密钥只保存在本地应用数据目录。
- 本地服务启动前，先按账户的登录 shell 读取一次环境变量（与打开终端时读取同样的 `.zprofile`、`.zshrc`），因此从 Finder 打开时，Agent 运行的命令也能找到 nvm、pyenv、Homebrew 等安装的工具。读取超过 10 秒或失败时依次改用 `/bin/zsh`、`/bin/bash`、`/bin/sh`，都不成功则沿用继承的环境；无论哪种情况，`PATH` 末尾都会补上 `~/.local/bin`、`~/.cargo/bin`、`/opt/homebrew/bin` 等常用目录。
- 环境变量中没有 `HTTPS_PROXY` 等代理变量时，读取 macOS 网络设置中的 HTTP/HTTPS 代理并传给本地服务，使模型请求能走系统代理（如 Clash 的“系统代理”）。切换系统代理或修改 shell 配置后，用“帮助 > 重启本地服务”即可生效，不必退出应用。
- 切换工作区、会话或进入设置时，未发送的文字和附件分别保留在原草稿中；这些草稿只保留到本次应用退出或页面重新加载。
- 模型菜单可直达模型服务设置，也可将当前模型设为全局默认；工作区已有的专属默认值仍优先使用。
- 会话和模型配置保存在 `~/Library/Application Support/com.yetang.dim`，与命令行版本的 home 分开，不自动迁移或修改已有会话。应用由 Dim 改名为 Lacus 后，内部标识仍是 `com.yetang.dim`，原有会话和设置照常可用。`webview-id` 保存一个 UUID，对应 macOS 管理的独立 WebKit 存储空间，隔离不同 home 的 Cookie、界面偏好和最近工作区。
- 关闭窗口（含 `Cmd+W`）会隐藏应用；再次点击 Dock 图标或启动应用会显示原窗口。`Cmd+Q` 退出应用并停止这个独立 home 下的本地服务。
- 窗口大小和是否最大化在关闭窗口或退出时保存，下次打开时恢复；最大化时保留的是还原后的大小。
- 菜单栏为中文。“显示”菜单可重新加载页面（`Cmd+R`）、缩放（`Cmd+=`、`Cmd+-`、`Cmd+0`，共 9 档，跨启动保留）、进入全屏和打开开发者工具（`Option+Cmd+I`，打包版同样可用）；“帮助”菜单可打开数据目录、在 Finder 中显示本地服务日志、重启本地服务；“关于 Lacus”显示版本号以及构建所用的提交和日期。
- 本地服务意外停止或无法启动时，窗口上弹出提示，可选择“重启本地服务”或“退出 Lacus”，应用不会直接退出。
- 外部 HTTP/HTTPS 链接由默认浏览器打开，桌面窗口保留在本地工作区。
- 默认构建支持 API-key 模型服务。Dim 账号登录沿用现有构建时的 OAuth 配置；Node/Python 插件沿用 RuntimeManager 的显式安装机制，首次启动不下载运行时。

需要使用另一个本地数据目录时，可以通过 `DIM_DESKTOP_HOME` 指定绝对路径。`DIMCODE_MODEL_CONFIG` 可指定现有 CLI 格式的模型配置文件；这些环境变量适合从终端启动应用可执行文件。开发和验收必须使用独立的临时 home 和本地模型夹具，不连接实际模型服务。

文件配置模式中的模型由配置文件维护；应用内的模型服务管理使用默认模式。

诊断时可查看应用数据目录下 `rust/daemon.log`（“帮助 > 显示本地服务日志”直接在 Finder 中定位）；应用不会记录一次性 Web 登录链接。窗口大小和缩放档位保存在同一目录的 `desktop-state.json`。

```text
打开工作区 → macOS 选择窗口 → 最近工作区 → 新会话及附件
侧栏会话 … → 重命名 / 归档 / 复制路径 / 永久删除（确认）
已归档会话 → 恢复到侧栏
模型菜单   → 模型服务设置 / 设为全局默认模型
```

原生命令（选择文件夹、提醒、打开文件）仅授权给当前主窗口的本地服务地址，不向其他网页开放文件或 Shell 权限。
会话归档使用原生存储 v34；旧格式在启动事务内升级，测试和开发始终使用独立 home。

## 应用内更新

- 启动约 10 秒后和之后每 6 小时，Lacus 读取 `https://github.com/yq1930/dim/releases/latest/download/latest.json`。发现更新的构建（版本号更高，或版本号相同但构建时间更晚）时，在窗口上提示但不把窗口提到前台，可选“稍后”“更新并重启”“跳过此版本”；默认按钮是“稍后”，按回车不会开始更新。窗口隐藏或最小化时不提示，等下一次检查。
- “Lacus > 检查更新…”随时手动检查，已跳过的版本也会再次提示；没有更新时显示当前构建。
- 选择“更新并重启”后在后台下载，Dock 图标显示进度；连接中断或 60 秒收不到数据时，从已下载的位置续传，连续 4 次没有进展才放弃并提示。下载完成后先用内置公钥校验签名，通过后才停止本地服务、替换应用并重启，正在运行的任务会中断。下载或校验失败时保持当前版本；替换失败时恢复原应用后重启。
- 检查和下载使用 macOS 系统代理。跳过的版本记录在数据目录的 `desktop-update.json`。
- `LACUS_UPDATE_CHECK=0` 关闭定时检查；`LACUS_UPDATE_ENDPOINT` 改用其他清单地址，发布构建只接受 https。开发包（`--debug`）只有设置了 `LACUS_UPDATE_ENDPOINT` 时才定时检查。

## 发布更新

更新包用本机私钥签名，对应的公钥写在 `src-tauri/tauri.conf.json` 的 `plugins.updater.pubkey`。私钥默认位于 `~/.tauri/lacus-updater.key`（可用 `TAURI_SIGNING_PRIVATE_KEY_PATH` 指定，有口令时用 `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`），不得提交到仓库。请另行备份私钥：丢失后，已安装的应用无法校验新版本，只能手动重装更换了公钥的版本。

在 master 构建完成后发布：

```sh
node scripts/build-macos-app.mjs
node scripts/publish-macos-app.mjs --dry-run
node scripts/publish-macos-app.mjs
```

发布脚本先确认 `dist/macos/Lacus.app` 是当前 `origin/master` 的发布构建、应用签名有效，且其中的公钥与私钥匹配；然后打包 `Lacus.app.tar.gz` 并签名（签名绑定版本号），生成 `latest.json`，在 `yq1930/dim` 创建标记为 latest 的 Release `lacus-<构建日期>-<提交>`，并附带首次安装用的 `Lacus-macos-arm64.zip`。版本号是 `npm/package.json` 的版本加构建时间和提交，例如 `0.1.0-alpha.18+20261107071400.42138b5`；同一提交只发布一次。`--dry-run` 只在 `staging/macos-release` 生成文件，不上传。

从 Release 首次安装时，压缩包经浏览器下载，ad-hoc 签名的应用会被 Gatekeeper 拦下，第一次需在 Finder 中右键选择“打开”；之后的应用内更新不经过浏览器下载，不会再被拦。

## 代码结构

桌面壳的 Rust 代码在 `src-tauri/src`，按职责分为几个模块：

| 模块 | 职责 |
| --- | --- |
| `main.rs` | 组装插件、菜单、窗口和本地服务，分发窗口与退出事件 |
| `service.rs` | 本地服务的生命周期：启动 `dimcode web --no-open`、识别工作区地址、授予原生命令、意外停止时询问重启或退出、重启与停止 |
| `environment.rs` | 本地服务的环境变量：登录 shell 环境、`PATH` 补充目录、系统代理 |
| `window.rs` | 主窗口：导航规则、窗口大小与缩放的保存和恢复、关闭即隐藏 |
| `menu.rs` | 中文应用菜单及其命令 |
| `commands.rs` | 工作区页面可调用的原生命令：选择文件夹、提醒与 Dock 计数、打开文件或在 Finder 中定位 |
| `update.rs` | 应用内更新：定时与菜单检查、提示、下载与签名校验、停止本地服务后替换应用并重启 |

`src-tauri/Info.plist` 声明应用的本地化为简体中文，macOS 自动加入的菜单项（如“表情与符号”）和系统对话框按钮也显示中文。

在已有 Lacus 运行时验证桌面改动，可构建标识不同的开发包，避免单实例插件把新进程转交给正在运行的应用：

```sh
cd desktop
npm exec -- tauri build --debug --bundles app --config '{"identifier":"com.yetang.dim.dev","productName":"LacusDev"}'
```

运行时用 `DIM_DESKTOP_HOME` 指向临时目录。
