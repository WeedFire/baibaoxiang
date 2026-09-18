use crate::models::{AppItem, LaunchResult, WindowStyle};
use crate::services::python_detector::{apply_console_preference, resolve_interpreter_with_base};
use crate::utils::args::{quote_arg, split_args};
use std::path::Path;
use std::process::Command;

/// 启动前解析出的最终命令行。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchPlan {
    pub program: String,
    pub args: Vec<String>,
    pub working_dir: Option<String>,
}

/// 根据应用配置解析出真正要执行的程序、参数与工作目录。
/// 相对路径以「程序运行目录」（安装目录）为根解析。
pub fn build_launch_plan(app: &AppItem) -> Result<LaunchPlan, String> {
    build_launch_plan_with_base(app, crate::utils::app_base_dir())
}

/// `build_launch_plan` 的实现，相对路径的根由参数注入以便测试。
pub fn build_launch_plan_with_base(
    app: &AppItem,
    base: Option<&Path>,
) -> Result<LaunchPlan, String> {
    let extra_args = split_args(app.arguments.as_deref().unwrap_or(""));

    let working_dir = app
        .working_directory
        .as_deref()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(|s| crate::utils::resolve_path_with(base, s));

    if let Some(ref dir) = working_dir {
        if !dir.is_dir() {
            return Err(format!("工作目录不存在: {}", dir.display()));
        }
    }

    if app.is_python_script {
        let raw_script = app.executable_path.trim();
        if raw_script.is_empty() {
            return Err("未指定 Python 脚本路径".to_string());
        }
        let script = crate::utils::resolve_path_with(base, raw_script);
        if !script.exists() {
            return Err(format!("Python 脚本不存在: {}", script.display()));
        }

        let interpreter =
            apply_console_preference(&resolve_interpreter_with_base(app, base)?, app.show_console);

        // 默认工作目录取脚本所在目录，保证相对路径导入/读写可用
        let working_dir = match working_dir {
            Some(d) => Some(d.to_string_lossy().to_string()),
            None => script
                .parent()
                .filter(|p| !p.as_os_str().is_empty())
                .map(|p| p.to_string_lossy().to_string()),
        };

        let mut args = vec![script.to_string_lossy().to_string()];
        args.extend(extra_args);

        if app.show_console {
            // 用 cmd /s /k 包裹，脚本结束后保留窗口以便查看输出。
            // 整条命令行作为一个原始参数传递（见 `create_process` 中的 raw_arg），
            // 否则 Rust 的转义会在引号外再套一层，cmd 会把带引号的路径当成命令名本身。
            let mut parts = vec![interpreter.clone()];
            parts.extend(args.iter().cloned());
            return Ok(LaunchPlan {
                program: cmd_exe(),
                args: vec![
                    "/s".to_string(),
                    "/k".to_string(),
                    cmd_command_line(&parts),
                ],
                working_dir,
            });
        }

        return Ok(LaunchPlan {
            program: interpreter,
            args,
            working_dir,
        });
    }

    let program = app.executable_path.trim();
    if program.is_empty() {
        return Err("未指定应用路径".to_string());
    }
    let program = crate::utils::resolve_path_with(base, program);
    if !program.exists() {
        return Err(format!("文件不存在: {}", program.display()));
    }

    Ok(LaunchPlan {
        program: program.to_string_lossy().to_string(),
        args: extra_args,
        working_dir: working_dir.map(|d| d.to_string_lossy().to_string()),
    })
}

/// 构造交给 `cmd /s /k`（或 `/s /c`）的整条命令行。
///
/// 每个参数按 Windows CRT 规则单独加引号，最外层再包一对引号供 `/s` 去掉，
/// 这样 cmd 拿到的是 `"解释器" "脚本" ...` 的字面内容，不会再被转义一次。
fn cmd_command_line(parts: &[String]) -> String {
    let inner: Vec<String> = parts.iter().map(|p| quote_arg(p)).collect();
    format!("\"{}\"", inner.join(" "))
}

fn cmd_exe() -> String {
    std::env::var("COMSPEC").unwrap_or_else(|_| "C:\\Windows\\System32\\cmd.exe".to_string())
}

