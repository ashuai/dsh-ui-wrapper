// 后端自举:探测 3080 → 没有就按 bunx/pnpm/npm/dsh 拉起 dsh web → 轮询就绪。
// 全部逻辑在后台线程;通过 mpsc 向主线程上报状态事件(toast / 错误 / 就绪)。

use std::net::{SocketAddr, TcpStream};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::Sender;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use crate::log_msg;

pub enum BootCmd {
    /// 启动时执行一次
    Start,
    /// 错误面板上的"重试"按钮触发
    Retry,
}

pub enum BootEvent {
    /// 界面 toast:kind = info / success / error
    Toast { text: String, kind: &'static str },
    /// 错误面板:可读信息 + 后端日志尾部
    ErrorPanel { message: String, log_tail: String },
    /// 端口就绪,主线程随后 load_url
    Ready,
}

pub struct BootConfig {
    pub url: String,
    pub port: u16,
    pub autostart: bool,
    pub timeout: Duration,
    pub backend_log: PathBuf,
}

fn blog(msg: &str) {
    log_msg(msg);
}

/// TCP 探测:300ms 超时,毫秒级返回
pub fn probe(port: u16) -> bool {
    let addr: SocketAddr = format!("127.0.0.1:{port}").parse().unwrap_or_else(|_| {
        SocketAddr::from(([127, 0, 0, 1], port))
    });
    TcpStream::connect_timeout(&addr, Duration::from_millis(300)).is_ok()
}

/// 查可执行文件:先查 PATH,再兜底查常见安装目录(双击启动 .app 时 PATH 不含 Homebrew)。
/// 按 OS 分支:Unix 检查执行位;Windows 尝试 .exe/.cmd/.bat,用 ';' 分隔 PATH。
#[cfg(not(windows))]
fn is_executable(p: &PathBuf) -> bool {
    use std::os::unix::fs::PermissionsExt;
    p.is_file()
        && p.metadata()
            .map(|m| m.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
}

#[cfg(windows)]
fn is_executable(p: &PathBuf) -> bool {
    p.is_file() // Windows 无执行位,存在即可;候选由 which 展开
}

#[cfg(windows)]
fn path_sep() -> char {
    ';'
}
#[cfg(not(windows))]
fn path_sep() -> char {
    ':'
}

/// 常见安装目录兜底(双击启动/极简 PATH 场景)
fn fallback_dirs() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = Vec::new();
    #[cfg(target_os = "macos")]
    {
        dirs.extend(
            ["/opt/homebrew/bin", "/usr/local/bin", "/usr/bin", "/bin"]
                .map(PathBuf::from),
        );
    }
    #[cfg(target_os = "linux")]
    {
        dirs.extend(
            ["/usr/local/bin", "/usr/bin", "/bin", "/home/linuxbrew/.linuxbrew/bin"]
                .map(PathBuf::from),
        );
    }
    #[cfg(windows)]
    {
        if let Ok(pf) = std::env::var("ProgramFiles") {
            dirs.push(PathBuf::from(pf).join("nodejs"));
        }
        if let Ok(app) = std::env::var("APPDATA") {
            dirs.push(PathBuf::from(app).join("npm"));
        }
        if let Ok(lapp) = std::env::var("LOCALAPPDATA") {
            dirs.push(PathBuf::from(lapp).join("Programs"));
        }
    }
    dirs
}

fn which(cmd: &str) -> Option<PathBuf> {
    let mut dirs: Vec<PathBuf> = Vec::new();
    if let Ok(paths) = std::env::var("PATH") {
        for d in paths.split(path_sep()) {
            if !d.is_empty() {
                dirs.push(PathBuf::from(d));
            }
        }
    }
    dirs.extend(fallback_dirs());
    for dir in dirs {
        #[cfg(windows)]
        for cand in [
            format!("{cmd}.exe"),
            format!("{cmd}.cmd"),
            format!("{cmd}.bat"),
            cmd.to_string(),
        ] {
            let p = dir.join(&cand);
            if is_executable(&p) {
                return Some(p);
            }
        }
        #[cfg(not(windows))]
        {
            let p = dir.join(cmd);
            if is_executable(&p) {
                return Some(p);
            }
        }
    }
    None
}

/// 按序找启动器:返回 (显示名, 可执行文件绝对路径, 参数)
pub fn find_runner() -> Option<(&'static str, PathBuf, Vec<&'static str>)> {
    if let Some(p) = which("bunx") {
        return Some(("bunx", p, vec!["@deepseek-ai/dsh", "web"]));
    }
    if let Some(p) = which("pnpm") {
        return Some(("pnpm", p, vec!["dlx", "@deepseek-ai/dsh", "web"]));
    }
    if let Some(p) = which("npm") {
        return Some(("npm", p, vec!["npx", "@deepseek-ai/dsh", "web"]));
    }
    if let Some(p) = which("dsh") {
        return Some(("dsh", p, vec!["web"]));
    }
    None
}

fn log_tail(path: &PathBuf, n: usize) -> String {
    let Ok(content) = std::fs::read_to_string(path) else {
        return "(后端日志文件尚不存在)".to_string();
    };
    let lines: Vec<&str> = content.lines().collect();
    let start = lines.len().saturating_sub(n);
    lines[start..].join("\n")
}

/// 成功拉起的后端子进程句柄:App 存活期间持有,退出时随 App 一起杀
static BACKEND_CHILD: OnceLock<Mutex<Option<Child>>> = OnceLock::new();

fn backend_slot() -> &'static Mutex<Option<Child>> {
    BACKEND_CHILD.get_or_init(|| Mutex::new(None))
}

