// DSH — 极简壳:用系统浏览器核心(WebKit / WKWebView)打开本机 dsh web 界面。
//
// 启动自举:打开即显示带 toast 的自举页 → 后台探测 127.0.0.1:3080 →
// 不通则按 bunx/pnpm/npm/dsh 自动拉起 dsh web → 就绪后自动载入页面;
// 失败给出错误面板(后端日志尾部 + 重试)。
//
// 环境变量:
//   DSH_URL=<url>            后端地址(默认 http://127.0.0.1:3080)
//   DSH_NO_AUTOSTART=1       只探测,不通直接错误(不拉起后端)
//   DSH_BACKEND_TIMEOUT=<秒> 等待就绪超时(默认 90)
//   DSH_BACKEND_LOG=<path>   后端进程日志(默认 ~/Library/Logs/DSH-backend.log)
//   DSH_DEBUG=1              壳日志(默认 ~/Library/Logs/DSH.log)
//   DSH_LOG=<path>           指定壳日志文件
//   DSH_DEVTOOLS=1           打开 WebKit 开发者工具

mod backend;

use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

use tao::{
    dpi::LogicalSize,
    event::{ElementState, Event, KeyEvent, WindowEvent},
    event_loop::{ControlFlow, EventLoop},
    keyboard::{Key, ModifiersState},
    window::WindowBuilder,
};
use wry::{PageLoadEvent, WebView, WebViewBuilder};

use backend::{BootCmd, BootConfig, BootEvent};

// ---------------- 简单文件日志(debug 模式启用) ----------------
static LOG_FILE: Mutex<Option<std::fs::File>> = Mutex::new(None);
static LOG_ENABLED: AtomicBool = AtomicBool::new(false);

pub(crate) fn log_msg(msg: &str) {
    if !LOG_ENABLED.load(Ordering::Relaxed) {
        return;
    }
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    if let Ok(mut guard) = LOG_FILE.lock() {
        if let Some(f) = guard.as_mut() {
            let _ = writeln!(f, "[{ts}] {msg}");
            let _ = f.flush();
        }
    }
}

macro_rules! log {
    ($($arg:tt)*) => { log_msg(&format!($($arg)*)) };
}

/// 家目录:macOS/Linux 用 $HOME,Windows 用 %USERPROFILE%
fn home_dir() -> PathBuf {
    std::env::var("HOME")
        .ok()
        .or_else(|| std::env::var("USERPROFILE").ok())
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_default())
}

fn init_logging(debug: bool) {
    if !debug {
        return;
    }
    let path: PathBuf = std::env::var("DSH_LOG").map(PathBuf::from).unwrap_or_else(|_| {
        home_dir().join("Library/Logs/DSH.log")
    });
    match OpenOptions::new().create(true).append(true).open(&path) {
        Ok(f) => {
            *LOG_FILE.lock().unwrap() = Some(f);
            LOG_ENABLED.store(true, Ordering::Relaxed);
            log!("========== DSH 启动 ==========");
            log!("日志文件: {}", path.display());
            std::panic::set_hook(Box::new(|info| {
                log!("PANIC: {info}");
                log!("backtrace:\n{:?}", std::backtrace::Backtrace::force_capture());
            }));
        }
        Err(e) => eprintln!("无法打开日志文件 {}: {e}", path.display()),
    }
}

// ---------------- 自举页(带 toast 与错误面板) ----------------
// 大胖鲸 logo 以 base64 内嵌(assets/boot-logo.b64,由 dsh-whale.jpg 生成)
const LOGO_B64: &str = include_str!("../assets/boot-logo.b64");

