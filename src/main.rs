// DSH — 极简壳:用系统浏览器核心(WebKit / WKWebView)打开本机 dsh web 界面。
//
// 窗口/事件循环用 tao(Tauri 维护的 winit 分支)+ wry 默认 build():
// 与 Tauri 相同的组合。这样:
//   1. 输入(含中文输入法 IME)走正常路径——wry 默认模式会 makeFirstResponder;
//     之前为绕崩溃用的 build_as_child(子视图)跳过这一步,击键/输入法会卡。
//   2. 不会像 winit 那样在 contentView 被替换后,失焦时按错误类型解析而崩溃。
//
// Debug 模式(输出更多日志):
//   DSH_DEBUG=1 ./DSH           # 开启文件日志(默认 ~/Library/Logs/DSH.log)
//   DSH_LOG=/path/to.log ./DSH  # 指定日志文件
//   DSH_DEVTOOLS=1 ./DSH        # 同时打开 WebKit 开发者工具(页面侧诊断)

use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use tao::{
    dpi::LogicalSize,
    event::{ElementState, Event, KeyEvent, WindowEvent},
    event_loop::{ControlFlow, EventLoop},
    keyboard::{Key, ModifiersState},
    window::WindowBuilder,
};
use wry::{PageLoadEvent, WebViewBuilder};

const URL: &str = "http://127.0.0.1:3080";

// ---------------- 简单文件日志(debug 模式启用) ----------------
static LOG_FILE: Mutex<Option<std::fs::File>> = Mutex::new(None);
static LOG_ENABLED: AtomicBool = AtomicBool::new(false);

fn log_msg(msg: &str) {
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

fn init_logging(debug: bool) {
    if !debug {
        return;
    }
    let path: PathBuf = std::env::var("DSH_LOG").map(PathBuf::from).unwrap_or_else(|_| {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
        PathBuf::from(home).join("Library/Logs/DSH.log")
    });
    match OpenOptions::new().create(true).append(true).open(&path) {
        Ok(f) => {
            *LOG_FILE.lock().unwrap() = Some(f);
            LOG_ENABLED.store(true, Ordering::Relaxed);
            log!("========== DSH 启动 ==========");
            log!("日志文件: {}", path.display());
            // panic 也写进日志,便于定位
            std::panic::set_hook(Box::new(|info| {
                log!("PANIC: {info}");
                log!("backtrace:\n{:?}", std::backtrace::Backtrace::force_capture());
            }));
        }
        Err(e) => eprintln!("无法打开日志文件 {}: {e}", path.display()),
    }
}

fn main() {
    let debug = std::env::var("DSH_DEBUG").is_ok()
        || std::env::args().any(|a| a == "--debug");
    let devtools = std::env::var("DSH_DEVTOOLS").is_ok();
    init_logging(debug);
    log!("debug={debug} devtools={devtools} url={URL}");

    let event_loop = EventLoop::new();
    let window = WindowBuilder::new()
        .with_title("DSH")
        .with_inner_size(LogicalSize::new(1100.0, 760.0))
        .build(&event_loop)
        .expect("创建窗口失败");
    log!("窗口已创建 {:?}", window.inner_size());

    let mut builder = WebViewBuilder::new()
        .with_url(URL)
        .with_navigation_handler(Box::new(|url| {
            log!("导航: {url}");
            true // 放行所有导航
        }))
        .with_on_page_load_handler(Box::new(|event, url| {
            let phase = match event {
                PageLoadEvent::Started => "开始",
                PageLoadEvent::Finished => "完成",
            };
            log!("页面加载{phase}: {url}");
        }));
    if devtools {
        builder = builder.with_devtools(true);
    }
    // 默认 build(非 child):输入/输入法走正常路径,窗口缩放自动适配
    let webview = builder.build(&window).expect("创建 WebView 失败");
    log!("WebView 已创建(默认 build)");
    if devtools {
        webview.open_devtools();
        log!("已打开开发者工具");
    }

    let mut modifiers = ModifiersState::empty();
    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;
        if let Event::WindowEvent { event, .. } = event {
            match event {
                WindowEvent::CloseRequested => {
                    log!("窗口关闭,退出");
                    *control_flow = ControlFlow::Exit;
                }
                WindowEvent::Focused(focused) => {
                    log!("焦点: {focused}");
                }
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
                        let _ = webview.reload();
                    }
                }
                _ => {}
            }
        }
    });
}