pub fn launch_app(app: &AppItem) -> Result<LaunchResult, String> {
    let plan = build_launch_plan(app)?;

    if !app.allow_multiple_instances {
        if let Some(name) = Path::new(&plan.program)
            .file_name()
            .and_then(|n| n.to_str())
        {
            #[cfg(target_os = "windows")]
            {
                if is_process_running(name) {
                    return Ok(LaunchResult {
                        ok: false,
                        message: format!("{} 已在运行中", app.name),
                    });
                }
            }
            #[cfg(not(target_os = "windows"))]
            {
                let _ = name;
            }
        }
    }

    #[cfg(target_os = "windows")]
    {
        // 提权或需要控制窗口状态时走 ShellExecuteW，其余走 CreateProcess
        if app.run_as_admin || app.startup_window_style != WindowStyle::Normal {
            let verb = if app.run_as_admin { "runas" } else { "open" };
            return shell_execute(
                verb,
                &plan.program,
                &plan.args,
                plan.working_dir.as_deref(),
                show_cmd(app.startup_window_style),
            );
        }

        // Python 脚本在不显示控制台时不弹窗
        let hide_console = !app.show_console && app.is_python_script;
        return create_process(&plan, hide_console);
    }

    #[cfg(not(target_os = "windows"))]
    {
        create_process(&plan, false)
    }
}

#[cfg(target_os = "windows")]
fn show_cmd(style: WindowStyle) -> i32 {
    match style {
        WindowStyle::Maximized => 3,    // SW_MAXIMIZE
        WindowStyle::Minimized => 2,    // SW_SHOWMINIMIZED
        WindowStyle::Normal => 1,       // SW_SHOWNORMAL
    }
}

#[cfg(target_os = "windows")]
fn create_process(plan: &LaunchPlan, hide_console: bool) -> Result<LaunchResult, String> {
    use std::os::windows::process::CommandExt;

    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    const CREATE_NEW_CONSOLE: u32 = 0x0000_0010;

    let program = plan.program.trim_matches('"');
    let is_cmd = program.to_lowercase().ends_with("cmd.exe");

    let mut cmd = Command::new(program);
    if is_cmd {
        // 前几个是 /s /k 之类的开关，最后一个是整条命令行，必须原样传入
        if let Some((raw, rest)) = plan.args.split_last() {
            cmd.args(rest);
            cmd.raw_arg(raw);
        }
    } else {
        cmd.args(&plan.args);
    }

    if let Some(ref dir) = plan.working_dir {
        cmd.current_dir(dir);
    }

    if hide_console {
        cmd.creation_flags(CREATE_NO_WINDOW);
    } else if is_cmd {
        cmd.creation_flags(CREATE_NEW_CONSOLE);
    }

    match cmd.spawn() {
        Ok(_) => Ok(LaunchResult {
            ok: true,
            message: format!("已启动 {}", program),
        }),
        Err(e) => Err(format!("启动失败: {}", e)),
    }
}

#[cfg(not(target_os = "windows"))]
fn create_process(plan: &LaunchPlan, _hide_console: bool) -> Result<LaunchResult, String> {
    let program = plan.program.trim_matches('"');
    let mut cmd = Command::new(program);
    cmd.args(&plan.args);
    if let Some(ref dir) = plan.working_dir {
        cmd.current_dir(dir);
    }
    match cmd.spawn() {
        Ok(_) => Ok(LaunchResult {
            ok: true,
            message: format!("已启动 {}", program),
        }),
        Err(e) => Err(format!("启动失败: {}", e)),
    }
}

