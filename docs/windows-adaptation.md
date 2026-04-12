# Windows 适配技术方案

> 目标：让 Claude Keyboard 在 Windows 10 21H2+ / Windows 11 上正常运行
> 最低要求：Windows 10 21H2+（自带 WebView2）

---

## 一、改动总览

| 优先级 | 模块 | 文件 | 工作量 | 说明 |
|--------|------|------|--------|------|
| P0 | IPC 通信 | `socket_server.rs` | 大 | 抽象 IPC 层，新增 Named Pipe 实现 |
| P0 | Hook 脚本 | 新增 `src/bin/hook.rs`，废弃 `.py` | 中 | Hook 改为 Rust 编译的原生二进制 |
| P1 | Hook 安装 | `hook_installer.rs` | 小 | 路径和命令格式适配 |
| P1 | 窗口定位 | `lib.rs` | 小 | 条件分支：macOS 刘海 vs Windows 顶部 |
| P2 | 依赖配置 | `Cargo.toml` | 小 | 条件编译依赖 |
| P2 | 透明窗口 | `lib.rs` + `style.css` | 小 | `window-vibrancy` + CSS 补丁 |
| P2 | CI/CD | `.github/workflows/` | 中 | 添加 Windows 构建矩阵 |

预估总工作量：~300 行 Rust + ~30 行 CSS/配置

---

## 二、各模块详细方案

### 2.1 IPC 通信层（P0）

**现状**：`socket_server.rs` 直接使用 `std::os::unix::net::{UnixListener, UnixStream}`

**方案**：条件编译，macOS 保留 Unix Socket，Windows 使用 Named Pipe

**IPC 路径**：

| 平台 | 路径 |
|------|------|
| macOS/Linux | `/tmp/claude-keyboard.sock` |
| Windows | `\\.\pipe\claude-keyboard` |

**代码架构**：

```rust
// ipc.rs — 抽象 IPC 层

pub trait IpcListener: Send + 'static {
    type Stream: IpcStream;
    fn bind() -> Result<Self> where Self: Sized;
    fn accept(&self) -> Result<Self::Stream>;
    fn cleanup();
}

pub trait IpcStream: Read + Write + Send + 'static {}

// ---- Unix 实现 ----
#[cfg(unix)]
mod unix {
    use std::os::unix::net::{UnixListener, UnixStream};
    // 保留现有逻辑，实现 IpcListener trait
}

// ---- Windows 实现 ----
#[cfg(windows)]
mod windows {
    use tokio::net::windows::named_pipe::{ServerOptions, NamedPipeServer};
    // Named Pipe 实现 IpcListener trait
}
```

**文件变更**：
- 新增 `src-tauri/src/ipc.rs` — 抽象层 + 两套实现
- 重构 `socket_server.rs` — 调用 `ipc::IpcListener` 替代直接使用 `UnixListener`

---

### 2.2 Hook 二进制化（P0）

**现状**：`claude-keyboard.py`（Python 脚本），Windows 用户可能没有 Python 环境

**方案**：用 Rust 编写 Hook，编译为独立二进制，macOS/Windows 统一使用

**文件结构**：

```
src-tauri/
├── src/
│   ├── lib.rs              # 主应用
│   ├── ipc.rs              # IPC 抽象层（新增）
│   └── bin/
│       └── hook.rs         # Hook CLI 二进制（新增）
├── resources/
│   └── claude-keyboard.py  # 保留但不再使用，留作参考
```

**Hook 二进制逻辑**：

```rust
// src/bin/hook.rs
// 编译产物：claude-keyboard-hook (macOS) / claude-keyboard-hook.exe (Windows)

fn main() {
    // 1. 从 stdin 读取 JSON（Claude Code hook 协议）
    let input: HookEvent = serde_json::from_reader(std::io::stdin()).unwrap();

    // 2. 连接 IPC（自动选择 Unix Socket 或 Named Pipe）
    let mut client = IpcClient::connect();

    // 3. 发送事件
    client.send(&input);

    // 4. 如果是 PermissionRequest，等待响应并输出
    if input.status == "waiting_for_approval" {
        let response = client.recv();
        println!("{}", serde_json::to_string(&response).unwrap());
    }
}
```

**优势**：
- 零运行时依赖
- 启动速度 ~5ms（Python ~100ms）
- IPC 连接代码与主应用复用同一个 `ipc.rs`
- macOS 上也受益（去掉 Python 依赖）

**安装流程**：`hook_installer.rs` 把编译好的二进制从 `resources/` 复制到 `~/.claude/hooks/`

---

### 2.3 Hook 安装器适配（P1）

**现状**：`hook_installer.rs` 硬编码 `python3` 命令和 Unix 路径风格

**改动**：

