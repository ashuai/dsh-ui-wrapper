# DSH — 极简 dsh Web 界面壳(macOS)

[![Rust CI](https://github.com/ashuai/dsh-ui-wrapper/actions/workflows/rust.yml/badge.svg)](https://github.com/ashuai/dsh-ui-wrapper/actions/workflows/rust.yml)

> [English](README.md) | 中文版

一个超简单的 macOS 原生窗口:用**系统浏览器核心(WebKit / WKWebView)直接打开
`http://127.0.0.1:3080`** 的 dsh web 界面。不画界面、不套浏览器外壳、没有标签页,
窗口里就是 dsh 自己的网页——并带一个后端自动拉起的小助手,打开就有页面。

## 功能

- 原生窗口内嵌系统 WebKit(Safari 同款引擎,不自造前端,零前端资源)
- **后端自动拉起**:启动即探测 `127.0.0.1:3080`;不通则自动启动 dsh(`bunx` → `pnpm` → `npm` → `dsh`),
  toast 实时提示(`打开中…` / `启动中… 已等待 Ns` / `已就绪`),端口就绪后自动载入页面
- **快速失败**:拉起进程提前死掉(端口被占/缺依赖)时,~0.4s 内弹出错误面板并附后端日志尾部,不用傻等超时
- 启动页**自适应深浅色**(跟随系统外观)
- `Cmd+R` 刷新(后端重启后很好用);关窗即退出
- DeepSeek 大胖鲸应用图标(官方 1024px 素材;图标版权归 DeepSeek,仅作应用图标使用)

## 环境要求

- macOS 12+
- dsh 后端:可选。`127.0.0.1:3080` 已在跑就直接加载;否则自动启动 dsh,需要本机有
  `bunx` / `pnpm` / `npm` 之一(或全局 `dsh`),在 PATH 或常见安装目录中即可

## 快速开始

```bash
cd DSH
./make_app.sh                   # 编译 + 生成图标 + 组装 target/DSH.app
open target/DSH.app
```

或直接跑裸二进制:`./target/release/DSH`。

## 工作原理

```
启动
 ├─ 显示自举页(大胖鲸 + 自适应主题 + toast 区)
 ├─ 后台线程:
 │    ① 探测 127.0.0.1:3080(TCP,约 300ms)
 │       ├─ 通   → toast「已就绪」→ 载入 http://127.0.0.1:3080
 │       └─ 不通 → 找启动器(bunx/pnpm/npm/dsh,按系统分支)→ spawn「… dsh web」
 │                  (detach 常驻,日志 → ~/Library/Logs/DSH-backend.log)
 │                  每 400ms 轮询,toast 每秒报「启动中… 已等待 Ns」
 │                  ├─ 进程提前退出 → 错误面板(快速失败)
 │                  └─ 超时(默认 30s)→ 错误面板 + 重试
 └─ 就绪 → 载入真实页面
```

被拉起的 dsh 在 App 退出后**继续常驻运行**,重开秒连。

## 环境变量

| 变量 | 默认值 | 说明 |
| --- | --- | --- |
| `DSH_URL` | `http://127.0.0.1:3080` | 后端地址 |
| `DSH_NO_AUTOSTART` | 关 | 只探测,不通直接报错(不拉起) |
| `DSH_BACKEND_TIMEOUT` | `30` | 拉起后等待就绪的秒数 |
| `DSH_BACKEND_LOG` | `~/Library/Logs/DSH-backend.log` | 被拉起后端的日志文件 |
| `DSH_DEBUG` | 关 | 开启壳日志 |
| `DSH_LOG` | `~/Library/Logs/DSH.log` | 壳日志文件(需 `DSH_DEBUG=1`) |
| `DSH_DEVTOOLS` | 关 | 打开 WebKit 开发者工具(页面侧诊断) |

## Debug 模式

```bash
DSH_DEBUG=1 ./target/DSH.app/Contents/MacOS/DSH
# 可选:DSH_LOG=/tmp/dsh.log DSH_BACKEND_LOG=/tmp/dsh-backend.log
```

`DSH.log` 记录自举状态机(探测/启动器/拉起/就绪/超时);`DSH-backend.log` 是被拉起
dsh 自己的输出——自动拉起失败时先看它。panic 也会写进日志。

## 跨平台

macOS 是主要目标。同一份代码在 Windows(WebView2)和 Linux(WebKitGTK)也能编译——
启动器发现按系统分支(PATH 分隔符、Windows 的 `.exe/.cmd/.bat`、各平台兜底目录)。
CI 三平台构建;`make_app.sh` 是 macOS 专属的打包步骤。

## CI 与发版

CI 以 changelog 为门:**只有当 `changelog/` 出现新版本文件(`vX.Y.Z.md`)时**才三平台构建,
并自动发布 GitHub Release(tag = 版本号,正文 = changelog 内容,附件 = macOS `.app`
压缩包 / Windows exe / Linux 二进制)。手动触发:GitHub → Actions → Run workflow。

流程:改代码 → 新建 `changelog/vX.Y.Z.md` → push → CI 构建 → Release 自动出现。

## 已知坑(已修复)

- **失焦崩溃**(早期版本):wry 默认 `build()` 会替换窗口 contentView,vanilla winit
  在失焦时按错误类型解析 → 段错误。已改用 **tao**(Tauri 维护的 winit 分支)+ wry
  默认 `build()`——与 Tauri 相同组合:不崩,且键盘/输入法正常。
- **打字/输入法卡顿**:子视图绕法引入,同上切换后一并解决。

## 仓库结构

- `src/main.rs` — 入口、自举页、事件循环
- `src/backend.rs` — 自举状态机(探测/启动器/拉起/轮询/快速失败)
- `make_app.sh` — 编译 + 图标 + `.app` 组装
- `assets/` — 大胖鲸图标(源 jpg、`DSH.icns`、自举页 base64 logo)
- `changelog/` — 触发 CI/发版的版本条目
- `DSH-docs/`(本仓库外)— 需求与设计文档

## 许可证

Apache-2.0。大胖鲸图标为 DeepSeek 品牌资产(此处仅作应用图标使用)。