/// 调用 ShellExecuteW，支持 runas 提权与窗口状态控制。
#[cfg(target_os = "windows")]
fn shell_execute(
    verb: &str,
    program: &str,
    args: &[String],
    working_dir: Option<&str>,
    show_cmd_value: i32,
) -> Result<LaunchResult, String> {
    use windows::core::PCWSTR;
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SHOW_WINDOW_CMD;

    fn wide(s: &str) -> Vec<u16> {
        use std::os::windows::ffi::OsStrExt;
        std::ffi::OsStr::new(s)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect()
    }

    let program = program.trim_matches('"');
    // cmd.exe 的最后一个参数已经是一整条命令行，不能再逐个加引号
    let params: String = if program.to_lowercase().ends_with("cmd.exe") {
        match args.split_last() {
            Some((raw, rest)) if !rest.is_empty() => format!("{} {}", rest.join(" "), raw),
            Some((raw, _)) => raw.clone(),
            None => String::new(),
        }
    } else {
        args.iter()
            .map(|a| quote_arg(a))
            .collect::<Vec<_>>()
            .join(" ")
    };

    let w_verb = wide(verb);
    let w_file = wide(program);
    let w_params = wide(&params);
    let w_dir = working_dir.map(wide).unwrap_or_default();

    let dir_ptr = if working_dir.is_some() {
        PCWSTR(w_dir.as_ptr())
    } else {
        PCWSTR::null()
    };

    unsafe {
        let inst = ShellExecuteW(
            None,
            PCWSTR(w_verb.as_ptr()),
            PCWSTR(w_file.as_ptr()),
            PCWSTR(w_params.as_ptr()),
            dir_ptr,
            SHOW_WINDOW_CMD(show_cmd_value),
        );
        let code = inst.0 as isize;
        if code > 32 {
            Ok(LaunchResult {
                ok: true,
                message: format!("已启动 {}", program),
            })
        } else {
            Err(match code {
                0 => "系统资源不足，无法启动".to_string(),
                2 => format!("找不到文件: {}", program),
                3 => format!("找不到路径: {}", program),
                5 => "拒绝访问".to_string(),
                8 => "内存不足".to_string(),
                31 => "没有关联的应用程序".to_string(),
                _ => format!("启动失败 (ShellExecute 错误码 {})", code),
            })
        }
    }
}

/// 用系统默认浏览器打开网址（仅 http/https，调用方需先校验）。
#[cfg(target_os = "windows")]
pub fn open_url(url: &str) -> Result<(), String> {
    // 复用 ShellExecuteW：program 直接传网址即可交给默认浏览器
    const SW_SHOWNORMAL: i32 = 1;
    shell_execute("open", url, &[], None, SW_SHOWNORMAL).map(|_| ())
}

#[cfg(not(target_os = "windows"))]
pub fn open_url(_url: &str) -> Result<(), String> {
    Err("当前平台不支持".to_string())
}

/// 判断指定可执行文件是否已在运行（用于“允许多实例”开关）。
#[cfg(target_os = "windows")]
pub fn is_process_running(exe_name: &str) -> bool {
    use windows::Win32::Foundation::{CloseHandle, HANDLE};
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };

    unsafe {
        let snapshot = match CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) {
            Ok(h) => h,
            Err(_) => return false,
        };
        if snapshot.is_invalid() {
            return false;
        }

        let mut entry = PROCESSENTRY32W::default();
        entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;

        let mut found = false;
        if Process32FirstW(snapshot, &mut entry).is_ok() {
            loop {
                let len = entry
                    .szExeFile
                    .iter()
                    .position(|&c| c == 0)
                    .unwrap_or(entry.szExeFile.len());
                let name = String::from_utf16_lossy(&entry.szExeFile[..len]);
                if name.eq_ignore_ascii_case(exe_name) {
                    found = true;
                    break;
                }
                if Process32NextW(snapshot, &mut entry).is_err() {
                    break;
                }
            }
        }
        let _ = CloseHandle(HANDLE(snapshot.0 as *mut _));
        found
    }
}

