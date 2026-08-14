// DSH — 极简壳:用系统浏览器核心(WebKit / WKWebView)打开本机 dsh web 界面。
// 不画任何界面:窗口里就是 dsh 自己的网页(http://127.0.0.1:3080)。
// 用法:确保 dsh 后端在 3080 端口运行(通常 `dsh web`),然后启动本程序。
// Cmd+R 刷新页面(后端重启后很好用);关窗即退出。

use wry::{WebView, WebViewBuilder};
use winit::{
    application::ApplicationHandler,
    event::{ElementState, KeyEvent, WindowEvent},
    event_loop::{ActiveEventLoop, EventLoop},
    keyboard::{Key, ModifiersState},
    window::{Window, WindowId},
};

const URL: &str = "http://127.0.0.1:3080";

#[derive(Default)]
struct App {
    window: Option<Window>,
    webview: Option<WebView>,
    modifiers: ModifiersState,
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return; // 已创建过窗口(例如 macOS 重新激活)
        }
        let window = event_loop
            .create_window(
                Window::default_attributes()
                    .with_title("DSH")
                    .with_inner_size(winit::dpi::LogicalSize::new(1100.0, 760.0)),
            )
            .expect("创建窗口失败");
        let webview = WebViewBuilder::new()
            .with_url(URL)
            .build(&window)
            .expect("创建 WebView 失败");
        self.window = Some(window);
        self.webview = Some(webview);
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: WindowId,
        event: WindowEvent,
    ) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
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
                    if let Some(wv) = &self.webview {
                        let _ = wv.evaluate_script("location.reload()");
                    }
                }
            }
            _ => {}
        }
    }
}

fn main() {
    let event_loop = EventLoop::new().expect("创建事件循环失败");
    let mut app = App::default();
    event_loop
        .run_app(&mut app)
        .expect("运行事件循环失败");
}