const BOOT_HTML: &str = r#"<!doctype html><html><head><meta charset="utf-8"><style>
html,body{height:100%}
:root{color-scheme:light dark;
  --bg:#f5f6f8;--fg:#1f2328;--sub:#6b7280;
  --toast-bg:#ffffff;--toast-bd:#d0d5dd;--toast-fg:#1f2328;
  --ok-bg:#e6f7ec;--ok-bd:#4caf7d;--ok-fg:#166534;
  --err-bg:#fdecec;--err-bd:#e07b7b;--err-fg:#b42318;
  --panel-bg:#ffffff;--panel-bd:#e07b7b;
  --pre-bg:#f0f1f3;--pre-fg:#4b5563;
  --btn-bg:#eceef1;--btn-fg:#1f2328;--btn-hover:#e2e5ea;
  --shadow:rgba(0,0,0,.12)}
@media (prefers-color-scheme: dark){
  :root{
  --bg:#111418;--fg:#c8ccd4;--sub:#6b7280;
  --toast-bg:#1f2937;--toast-bd:#374151;--toast-fg:#e5e7eb;
  --ok-bg:#0d3b26;--ok-bd:#1c7a45;--ok-fg:#7ee2a8;
  --err-bg:#3b0d14;--err-bd:#a13b3b;--err-fg:#ff9b9b;
  --panel-bg:#1a1d23;--panel-bd:#4b2a2a;
  --pre-bg:#0d0f12;--pre-fg:#9ca3af;
  --btn-bg:#262b33;--btn-fg:#d1d5db;--btn-hover:#31363f;
  --shadow:rgba(0,0,0,.45)}
}
body{margin:0;display:flex;flex-direction:column;align-items:center;justify-content:center;background:var(--bg);color:var(--fg);font-family:-apple-system,'PingFang SC','Helvetica Neue',sans-serif;user-select:none}
.logo{width:128px;height:128px;border-radius:26px;box-shadow:0 8px 30px var(--shadow);border:1px solid var(--toast-bd);margin-bottom:20px}
.title{font-size:22px;font-weight:700;letter-spacing:4px}
.sub{font-size:13px;color:var(--sub);margin-top:10px}
#toast{position:fixed;top:26px;left:50%;transform:translateX(-50%);padding:10px 20px;border-radius:22px;background:var(--toast-bg);border:1px solid var(--toast-bd);color:var(--toast-fg);font-size:14px;box-shadow:0 4px 18px var(--shadow);display:none;max-width:82vw;text-align:center;z-index:10}
#toast.success{background:var(--ok-bg);border-color:var(--ok-bd);color:var(--ok-fg)}
#toast.error{background:var(--err-bg);border-color:var(--err-bd);color:var(--err-fg)}
#errpanel{display:none;position:fixed;bottom:36px;left:50%;transform:translateX(-50%);width:min(660px,92vw);background:var(--panel-bg);border:1px solid var(--panel-bd);border-radius:10px;padding:14px 16px;font-size:13px;z-index:9}
#errpanel .msg{color:var(--err-fg);margin-bottom:10px;white-space:pre-wrap}
#errpanel .lbl{color:var(--sub);font-size:11px;margin-bottom:4px}
#errpanel pre{background:var(--pre-bg);padding:8px;border-radius:6px;white-space:pre-wrap;word-break:break-all;max-height:150px;overflow:auto;font-size:11px;color:var(--pre-fg);margin:0 0 10px}
#errpanel button{padding:6px 16px;border-radius:6px;border:1px solid var(--toast-bd);background:var(--btn-bg);color:var(--btn-fg);cursor:pointer;font-size:13px}
#errpanel button:hover{background:var(--btn-hover)}
</style></head><body>
<img class="logo" src="data:image/png;base64,__LOGO_B64__" alt="DSH">
<div class="title">DSH</div>
<div class="sub" id="sub">正在连接 dsh 后端…</div>
<div id="toast"></div>
<div id="errpanel">
  <div class="msg" id="errmsg"></div>
  <div class="lbl">后端日志尾部(~/Library/Logs/DSH-backend.log)</div>
  <pre id="errlog"></pre>
  <button onclick="dshRetry()">重试</button>