```rust
// 二进制文件名
#[cfg(unix)]
const HOOK_BINARY_NAME: &str = "claude-keyboard-hook";
#[cfg(windows)]
const HOOK_BINARY_NAME: &str = "claude-keyboard-hook.exe";

// settings.json 中的 command
fn hook_command() -> String {
    let hook_path = claude_dir().join("hooks").join(HOOK_BINARY_NAME);
    // macOS: /Users/xxx/.claude/hooks/claude-keyboard-hook
    // Windows: C:\Users\xxx\.claude\hooks\claude-keyboard-hook.exe
    hook_path.to_string_lossy().to_string()
}

// 文件权限：仅 Unix 需要
#[cfg(unix)]
fn make_executable(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o755));
}

#[cfg(windows)]
fn make_executable(_path: &Path) {
    // Windows 上 .exe 天然可执行，无需处理
}
```

---

### 2.4 窗口定位适配（P1）

**现状**：`y = 38.0` 硬编码 macOS 菜单栏高度

**改动**：

```rust
// lib.rs — setup 中窗口定位
let y = if cfg!(target_os = "macos") {
    38.0  // macOS 菜单栏下方，与刘海融合
} else {
    8.0   // Windows 屏幕顶部，留小间距
};
```

**前端 JS 同步**（`main.js`）：

```javascript
// resizeWindow() 中的 y 值也需要平台判断
// Tauri 提供 navigator.platform 或通过 invoke 获取
const y = navigator.platform.includes('Mac') ? 38 : 8;
```

**视觉效果**：Windows 没有刘海，顶部居中的黑色药丸依然视觉效果好，无需改 UI 形态。

---

### 2.5 依赖配置（P2）

**Cargo.toml**：

```toml
[dependencies]
tauri = { version = "2", features = ["tray-icon"] }

[target.'cfg(target_os = "macos")'.dependencies]
tauri = { version = "2", features = ["tray-icon", "macos-private-api"] }

[target.'cfg(windows)'.dependencies]
window-vibrancy = "0.5"
```

---

### 2.6 透明窗口适配（P2）

**方案**：使用 `window-vibrancy` crate 处理 Windows 上的窗口透明

```rust
// lib.rs — setup
#[cfg(windows)]
{
    use window_vibrancy::apply_mica;
    if let Some(window) = app.get_webview_window("main") {
        // Windows 11 Mica 效果，与系统风格融合
        let _ = apply_mica(&window, None);
    }
}
```

**CSS 补丁**（`style.css`）：

```css
/* Windows 上窗口无原生圆角，用 clip-path 模拟 */
@media screen and (-ms-high-contrast: none), (-ms-high-contrast: active) {
  #island {
    clip-path: inset(0 round 28px);
  }
}
```

---

## 三、风险点及应对

### 3.1 Named Pipe 权限

| 项目 | 说明 |
|------|------|
| **风险** | Claude Code hook 子进程能否访问 Named Pipe |
| **结论** | 无风险。Named Pipe 默认 ACL 允许同用户进程访问，Claude Code 和 hook 在同一用户会话下运行 |
| **行动** | 无需特殊处理，默认行为即可 |

### 3.2 WebView2 运行时

| 项目 | 说明 |
|------|------|
| **风险** | 目标系统是否自带 WebView2 |
| **结论** | 无风险。Windows 10 21H2+ 和 Windows 11 全部自带 WebView2 |
| **行动** | 不兼容老版本，README 注明最低系统要求即可 |

### 3.3 透明窗口差异

| 项目 | 说明 |
|------|------|
| **风险** | Windows 上 `transparent: true` 可能出现白边或圆角渲染问题 |
| **方案** | 使用 `window-vibrancy` crate 的 Mica 效果 + CSS `clip-path` 兜底 |
| **行动** | 添加 `window-vibrancy` 依赖，`#[cfg(windows)]` 条件应用 |

### 3.4 Hook 二进制分发

| 项目 | 说明 |
|------|------|
| **风险** | Hook 二进制需要随应用一起打包分发 |
| **方案** | Tauri `bundle.resources` 配置中加入编译好的 hook 二进制 |
| **行动** | `tauri.conf.json` 的 `resources` 数组添加 hook 二进制路径，`hook_installer.rs` 从 resources 目录复制到 `~/.claude/hooks/` |

---

## 四、不需要改动的部分

| 模块 | 原因 |
|------|------|
| `tauri.conf.json` 中 `macOSPrivateApi` | Windows 上自动忽略 |
| 前端 UI（HTML/CSS/JS） | Web 技术天然跨平台，两态设计、动画、音效无需改动 |
| 打包格式 | `"targets": "all"` 自动选择平台打包器（macOS → `.dmg`，Windows → `.msi`） |
| 图标 | `icon.ico` 已存在 |

---

## 五、实施顺序

```
Phase 1 — 核心（可编译 & 可运行）
  ├── 新增 ipc.rs 抽象层 + Named Pipe 实现
  ├── 重构 socket_server.rs 使用 ipc 抽象
  ├── 新增 src/bin/hook.rs
  └── 适配 hook_installer.rs

Phase 2 — 体验（看起来对）
  ├── 窗口定位条件分支
  ├── window-vibrancy 透明窗口
  └── CSS 圆角补丁

Phase 3 — 发布（能分发）
  ├── Cargo.toml 条件依赖
  ├── GitHub Actions Windows 构建
  └── README 更新系统要求
```
