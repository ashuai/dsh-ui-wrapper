// DSH — 极简壳:用系统浏览器核心(WebKit / WKWebView)打开本机 dsh web 界面。
//
// 修复说明:必须用 build_as_child 方式挂载 WebView(子视图,不替换窗口 contentView)。
// wry 默认的 build() 会把 contentView 换成自己的 WryWebViewParent,而 winit 的
// 窗口 delegate 假定 contentView 永远是 WinitView —— 窗口失焦(resignKey)时按
// WinitView 解析 WebView 对象 → 野指针 → EXC_BAD_ACCESS 崩溃。改为子视图后
// contentView 保持原样,失焦事件走 winit 自己的视图,不再崩溃。
//
// Debug 模式(输出更多日志):
//   DSH_DEBUG=1 ./DSH           # 开启文件日志(默认 ~/Library/Logs/DSH.log)
//   DSH_LOG=/path/to.log ./DSH  # 指定日志文件
//   DSH_DEVTOOLS=1 ./DSH        # 同时打开 WebKit 开发者工具(页面侧诊断)
// 日志包含:生命周期、页面加载、导航、焦点变化、缩放、刷新、panic 回溯。

use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use wry::{PageLoadEvent, Rect, WebView, WebViewBuilder};
use winit::{
    application::ApplicationHandler,
    dpi::{LogicalPosition, LogicalSize},
    event::{ElementState, KeyEvent, WindowEvent},
    event_loop::{ActiveEventLoop, EventLoop},
    keyboard::{Key, ModifiersState},
    window::{Window, WindowId},
};

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

// ---------------- 应用 ----------------
struct App {
    window: Option<Window>,
    webview: Option<WebView>,
    modifiers: ModifiersState,
    devtools: bool,
}

impl App {
    fn resize_webview(&self, size: winit::dpi::PhysicalSize<u32>) {
        if let (Some(w), Some(wv)) = (&self.window, &self.webview) {
            let size = size.to_logical::<u32>(w.scale_factor());
            let _ = wv.set_bounds(Rect {
                position: LogicalPosition::new(0, 0).into(),
                size: LogicalSize::new(size.width, size.height).into(),
            });
        }
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        log!("resumed(window={})", self.window.is_some());
        if self.window.is_some() {
            return; // 已创建过窗口(例如 macOS 重新激活)
        }
        let window = event_loop
            .create_window(
                Window::default_attributes()
                    .with_title("DSH")
                    .with_inner_size(LogicalSize::new(1100.0, 760.0)),
            )
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
        if self.devtools {
            builder = builder.with_devtools(true);
        }
        // 关键:build_as_child 而不是 build()(避免替换 contentView 导致失焦崩溃)
        let webview = builder.build_as_child(&window).expect("创建 WebView 失败");
        log!("WebView 已创建(build_as_child)");
        if self.devtools {
            webview.open_devtools();
            log!("已打开开发者工具");
        }

        let size = window.inner_size();
        self.window = Some(window);
        self.webview = Some(webview);
        self.resize_webview(size);
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: WindowId,
        event: WindowEvent,
    ) {
        match event {
            WindowEvent::CloseRequested => {
                log!("窗口关闭,退出");
                event_loop.exit();
            }
            WindowEvent::Resized(size) => {
                log!("窗口缩放: {size:?}");
                self.resize_webview(size);
            }
            WindowEvent::Focused(focused) => {
                log!("焦点: {focused}");
            }
            WindowEvent::ModifiersChanged(m) => self.modifiers = m.state(),
            WindowEvent::KeyboardInput {
                event:
                    KeyEvent {
                        state: ElementState::Pressed,
                        logical_key: Key::Character(c),
                        ..
                    },
                ..
            } => {
                if self.modifiers.super_key() && (c == "r" || c == "R") {
                    log!("Cmd+R 刷新");
                    if let Some(wv) = &self.webview {
                        let _ = wv.reload();
                    }
                }
            }
            _ => {}
        }
    }
}

fn main() {
    let debug = std::env::var("DSH_DEBUG").is_ok()
        || std::env::args().any(|a| a == "--debug");
    let devtools = std::env::var("DSH_DEVTOOLS").is_ok();
    init_logging(debug);
    log!("debug={debug} devtools={devtools} url={URL}");

    let event_loop = EventLoop::new().expect("创建事件循环失败");
    let mut app = App {
        window: None,
        webview: None,
        modifiers: ModifiersState::empty(),
        devtools,
    };
    event_loop.run_app(&mut app).expect("运行事件循环失败");
    log!("正常退出");
}