</div>
<script>
var toastTimer=null;
function showToast(text,kind){
  var t=document.getElementById('toast');
  t.textContent=text;
  t.className=kind||'';
  t.style.display='block';
  clearTimeout(toastTimer);
  if(kind==='success'){toastTimer=setTimeout(function(){t.style.display='none';},900);}
  else if(kind==='info'){toastTimer=setTimeout(function(){t.style.display='none';},2600);}
}
function showError(msg,logtail){
  document.getElementById('errmsg').textContent=msg;
  document.getElementById('errlog').textContent=logtail||'';
  document.getElementById('errpanel').style.display='block';
}
function clearError(){document.getElementById('errpanel').style.display='none';}
function dshRetry(){ if(window.ipc){window.ipc.postMessage(JSON.stringify({cmd:'retry'}));} }
function setSub(t){document.getElementById('sub').textContent=t;}
</script></body></html>"#;

/// 把字符串安全地嵌进 JS 字面量
fn js_quote(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_else(|_| "\"\"".to_string())
}

fn eval(wv: &WebView, js: &str) {
    let _ = wv.evaluate_script(js);
    log!("[页面] {js}");
}

fn port_of(url: &str) -> u16 {
    url.split("://")
        .nth(1)
        .unwrap_or("")
        .split('/')
        .next()
        .unwrap_or("")
        .rsplit_once(':')
        .and_then(|(_, p)| p.parse().ok())
        .unwrap_or(3080)
}