/// Unix:让子进程成为新进程组的组长(pgid == pid),便于整组终止
#[cfg(unix)]
fn spawn_in_group(cmd: &mut Command) {
    use std::os::unix::process::CommandExt;
    cmd.process_group(0);
}
#[cfg(not(unix))]
fn spawn_in_group(_cmd: &mut Command) {}

/// 终止整个进程组:SIGTERM → 最多等 2s → SIGKILL
#[cfg(unix)]
fn terminate_group(child: &mut Child) {
    let pid = child.id() as i32;
    unsafe {
        libc::kill(-pid, libc::SIGTERM);
    }
    for _ in 0..40 {
        match child.try_wait() {
            Ok(Some(_)) => return,
            Ok(None) => std::thread::sleep(Duration::from_millis(50)),
            Err(_) => return,
        }
    }
    unsafe {
        libc::kill(-pid, libc::SIGKILL);
    }
    let _ = child.wait();
}
#[cfg(not(unix))]
fn terminate_group(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

/// 关掉 App 时杀掉它拉起的后端(整进程组)
pub fn kill_backend() {
    if let Ok(mut g) = backend_slot().lock() {
        if let Some(mut c) = g.take() {
            blog(&format!("关停后端:杀进程组 pid={}", c.id()));
            terminate_group(&mut c);
        }
    }
}

/// atexit 兜底:Cmd+Q(terminate:)、process::exit 等退出路径都会触发
#[cfg(unix)]
extern "C" fn atexit_kill_backend() {
    kill_backend();
}

#[cfg(unix)]
pub fn register_exit_cleanup() {
    unsafe {
        libc::atexit(atexit_kill_backend);
    }
}
#[cfg(not(unix))]
pub fn register_exit_cleanup() {}

/// SIGTERM 优雅退出:kill <pid> 也会触发 atexit 清理(杀后端),不留孤儿
#[cfg(unix)]
extern "C" fn handle_sigterm(_sig: libc::c_int) {
    std::process::exit(0);
}

#[cfg(unix)]
pub fn install_sigterm_handler() {
    let handler: unsafe extern "C" fn(libc::c_int) = handle_sigterm;
    unsafe {
        libc::signal(libc::SIGTERM, handler as *const () as libc::sighandler_t);
    }
}
#[cfg(not(unix))]
pub fn install_sigterm_handler() {}

/// 拉起 dsh 后端,放入独立进程组(退出时整组 kill,覆盖 bunx→node 整棵树)。
/// 返回子进程句柄,由调用方观察(进程提前退出 = 快速失败)。
fn spawn_dsh(
    runner: (&str, PathBuf, Vec<&'static str>),
    log_path: &PathBuf,
) -> Result<Child, String> {
    let file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path)
        .map_err(|e| format!("无法打开后端日志 {}: {e}", log_path.display()))?;
    let out = file.try_clone().map_err(|e| format!("克隆日志句柄失败: {e}"))?;
    let mut cmd = Command::new(&runner.1);
    // 补全 PATH:双击启动 .app 时 PATH 只有系统目录,子进程树(bunx→node→dsh)需要
    // 能看到常见安装目录,否则会 exit 127(command not found);Windows 用 ';' 分隔
    let mut path_parts: Vec<String> = Vec::new();
    if let Ok(p) = std::env::var("PATH") {
        path_parts.push(p);
    }
    path_parts.extend(fallback_dirs().iter().map(|d| d.display().to_string()));
    spawn_in_group(&mut cmd); // 独立进程组:退出时整组杀
    cmd.args(&runner.2)
        .env("PATH", path_parts.join(&path_sep().to_string()))
        .stdin(Stdio::null())
        .stdout(Stdio::from(out))
        .stderr(Stdio::from(file));
    let child = cmd
        .spawn()
        .map_err(|e| format!("启动 {} 失败: {e}(请手动运行: dsh web)", runner.0))?;
    blog(&format!(
        "已拉起后端: {} {} (pid={})",
        runner.0,
        runner.2.join(" "),
        child.id()
    ));
    Ok(child)
}

/// 完整自举状态机(在管理线程内顺序执行;每次 Start/Retry 跑一遍)
pub fn run_bootstrap(cfg: &BootConfig, evt: &Sender<BootEvent>) {
    blog(&format!(
        "bootstrap 开始: url={} autostart={} timeout={}s",
        cfg.url,
        cfg.autostart,
        cfg.timeout.as_secs()
    ));

    // ① 快速探测
    let _ = evt.send(BootEvent::Toast { text: "打开中…".into(), kind: "info" });
    if probe(cfg.port) {
        blog("端口已通,直接就绪");
        let _ = evt.send(BootEvent::Toast { text: "已就绪".into(), kind: "success" });
        let _ = evt.send(BootEvent::Ready);
        return;
    }
    blog("端口不通,尝试自动启动");

    // ② 关闭自动启动 → 直接报错
    if !cfg.autostart {
        let message = format!(
            "无法连接 dsh 后端 {}(已设置 DSH_NO_AUTOSTART)。请手动运行: dsh web",
            cfg.url
        );
        let _ = evt.send(BootEvent::Toast { text: "连接失败".into(), kind: "error" });
        let _ = evt.send(BootEvent::ErrorPanel { message, log_tail: String::new() });
        return;
    }

    // ③ 找启动器
    let Some(runner) = find_runner() else {
        let message =
            "本机找不到 bunx / pnpm / npm / dsh,无法自动启动 dsh 后端。\n请安装其一后重试,或手动运行: dsh web".to_string();
        let _ = evt.send(BootEvent::Toast { text: "启动失败".into(), kind: "error" });
        let _ = evt.send(BootEvent::ErrorPanel { message, log_tail: String::new() });
        return;
    };
    blog(&format!("使用启动器: {}", runner.0));
    let _ = evt.send(BootEvent::Toast {
        text: format!("启动中…({})", runner.0),
        kind: "info",
    });

    // ④ 拉起(保留句柄用于快速失败检测)
    let mut child = match spawn_dsh(runner, &cfg.backend_log) {
        Ok(c) => Some(c),
        Err(e) => {
            let tail = log_tail(&cfg.backend_log, 15);
            let _ = evt.send(BootEvent::Toast { text: "启动失败".into(), kind: "error" });
            let _ = evt.send(BootEvent::ErrorPanel { message: e, log_tail: tail });
            return;
        }
    };

    // ⑤ 轮询端口,每秒更新 toast 秒数;进程提前退出 → 快速失败
    let t0 = Instant::now();
    let mut last_toast = Instant::now();
    loop {
        if probe(cfg.port) {
            blog("后端端口就绪");
            let _ = evt.send(BootEvent::Toast { text: "已就绪".into(), kind: "success" });
            let _ = evt.send(BootEvent::Ready);
            // 持有句柄:App 退出时随进程组一起杀
            *backend_slot().lock().unwrap() = child.take();
            return;
        }
        // 快速失败:启动进程已退出(端口被占 / 依赖缺失 / 命令失败等,通常 1s 内)
        if let Some(c) = child.as_mut() {
            if let Ok(Some(status)) = c.try_wait() {
                let message = format!(
                    "dsh 后端启动进程已退出(码 {status})。多半是端口被占用或缺少依赖,\n请看下方日志;也可以手动运行: dsh web"
                );
                let tail = log_tail(&cfg.backend_log, 15);
                let _ = evt.send(BootEvent::Toast { text: "启动失败".into(), kind: "error" });
                let _ = evt.send(BootEvent::ErrorPanel { message, log_tail: tail });
                blog(&format!("启动进程提前退出: {status}"));
                return;
            }
        }
        if t0.elapsed() > cfg.timeout {
            let message = format!(
                "等待 dsh 后端就绪超时({}s)。首次运行可能需要下载依赖,请稍后重试;或手动运行: dsh web",
                cfg.timeout.as_secs()
            );
            let tail = log_tail(&cfg.backend_log, 15);
            let _ = evt.send(BootEvent::Toast { text: "启动超时".into(), kind: "error" });
            let _ = evt.send(BootEvent::ErrorPanel { message, log_tail: tail });
            blog("等待超时");
            // 超时但进程还活着(可能在下载):整组杀掉,不留孤儿
            if let Some(mut c) = child.take() {
                terminate_group(&mut c);
            }
            return;
        }
        if last_toast.elapsed() >= Duration::from_secs(1) {
            last_toast = Instant::now();
            let _ = evt.send(BootEvent::Toast {
                text: format!("启动中… 已等待 {}s", t0.elapsed().as_secs()),
                kind: "info",
            });
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}
