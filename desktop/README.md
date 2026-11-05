# Dim for macOS

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

产物是 `dist/macos/Dim.app`，可以拖入用户的 `Applications` 目录后双击打开。`--debug` 可生成同样结构的开发包。构建会安装锁定的前端与桌面依赖，编译 Rust 后端，打包 Web 资源、默认插件和应用图标，并验证两个可执行文件的 ARM64 架构及应用签名。

这是使用 ad-hoc 签名的本机构建，不执行公证、不上传安装包，也不发布 npm 包或 GitHub Release。对外分发沿用仓库的正式发布准备流程，并补齐桌面依赖声明、Developer ID 签名和公证。

桌面壳使用独立 Cargo workspace，锁文件位于 `desktop/src-tauri/Cargo.lock`。它通过现有 CLI 管理本地服务，不改变 Agent 的组装或协议。桌面应用版本与 `npm/package.json` 保持一致。

`Contents/Resources/build-info.json` 记录 `sourceRemote`、`sourceRef`、`sourceBranch`、`sourceCommit` 和 `sourceDirty`。
正常安装包必须标记为 `origin`、`refs/heads/master`、`master`，且 `sourceDirty=false`。
这些字段描述实际构建时的代码来源；之后合并分支不会改变已安装应用的构建信息。

## 使用

- 首次打开后，在设置中配置模型服务，再新建会话。左侧“打开工作区”和工作区菜单中的“选择项目文件夹…”打开 macOS 原生选择窗口，也可手输绝对路径。取消选择保留原工作区。最近打开的目录保留在侧栏，没有会话也可以切换进入。
- 支持思考程度配置的模型会在输入框底部显示选择器。新会话可预先选择；已有会话显示当前配置，并在重新打开后恢复。
- 左侧会话行的“…”菜单支持重命名、归档、复制工作区路径和永久删除。归档保留完整历史，从左侧底部或设置页可恢复；永久删除需要再次确认。标题和归档状态在重启后保留，正在运行的会话完成后可归档或删除。
- 输入框可添加图片、PDF 和 UTF-8 文本/代码文件，首条消息也支持附件。支持 PNG、JPEG、WebP、GIF，单文件不超过 10 MiB；PDF 只提取文字，扫描件没有文字层时会明确提示；暂不支持 Office 等其他二进制文档。已有会话可通过“引用项目文件”搜索文件并插入引用。
- 输入框右下角显示当前上下文用量，接近自动压缩阈值时变红。输入 `/` 可选择技能，选中后插入 `$skill:` 引用；内置的 `/init` 技能根据仓库内容创建或更新 `AGENTS.md`。
- 窗口不在前台时，任务结束、等待审批或等待回答会发送系统通知并让 Dock 图标跳动一次。首次启用时如未显示，请在“系统设置 > 通知”中允许 Dim。
- 设置中的“网络搜索”用于配置 `web_search`：可选 Dim 账号、DeepSeek、Tavily 或 Brave Search，后三者需要各自的 API Key，密钥只保存在本地应用数据目录。
- 启动时读取 macOS 网络设置中的 HTTP/HTTPS 代理，并传给本地服务，使模型请求能走系统代理（如 Clash 的“系统代理”）。切换系统代理后需重启应用才生效；从终端启动且已设置 `HTTPS_PROXY` 等变量时以环境变量为准。
- 切换工作区、会话或进入设置时，未发送的文字和附件分别保留在原草稿中；这些草稿只保留到本次应用退出或页面重新加载。
- 模型菜单可直达模型服务设置，也可将当前模型设为全局默认；工作区已有的专属默认值仍优先使用。
- 会话和模型配置保存在 `~/Library/Application Support/com.yetang.dim`，与命令行版本的 home 分开，不自动迁移或修改已有会话。`webview-id` 保存一个 UUID，对应 macOS 管理的独立 WebKit 存储空间，隔离不同 home 的 Cookie、界面偏好和最近工作区。
- 关闭窗口会隐藏应用；再次点击 Dock 图标或启动应用会显示原窗口。`Cmd+Q` 退出应用并停止这个独立 home 下的本地服务。
- 外部 HTTP/HTTPS 链接由默认浏览器打开，桌面窗口保留在本地工作区。
- 默认构建支持 API-key 模型服务。Dim 账号登录沿用现有构建时的 OAuth 配置；Node/Python 插件沿用 RuntimeManager 的显式安装机制，首次启动不下载运行时。

需要使用另一个本地数据目录时，可以通过 `DIM_DESKTOP_HOME` 指定绝对路径。`DIMCODE_MODEL_CONFIG` 可指定现有 CLI 格式的模型配置文件；这些环境变量适合从终端启动应用可执行文件。开发和验收必须使用独立的临时 home 和本地模型夹具，不连接实际模型服务。

文件配置模式中的模型由配置文件维护；应用内的模型服务管理使用默认模式。

诊断时可查看应用数据目录下 `rust/daemon.log`；应用不会记录一次性 Web 登录链接。

```text
打开工作区 → macOS 选择窗口 → 最近工作区 → 新会话及附件
侧栏会话 … → 重命名 / 归档 / 复制路径 / 永久删除（确认）
已归档会话 → 恢复到侧栏
模型菜单   → 模型服务设置 / 设为全局默认模型
```

原生选择命令仅授权给当前主窗口的本地服务地址，不向其他网页开放文件或 Shell 权限。
会话归档使用原生存储 v34；旧格式在启动事务内升级，测试和开发始终使用独立 home。