fn main() {
    let debug = std::env::var("DSH_DEBUG").is_ok() || std::env::args().any(|a| a == "--debug");
    let devtools = std::env::var("DSH_DEVTOOLS").is_ok();
    init_logging(debug);

    let url = std::env::var("DSH_URL").unwrap_or_else(|_| "http://127.0.0.1:3080".to_string());
    let autostart = std::env::var("DSH_NO_AUTOSTART").is_err();
    let timeout = std::env::var("DSH_BACKEND_TIMEOUT")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(30);
    let backend_log = std::env::var("DSH_BACKEND_LOG").map(PathBuf::from).unwrap_or_else(|_| {
        home_dir().join("Library/Logs/DSH-backend.log")
    });
    log!("url={url} autostart={autostart} timeout={timeout}s devtools={devtools}");

    let cfg = Arc::new(BootConfig {
        url: url.clone(),
        port: port_of(&url),
        autostart,
        timeout: Duration::from_secs(timeout),
        backend_log,
    });

    // 自举管理线程:顺序执行 Start/Retry
    let (cmd_tx, cmd_rx) = mpsc::channel::<BootCmd>();
    let (evt_tx, evt_rx) = mpsc::channel::<BootEvent>();
    {
        let cfg = cfg.clone();
        std::thread::spawn(move || {
            while let Ok(cmd) = cmd_rx.recv() {
                match cmd {
                    BootCmd::Start => log!("收到 Start"),
                    BootCmd::Retry => log!("收到 Retry"),
                }
                backend::run_bootstrap(&cfg, &evt_tx);
            }
        });
    }
    let _ = cmd_tx.send(BootCmd::Start);

    // 窗口
    let event_loop = EventLoop::new();
    let window = WindowBuilder::new()
        .with_title("DSH")
        .with_inner_size(LogicalSize::new(1100.0, 760.0))
        .build(&event_loop)
        .expect("创建窗口失败");
    log!("窗口已创建 {:?}", window.inner_size());

    // WebView:自举页 + IPC(重试按钮)
    let webview_shared: Arc<Mutex<Option<WebView>>> = Arc::new(Mutex::new(None));
    let ipc_webview = webview_shared.clone();
    let ipc_cmd_tx = cmd_tx.clone();
    let boot_html = BOOT_HTML.replace("__LOGO_B64__", LOGO_B64.trim());
    let mut builder = WebViewBuilder::new()
        .with_html(boot_html)
        .with_navigation_handler(Box::new(|nav_url| {
            log!("导航: {nav_url}");
            true
        }))
        .with_on_page_load_handler(Box::new(|event, page_url| {
            let phase = match event {
                PageLoadEvent::Started => "开始",
                PageLoadEvent::Finished => "完成",
            };
            log!("页面加载{phase}: {page_url}");
        }))
        .with_ipc_handler(Box::new(move |req: wry::http::Request<String>| {
            let body = req.into_body();
            log!("IPC: {body}");
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&body) {
                if v.get("cmd").and_then(|c| c.as_str()) == Some("retry") {
                    if let Some(wv) = ipc_webview.lock().unwrap().as_ref() {
                        eval(wv, "clearError();showToast('打开中…','info');");
                    }
                    let _ = ipc_cmd_tx.send(BootCmd::Retry);
                }
            }
        }));
    if devtools {
        builder = builder.with_devtools(true);
    }
    let webview = builder.build(&window).expect("创建 WebView 失败");
    log!("WebView 已创建(自举页)");
    if devtools {
        webview.open_devtools();
        log!("已打开开发者工具");
    }
    *webview_shared.lock().unwrap() = Some(webview);

    // 事件循环
    let mut modifiers = ModifiersState::empty();
    let mut loaded = false;
    let mut pending_load: Option<Instant> = None;
    let wv_loop = webview_shared.clone();
    let cfg_loop = cfg.clone();

    event_loop.run(move |event, _, control_flow| {
        // 常驻 200ms 节拍:驱动 toast 秒数刷新、事件接收
        *control_flow = ControlFlow::WaitUntil(Instant::now() + Duration::from_millis(200));

        if let Event::WindowEvent { event, .. } = event {
            match event {
                WindowEvent::CloseRequested => {
                    log!("窗口关闭,退出");
                    *control_flow = ControlFlow::Exit;
                }
                WindowEvent::Focused(focused) => log!("焦点: {focused}"),
                WindowEvent::ModifiersChanged(m) => modifiers = m,
                WindowEvent::KeyboardInput {
                    event:
                        KeyEvent {
                            state: ElementState::Pressed,
                            logical_key: Key::Character(c),
                            ..
                        },
                    ..
                } => {
                    if modifiers.super_key() && (c == "r" || c == "R") {
                        log!("Cmd+R 刷新");
                        if let Some(wv) = wv_loop.lock().unwrap().as_ref() {
                            let _ = wv.reload();
                        }
                    }
                }
                _ => {}
            }
        }

        // 处理自举事件 → 驱动 toast / 错误面板 / 载入
        while let Ok(evt) = evt_rx.try_recv() {
            match evt {
                BootEvent::Toast { text, kind } => {
                    log!("toast[{kind}]: {text}");
                    if let Some(wv) = wv_loop.lock().unwrap().as_ref() {
                        eval(wv, &format!("showToast({},{});", js_quote(&text), js_quote(kind)));
                    }
                }
                BootEvent::ErrorPanel { message, log_tail } => {
                    log!("错误面板: {message}");
                    if let Some(wv) = wv_loop.lock().unwrap().as_ref() {
                        eval(
                            wv,
                            &format!("showError({},{});", js_quote(&message), js_quote(&log_tail)),
                        );
                    }
                }
                BootEvent::Ready => {
                    if !loaded {
                        pending_load = Some(Instant::now() + Duration::from_millis(800));
                    }
                }
            }
        }

        // 就绪后延迟 800ms(让"已就绪" toast 亮一下)再载入真实页面
        if let Some(t) = pending_load {
            if Instant::now() >= t {
                pending_load = None;
                loaded = true;
                log!("载入真实页面: {}", cfg_loop.url);
                if let Some(wv) = wv_loop.lock().unwrap().as_ref() {
                    let _ = wv.load_url(&cfg_loop.url);
                }
            }
        }
    });
}