/// 在资源管理器中选中并高亮指定路径（相对路径按程序运行目录解析）。
pub fn reveal_in_explorer(path: &str) -> Result<(), String> {
    let path = crate::utils::resolve_path(path).to_string_lossy().to_string();
    #[cfg(target_os = "windows")]
    {
        let exists = Path::new(&path).exists();
        let arg = if exists {
            format!("/select,{}", quote_arg(&path))
        } else {
            Path::new(&path)
                .parent()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|| path.clone())
        };

        let mut cmd = Command::new("explorer.exe");
        cmd.arg(arg);
        // 避免继承本进程的控制台
        #[cfg(target_os = "windows")]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }
        cmd.spawn()
            .map_err(|e| format!("打开资源管理器失败: {}", e))?;
        return Ok(());
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = path;
        Err("当前平台不支持".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_app() -> AppItem {
        AppItem {
            id: "1".into(),
            group_id: "default".into(),
            name: "Test".into(),
            executable_path: String::new(),
            arguments: None,
            working_directory: None,
            startup_window_style: WindowStyle::Normal,
            is_python_script: false,
            python_interpreter_path: None,
            show_console: false,
            run_as_admin: false,
            allow_multiple_instances: true,
            icon_path: None,
            sort_order: 0,
            created_at: String::new(),
            updated_at: String::new(),
        }
    }

    #[test]
    fn rejects_missing_executable() {
        let mut app = make_app();
        app.executable_path = "C:/__nope__.exe".into();
        let err = build_launch_plan(&app).unwrap_err();
        assert!(err.contains("不存在"), "{}", err);
    }

    #[test]
    fn splits_arguments_and_keeps_paths() {
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("my app.exe");
        std::fs::write(&exe, b"").unwrap();

        let mut app = make_app();
        app.executable_path = exe.to_string_lossy().to_string();
        app.arguments = Some(r#""D:\my data" --x=1"#.into());

        let plan = build_launch_plan(&app).unwrap();
        assert_eq!(plan.args, vec![r"D:\my data".to_string(), "--x=1".to_string()]);
    }

    #[test]
    fn rejects_missing_working_directory() {
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("a.exe");
        std::fs::write(&exe, b"").unwrap();

        let mut app = make_app();
        app.executable_path = exe.to_string_lossy().to_string();
        app.working_directory = Some("C:/__nope_dir__".into());
        let err = build_launch_plan(&app).unwrap_err();
        assert!(err.contains("工作目录"), "{}", err);
    }

    #[test]
    fn python_plan_uses_interpreter_and_script_dir() {
        let dir = tempfile::tempdir().unwrap();
        let script = dir.path().join("main.py");
        std::fs::write(&script, b"print(1)").unwrap();
        let interp = dir.path().join("python.exe");
        std::fs::write(&interp, b"").unwrap();

        let mut app = make_app();
        app.is_python_script = true;
        app.executable_path = script.to_string_lossy().to_string();
        app.python_interpreter_path = Some(interp.to_string_lossy().to_string());

        let plan = build_launch_plan(&app).unwrap();
        assert_eq!(plan.program, interp.to_string_lossy());
        assert_eq!(plan.args, vec![script.to_string_lossy().to_string()]);
        assert_eq!(
            plan.working_dir.as_deref(),
            Some(dir.path().to_string_lossy().as_ref())
        );
    }

    /// 脚本与解释器都写相对路径时，以「程序运行目录」为根展开。
    #[test]
    fn relative_python_paths_are_resolved_against_base() {
        let dir = tempfile::tempdir().unwrap();
        let script = dir.path().join("pyTools").join("main.py");
        std::fs::create_dir_all(script.parent().unwrap()).unwrap();
        std::fs::write(&script, b"print(1)").unwrap();
        let interp = dir.path().join("python").join("python.exe");
        std::fs::create_dir_all(interp.parent().unwrap()).unwrap();
        std::fs::write(&interp, b"").unwrap();

        let mut app = make_app();
        app.is_python_script = true;
        app.executable_path = "pyTools/main.py".into();
        app.python_interpreter_path = Some("python/python.exe".into());

        let plan = build_launch_plan_with_base(&app, Some(dir.path())).unwrap();
        assert_eq!(Path::new(&plan.program), interp);
        assert_eq!(plan.args.len(), 1);
        assert_eq!(Path::new(&plan.args[0]), script);
        assert_eq!(
            Path::new(plan.working_dir.as_deref().unwrap()),
            script.parent().unwrap()
        );
    }

    /// 普通程序也支持相对路径，工作目录同样按程序目录展开。
    #[test]
    fn relative_executable_and_workdir_are_resolved_against_base() {
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("bin").join("tool.exe");
        std::fs::create_dir_all(exe.parent().unwrap()).unwrap();
        std::fs::write(&exe, b"").unwrap();
        let workdir = dir.path().join("data");
        std::fs::create_dir_all(&workdir).unwrap();

        let mut app = make_app();
        app.executable_path = "bin/tool.exe".into();
        app.working_directory = Some("data".into());

        let plan = build_launch_plan_with_base(&app, Some(dir.path())).unwrap();
        assert_eq!(Path::new(&plan.program), exe);
        assert_eq!(Path::new(plan.working_dir.as_deref().unwrap()), workdir);
    }

    /// 相对路径在程序目录下不存在时应给出解析后的路径，而不是“文件不存在”之外的信息丢失。
    #[test]
    fn missing_relative_script_reports_resolved_path() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = make_app();
        app.is_python_script = true;
        app.executable_path = "pyTools/__nope__.py".into();

        let err = build_launch_plan_with_base(&app, Some(dir.path())).unwrap_err();
        assert!(err.contains("__nope__.py"), "{}", err);
        assert!(err.contains("pyTools"), "{}", err);
    }

    #[test]
    fn python_plan_wraps_with_cmd_when_console_requested() {
        let dir = tempfile::tempdir().unwrap();
        let script = dir.path().join("main.py");
        std::fs::write(&script, b"print(1)").unwrap();
        let interp = dir.path().join("python.exe");
        std::fs::write(&interp, b"").unwrap();

        let mut app = make_app();
        app.is_python_script = true;
        app.show_console = true;
        app.executable_path = script.to_string_lossy().to_string();
        app.python_interpreter_path = Some(interp.to_string_lossy().to_string());

        let plan = build_launch_plan(&app).unwrap();
        assert!(plan.program.to_lowercase().ends_with("cmd.exe"));
        assert_eq!(plan.args[0], "/s");
        assert_eq!(plan.args[1], "/k");
        let line = &plan.args[2];
        assert!(line.starts_with('"') && line.ends_with('"'));
        assert!(line.contains("python"));
        assert!(line.ends_with("main.py\""));
    }

    #[test]
    fn python_plan_rejects_missing_script() {
        let dir = tempfile::tempdir().unwrap();
        let interp = dir.path().join("python.exe");
        std::fs::write(&interp, b"").unwrap();

        let mut app = make_app();
        app.is_python_script = true;
        app.executable_path = dir.path().join("__nope__.py").to_string_lossy().to_string();
        app.python_interpreter_path = Some(interp.to_string_lossy().to_string());

        assert!(build_launch_plan(&app).is_err());
    }

    /// 端到端：用真实的 python.exe 跑一个脚本，并确认脚本确实执行成功。
    #[test]
    fn launches_a_real_python_script() {
        let Some(py) = crate::services::python_detector::detect_python_installations()
            .into_iter()
            .next()
        else {
            eprintln!("skip: 本机未安装 Python");
            return;
        };

        let dir = tempfile::tempdir().unwrap();
        let script = dir.path().join("hello.py");
        let output = dir.path().join("out.txt");
        std::fs::write(
            &script,
            "import sys\nwith open(sys.argv[1], 'w', encoding='utf-8') as f:\n    f.write('ok')\n",
        )
        .unwrap();

        let mut app = make_app();
        app.name = "Python 测试脚本".into();
        app.is_python_script = true;
        app.executable_path = script.to_string_lossy().to_string();
        app.python_interpreter_path = Some(py.path.clone());
        app.arguments = Some(output.to_string_lossy().to_string());

        let result = launch_app(&app).expect("启动 Python 脚本不应返回 Err");
        assert!(result.ok, "启动失败: {}", result.message);

        let mut ok = false;
        for _ in 0..100 {
            if output.exists() {
                if let Ok(content) = std::fs::read_to_string(&output) {
                    if content == "ok" {
                        ok = true;
                    }
                }
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        assert!(ok, "Python 脚本未产生预期输出（解释器: {}）", py.path);
    }

    /// 端到端：单实例开关生效时，已运行的程序不会被重复启动。
    #[test]
    fn single_instance_is_detected() {
        #[cfg(target_os = "windows")]
        {
            let exe = std::env::current_exe().expect("当前进程路径");
            let name = exe.file_name().unwrap().to_string_lossy().to_string();
            assert!(
                is_process_running(&name),
                "当前测试进程 {} 应被判定为正在运行",
                name
            );
            assert!(!is_process_running("__definitely_not_running__.exe"));
        }
    }

    #[test]
    fn cmd_command_line_quotes_each_part_once() {
        let line = cmd_command_line(&[
            r"C:\Program Files\Python\Python313\python.exe".to_string(),
            r"F:\my scripts\main.py".to_string(),
            "--flag".to_string(),
        ]);
        assert_eq!(
            line,
            "\"\"C:\\Program Files\\Python\\Python313\\python.exe\" \"F:\\my scripts\\main.py\" --flag\""
        );
    }
}
