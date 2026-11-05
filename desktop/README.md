# Dim for macOS

Apple Silicon 自用桌面入口。Tauri 窗口复用现有 Web 工作区，通过随应用携带的 `dimcode web --no-open` 启动本地服务；不要求用户先启动终端或安装全局 npm 包。

## 构建

在 macOS 14 或更新版本的 Apple Silicon Mac 上安装 Xcode Command Line Tools、仓库指定的 Rust 工具链和 Node.js，然后在仓库根目录执行：

```sh
node scripts/build-macos-app.mjs
```

产物是 `dist/macos/Dim.app`，可以拖入用户的 `Applications` 目录后双击打开。`--debug` 可生成同样结构的开发包。构建会安装锁定的前端与桌面依赖，编译 Rust 后端，打包 Web 资源、默认插件和应用图标，并验证两个可执行文件的 ARM64 架构及应用签名。

这是使用 ad-hoc 签名的本机构建，不执行公证、不上传安装包，也不发布 npm 包或 GitHub Release。对外分发沿用仓库的正式发布准备流程，并补齐桌面依赖声明、Developer ID 签名和公证。

桌面壳使用独立 Cargo workspace，锁文件位于 `desktop/src-tauri/Cargo.lock`。它通过现有 CLI 管理本地服务，不改变 Agent 的组装或协议。桌面应用版本与 `npm/package.json` 保持一致。

## 使用

- 首次打开后，在设置中配置模型服务，再新建会话并选择工作目录。
- 会话和模型配置保存在 `~/Library/Application Support/com.yetang.dim`，与命令行版本的 home 分开，不自动迁移或修改已有会话。`webview-id` 保存一个 UUID，对应 macOS 管理的独立 WebKit 存储空间，隔离不同 home 的 Cookie、界面偏好和最近工作区。
- 关闭窗口会隐藏应用；再次点击 Dock 图标或启动应用会显示原窗口。`Cmd+Q` 退出应用并停止这个独立 home 下的本地服务。
- 外部 HTTP/HTTPS 链接由默认浏览器打开，桌面窗口保留在本地工作区。
- 默认构建支持 API-key 模型服务。Dim 账号登录沿用现有构建时的 OAuth 配置；Node/Python 插件沿用 RuntimeManager 的显式安装机制，首次启动不下载运行时。

需要使用另一个本地数据目录时，可以通过 `DIM_DESKTOP_HOME` 指定绝对路径。`DIMCODE_MODEL_CONFIG` 可指定现有 CLI 格式的模型配置文件；这些环境变量适合从终端启动应用可执行文件。开发和验收必须使用独立的临时 home 和本地模型夹具，不连接实际模型服务。

文件配置模式中的模型由配置文件维护；应用内的模型服务管理使用默认模式。

诊断时可查看应用数据目录下 `rust/daemon.log`；应用不会记录一次性 Web 登录链接。
