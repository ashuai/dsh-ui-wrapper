# DSH — 极简 DSH 壳(macOS)

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
- 应用图标:DeepSeek 大胖鲸(`assets/dsh-whale.jpg`,官方 1024px 图标,自动生成 `DSH.icns`)。

## 代码

`src/main.rs` 总共约 80 行,依赖只有两个:

- `wry` — 跨平台 WebView 库(macOS 上用 WKWebView,即系统浏览器核心)
- `winit` — 窗口与事件循环

没有自定义 UI、没有 HTML/JS 前端代码、不打包任何前端资源。

## 目录

- `src/main.rs` — 全部代码
- `make_app.sh` — 打包 `DSH.app`(编译 + 生成图标 + 写 Info.plist)
- `assets/dsh-whale.jpg`、`assets/DSH.icns` — 大胖鲸图标素材
