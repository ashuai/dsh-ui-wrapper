# DSH — 极简 DSH 壳(macOS)

[![Rust CI](https://github.com/ashuai/dsh-ui-wrapper/actions/workflows/rust.yml/badge.svg)](https://github.com/ashuai/dsh-ui-wrapper/actions/workflows/rust.yml)

一个超简单的 macOS 原生窗口:用**系统浏览器核心(WebKit / WKWebView)直接打开
`http://127.0.0.1:3080`** 的 dsh web 界面。不画任何界面、不重新实现聊天 UI,
窗口里就是 dsh 自己的网页,但不再需要打开 Safari/Chrome 这样的独立浏览器。

## 运行

```bash
cd DSH
./make_app.sh                   # 编译 + 打包 target/DSH.app(含大胖鲸图标)
open target/DSH.app
```

也可以直接 `./target/release/DSH` 跑裸二进制(此时无 .app 图标)。

先确保 dsh 后端已启动且监听 3080(通常 `dsh web` 会启动)。

## 功能

- 原生窗口内嵌系统 WebKit,加载 dsh 网页,无浏览器外壳、无地址栏/标签页。
- `Cmd+R` 刷新页面(后端重启后很有用)。
- 关闭窗口即退出。
- 应用图标:DeepSeek 大胖鲸(`assets/dsh-whale.jpg`,官方 1024px 图标,自动生成 `DSH.icns`;图标版权归 DeepSeek,仅作应用图标使用)。

## Debug 模式(输出更多日志)

```bash
DSH_DEBUG=1 ./target/release/DSH     # 开启文件日志(默认 ~/Library/Logs/DSH.log)
DSH_LOG=/tmp/dsh.log DSH_DEBUG=1 ... # 指定日志文件
DSH_DEVTOOLS=1 DSH_DEBUG=1 ...       # 额外打开 WebKit 开发者工具(页面侧诊断)
```

日志内容:启动参数、窗口/WebView 生命周期、页面加载与导航、焦点变化、窗口缩放、
`Cmd+R` 刷新,以及任何 panic 的回溯。

> 仓库地址:https://github.com/ashuai/dsh-ui-wrapper

## 发版流程(CI 触发规则)

**只有 `changelog/` 出现新版本文件时才自动编译**,平时改代码不会触发 CI。
编译通过后**自动创建 GitHub Release**:tag 为版本号、正文为该版本的 changelog、
附件为三平台产物(macOS `DSH.app` 压缩包 / Windows exe / Linux 二进制)。

1. 改完代码,准备发版
2. 新建 `changelog/vX.Y.Z.md`,写这一版改了什么
3. 提交 push 到 main → 自动编译三平台 → 自动发布 `vX.Y.Z` 到 Releases
4. 想随时手动验证:`GitHub → Actions → Run workflow`(已发过的版本会重建)

详见 [`changelog/README.md`](changelog/README.md)。

## 代码

`src/main.rs` 约 150 行,依赖只有两个:

- `wry` — 跨平台 WebView 库(macOS 上用 WKWebView,即系统浏览器核心)
- `winit` — 窗口与事件循环

没有自定义 UI、没有 HTML/JS 前端代码、不打包任何前端资源。

### 已知坑(已修复):窗口失焦崩溃

早期版本用 wry 默认的 `build()` 挂载 WebView,它会用 `setContentView` **替换窗口的
contentView**;而 winit 的窗口 delegate 假定 contentView 永远是它自己的视图,窗口
失焦(`windowDidResignKey`,比如点击别的 App)时按错误类型解析对象 → 野指针
`EXC_BAD_ACCESS` 崩溃。现在改用 **`build_as_child()`**(WebView 作为子视图,不替换
contentView),并在 `Resized` 事件里 `set_bounds` 铺满窗口,失焦不再崩溃
(实测连续失焦/聚焦均稳定)。

## 目录

- `src/main.rs` — 全部代码
- `make_app.sh` — 打包 `DSH.app`(编译 + 生成图标 + 写 Info.plist)
- `assets/dsh-whale.jpg`、`assets/DSH.icns` — 大胖鲸图标素材
