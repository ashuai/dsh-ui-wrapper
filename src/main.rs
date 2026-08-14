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
/// 常驻 panic 日志(不依赖 DSH_DEBUG):任何崩溃都会写入 ~/Library/Logs/DSH-panic.log
static PANIC_FILE: Mutex<Option<std::fs::File>> = Mutex::new(None);

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
        }
        Err(e) => eprintln!("无法打开日志文件 {}: {e}", path.display()),
    }
}

/// 常驻 panic hook:写 DSH-panic.log(总是),DSH.log(debug 时)
fn arm_panic_hook() {
    let path = home_dir().join("Library/Logs/DSH-panic.log");
    if let Ok(f) = OpenOptions::new().create(true).append(true).open(&path) {
        *PANIC_FILE.lock().unwrap() = Some(f);
    }
    std::panic::set_hook(Box::new(|info| {
        let msg = format!(
            "[{}] PANIC: {info}\nbacktrace:\n{:?}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis())
                .unwrap_or(0),
            std::backtrace::Backtrace::force_capture()
        );
        if let Ok(mut g) = PANIC_FILE.lock() {
            if let Some(f) = g.as_mut() {
                let _ = writeln!(f, "{msg}");
                let _ = f.flush();
            }
        }
        log_msg(&msg);
    }));
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

/// macOS 主菜单:没有它,Cmd+V/Cmd+C/Cmd+X/Cmd+Z 等快捷键无处路由(只能右键菜单)。
/// 建标准的 App(Quit)+ Edit(Undo/Redo/Cut/Copy/Paste/Select All)菜单,
/// 动作直接 target 到 WKWebView(响应链对 WKWebView 不可靠,菜单项会点亮但点击无反应)。
/// webview_ptr:WKWebView 的裸指针(0 = 未找到,此时退化为 nil target 走响应链)。
#[cfg(target_os = "macos")]
fn setup_main_menu(webview_ptr: usize) {
    use objc2::runtime::Sel;
    use objc2_app_kit::{NSApplication, NSMenu, NSMenuItem, NSEventModifierFlags};
    use objc2_foundation::{MainThreadMarker, NSString};

    let mtm = MainThreadMarker::new().expect("主线程");
    let app = NSApplication::sharedApplication(mtm);
    let main_menu = NSMenu::new(mtm);

    // ---- App 菜单(标题留空,macOS 会自动显示应用名)----
    let app_item = NSMenuItem::new(mtm);
    let app_menu = NSMenu::new(mtm);
    let quit_item = NSMenuItem::new(mtm);
    quit_item.setTitle(&NSString::from_str("Quit DSH"));
    quit_item.setKeyEquivalent(&NSString::from_str("q"));
    quit_item.setKeyEquivalentModifierMask(NSEventModifierFlags::Command);
    // terminate: 交给响应链/NSApp 退出
    unsafe {
        quit_item.setAction(Some(objc2::sel!(terminate:)));
    }
    app_menu.addItem(&quit_item);
    app_item.setSubmenu(Some(&app_menu));
    main_menu.addItem(&app_item);

    // ---- Edit 菜单(快捷键经它路由到 WKWebView)----
    let edit_item = NSMenuItem::new(mtm);
    let edit_menu = NSMenu::new(mtm);
    edit_menu.setTitle(&NSString::from_str("Edit"));
    edit_item.setTitle(&NSString::from_str("Edit"));
    // 关闭自动置灰;target 保持 nil,动作走响应链(直接 target WKWebView 会触发
    // WebKit doneWithKeyEvent 回调 → tao 跨线程 panic → 崩溃,见 DSH-panic.log)
    edit_menu.setAutoenablesItems(false);
    let _ = webview_ptr; // 保留签名(供未来排查),当前不使用
    for (title, sel, key) in [
        ("Undo", "undo:", "z"),
        ("Redo", "redo:", "Z"),
        ("", "", ""), // 分隔线
        ("Cut", "cut:", "x"),
        ("Copy", "copy:", "c"),
        ("Paste", "paste:", "v"),
        ("Select All", "selectAll:", "a"),
    ] {
        if sel.is_empty() {
            edit_menu.addItem(&NSMenuItem::separatorItem(mtm));
            continue;
        }
        let item = NSMenuItem::new(mtm);
        item.setTitle(&NSString::from_str(title));
        item.setKeyEquivalent(&NSString::from_str(key));
        item.setKeyEquivalentModifierMask(NSEventModifierFlags::Command);
        // sel! 只吃字面量,变量要用 Sel::register(否则生成名为 "sel" 的假选择器,点了没反应)
        let sel_c = std::ffi::CString::new(sel).unwrap();
        unsafe {
            item.setAction(Some(Sel::register(&sel_c)));
        }
        edit_menu.addItem(&item);
    }
    edit_item.setSubmenu(Some(&edit_menu));
    main_menu.addItem(&edit_item);

    app.setMainMenu(Some(&main_menu));
    log!("已安装 macOS 主菜单(Edit 项直接 target WKWebView, webview_ptr={webview_ptr})");
}

#[cfg(not(target_os = "macos"))]
fn setup_main_menu(_webview_ptr: usize) {}

/// 窗口获得焦点时把 WKWebView 设为首响应者,保证 Edit 菜单(响应链)的
/// Cut/Copy/Paste 能送达 WebKit(右键能用=同机制;直接 target 会崩,故不用)
#[cfg(target_os = "macos")]
fn ensure_webview_focus(wk_ptr: usize) {
    if wk_ptr == 0 {
        return;
    }
    use objc2_app_kit::NSResponder;
    use objc2_web_kit::WKWebView;
    unsafe {
        let wk: &WKWebView = &*(wk_ptr as *const WKWebView);
        if let Some(win) = wk.window() {
            win.makeFirstResponder(Some(&*(wk as *const WKWebView as *const NSResponder)));
            log_msg("已确保 WKWebView 为首响应者");
        }
    }
}

#[cfg(not(target_os = "macos"))]
fn ensure_webview_focus(_wk_ptr: usize) {}

// ============================================================
// macOS 右键菜单接管:只保留 Cut / Copy / Paste
// ============================================================
#[cfg(target_os = "macos")]
mod context_menu {
    use block2::Block;
    use objc2::define_class;
    use objc2::msg_send;
    use objc2::rc::Retained;
    use objc2::runtime::{AnyClass, AnyObject, NSObject, ProtocolObject, Sel};
    use objc2::ClassType;
    use objc2::MainThreadOnly;
    use objc2_app_kit::{
        NSMenuItem, NSModalResponse, NSModalResponseOK, NSOpenPanel, NSView,
    };
    use objc2_foundation::{MainThreadMarker, NSArray, NSObjectProtocol, NSURL, NSString};
    use objc2_web_kit::{
        WKFrameInfo, WKMediaCaptureType, WKOpenPanelParameters, WKPermissionDecision,
        WKSecurityOrigin, WKUIDelegate, WKWebView,
    };

    // 我们的 UIDelegate:接管右键菜单(仅 Cut/Copy/Paste),其余方法复刻 wry 的行为
    // (文件上传面板、媒体权限直接 Grant),保证 dsh 附件上传等功能不受影响。
    define_class!(
        #[unsafe(super(NSObject))]
        #[thread_kind = MainThreadOnly]
        pub struct DshContextMenuDelegate;

        unsafe impl NSObjectProtocol for DshContextMenuDelegate {}

        unsafe impl WKUIDelegate for DshContextMenuDelegate {
            // 右键菜单:只返回 Cut/Copy/Paste(替换 WebKit 默认菜单,拼写检查等全部移除)
            #[unsafe(method(webView:contextMenuConfigurationForElement:completionHandler:))]
            fn context_menu(
                &self,
                _webview: &WKWebView,
                _element: *const AnyObject, // WKContextMenuElementInfo(绑定为空壳,用原始指针)
                handler: &Block<dyn Fn(*mut AnyObject)>,
            ) {
                crate::log_msg("[右键菜单] contextMenuConfigurationForElement 回调触发");
                unsafe {
                    let mtm = MainThreadMarker::new().unwrap();
                    let cut = menu_item(mtm, "Cut", "cut:");
                    let copy = menu_item(mtm, "Copy", "copy:");
                    let paste = menu_item(mtm, "Paste", "paste:");
                    let items = NSArray::from_slice(&[&*cut, &*copy, &*paste]);

                    // WKContextMenuConfiguration 不在 objc2-web-kit 绑定中,运行时查找
                    let cls = AnyClass::get(c"WKContextMenuConfiguration")
                        .expect("找不到 WKContextMenuConfiguration");
                    let config: *mut AnyObject = msg_send![cls, new];
                    let _: () = msg_send![config, setMenuItems: &*items];
                    // 不 release:把 +1 交给 WebKit 持有(autorelease 会过早回收导致回退默认菜单)

                    (*handler).call((config,));
                    crate::log_msg("[右键菜单] 已提交自定义配置(Cut/Copy/Paste)");
                }
            }

            /// 文件上传面板(复刻 wry 行为,保证 <input type=file> 附件可用)
            #[unsafe(method(webView:runOpenPanelWithParameters:initiatedByFrame:completionHandler:))]
            fn run_open_panel(
                &self,
                _webview: &WKWebView,
                open_panel_params: &WKOpenPanelParameters,
                _frame: &WKFrameInfo,
                handler: &Block<dyn Fn(*const NSArray<NSURL>)>,
            ) {
                unsafe {
                    if let Some(mtm) = MainThreadMarker::new() {
                        let open_panel = NSOpenPanel::openPanel(mtm);
                        open_panel.setCanChooseFiles(true);
                        open_panel
                            .setAllowsMultipleSelection(open_panel_params.allowsMultipleSelection());
                        open_panel.setCanChooseDirectories(open_panel_params.allowsDirectories());
                        let ok: NSModalResponse = open_panel.runModal();
                        if ok == NSModalResponseOK {
                            let urls = open_panel.URLs();
                            (*handler).call((Retained::as_ptr(&urls),));
                        } else {
                            (*handler).call((std::ptr::null(),));
                        }
                    }
                }
            }

            /// 媒体权限:直接 Grant(与 wry 默认一致)
            #[unsafe(method(webView:requestMediaCapturePermissionForOrigin:initiatedByFrame:type:decisionHandler:))]
            fn media_capture(
                &self,
                _webview: &WKWebView,
                _origin: &WKSecurityOrigin,
                _frame: &WKFrameInfo,
                _capture_type: WKMediaCaptureType,
                decision_handler: &Block<dyn Fn(WKPermissionDecision)>,
            ) {
                (*decision_handler).call((WKPermissionDecision::Grant,));
            }
        }
    );

    fn menu_item(mtm: MainThreadMarker, title: &str, sel: &str) -> Retained<NSMenuItem> {
        let item = NSMenuItem::new(mtm);
        item.setTitle(&NSString::from_str(title));
        let sel_c = std::ffi::CString::new(sel).unwrap();
        unsafe {
            item.setAction(Some(Sel::register(&sel_c)));
        }
        item
    }

    /// 找到窗口里的 WKWebView,替换 UIDelegate 为我们自己;
    /// 返回 WKWebView 裸指针(0 = 未找到),供主菜单把 Edit 项直接 target 到它
    pub fn install(window: &tao::window::Window) -> usize {
        use wry::raw_window_handle::{HasWindowHandle, RawWindowHandle};
        let Ok(handle) = window.window_handle() else {
            return 0;
        };
        let ns_view_ptr = match handle.as_raw() {
            RawWindowHandle::AppKit(w) => w.ns_view.as_ptr(),
            _ => return 0,
        };
        unsafe {
            let ns_view: &NSView = &*(ns_view_ptr as *const NSView);
            let Some(ns_window) = ns_view.window() else { return 0 };
            let Some(content) = ns_window.contentView() else { return 0 };
            let subviews = content.subviews();
            let count = subviews.count();
            for i in 0..count {
                let sub = subviews.objectAtIndex(i); // Retained<NSView>
                if sub.isKindOfClass(&WKWebView::class()) {
                    let wk: &WKWebView = &*(&*sub as *const NSView as *const WKWebView);
                    let mtm = MainThreadMarker::new().unwrap();
                    let delegate = mtm.alloc::<DshContextMenuDelegate>();
                    let delegate: Retained<DshContextMenuDelegate> =
                        msg_send![delegate, init];
                    let proto = ProtocolObject::from_ref(&*delegate);
                    wk.setUIDelegate(Some(proto));
                    // UIDelegate 是弱引用,必须自己持有;forget 使其存活到进程结束
                    std::mem::forget(delegate);
                    crate::log_msg("已接管右键菜单(仅保留 Cut/Copy/Paste)");
                    return &*wk as *const WKWebView as usize;
                }
            }
            crate::log_msg("[右键菜单] 未找到 WKWebView,跳过接管");
        }
        0
    }
}

#[cfg(not(target_os = "macos"))]
mod context_menu {
    pub fn install(_window: &tao::window::Window) -> usize {
        0
    }
}

fn main() {
    let debug = std::env::var("DSH_DEBUG").is_ok() || std::env::args().any(|a| a == "--debug");
    let devtools = std::env::var("DSH_DEVTOOLS").is_ok();
    init_logging(debug);
    arm_panic_hook(); // 常驻:任何崩溃都进 ~/Library/Logs/DSH-panic.log

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

    // 接管右键菜单(仅保留 Cut/Copy/Paste),并拿回 WKWebView 指针
    let wk_ptr = context_menu::install(&window);
    // macOS 主菜单(Edit 项直接 target 到 WKWebView),须在主线程
    setup_main_menu(wk_ptr);

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
                WindowEvent::Focused(focused) => {
                    log!("焦点: {focused}");
                    if focused {
                        ensure_webview_focus(wk_ptr);
                    }
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
