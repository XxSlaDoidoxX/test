use std::{
  os::windows::io::AsRawHandle,
  path::{Path, PathBuf},
  thread::JoinHandle,
};

use anyhow::{bail, Context};
use windows::{
  core::{w, PCWSTR},
  Win32::{
    Foundation::{HANDLE, HWND, LPARAM, POINT, WPARAM},
    System::{
      Environment::ExpandEnvironmentStringsW, Threading::GetThreadId,
    },
    UI::{
      Shell::{
        ShellExecuteExW, SEE_MASK_NOASYNC, SEE_MASK_NOCLOSEPROCESS,
        SHELLEXECUTEINFOW,
      },
      Input::KeyboardAndMouse::{GetAsyncKeyState, VK_MENU},
      WindowsAndMessaging::{
        CreateWindowExW, DispatchMessageW, GetAncestor, GetCursorPos,
        GetDesktopWindow, GetForegroundWindow, GetMessageW,
        GetShellWindow, MessageBoxW, PeekMessageW, PostThreadMessageW,
        RegisterClassW, SetCursorPos, SystemParametersInfoW,
        TranslateMessage, WindowFromPoint, ANIMATIONINFO, CS_HREDRAW,
        CS_VREDRAW, CW_USEDEFAULT, GA_ROOT, MB_ICONERROR, MB_OK,
        MB_SYSTEMMODAL, MSG, PM_REMOVE, SPIF_SENDCHANGE,
        SPIF_UPDATEINIFILE, SPI_GETANIMATION, SPI_SETANIMATION, SW_HIDE,
        SW_NORMAL, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, WINDOW_EX_STYLE,
        WM_QUIT, WNDCLASSW, WNDPROC, WS_OVERLAPPEDWINDOW,
      },
    },
  },
};
use wm_common::{ParsedConfig, Point};

use super::{
  native_monitor, native_window, EventListener, NativeMonitor,
  NativeWindow, SingleInstance,
};

pub type WindowProcedure = WNDPROC;

pub struct Platform;

impl Platform {
  /// Gets the `NativeWindow` instance of the currently focused window.
  #[must_use]
  pub fn foreground_window() -> NativeWindow {
    let handle = unsafe { GetForegroundWindow() };
    NativeWindow::new(handle.0)
  }

  /// Gets the `NativeWindow` instance of the desktop window.
  #[must_use]
  pub fn desktop_window() -> NativeWindow {
    let handle = match unsafe { GetShellWindow() } {
      HWND(0) => unsafe { GetDesktopWindow() },
      handle => handle,
    };

    NativeWindow::new(handle.0)
  }

  /// Checks if the Alt key is currently held down.
  #[must_use]
  pub fn is_alt_down() -> bool {
    // GetAsyncKeyState returns a SHORT (i16). If the most significant bit is set,
    // the key is down. In 2's complement, this means the value is negative.
    unsafe { GetAsyncKeyState(i32::from(VK_MENU.0)) < 0 }
  }

  /// Gets a vector of available monitors as `NativeMonitor` instances
  /// sorted from left-to-right and top-to-bottom.
  pub fn sorted_monitors() -> anyhow::Result<Vec<NativeMonitor>> {
    let monitors = native_monitor::available_monitors()?;

    let mut monitors_with_rect = monitors
      .into_iter()
      .map(|monitor| {
        let rect = monitor.rect()?.clone();
        anyhow::Ok((monitor, rect))
      })
      .try_collect::<Vec<_>>()?;

    monitors_with_rect.sort_by(|(_, rect_a), (_, rect_b)| {
      if rect_a.x() == rect_b.x() {
        rect_a.y().cmp(&rect_b.y())
      } else {
        rect_a.x().cmp(&rect_b.x())
      }
    });

    Ok(
      monitors_with_rect
        .into_iter()
        .map(|(monitor, _)| monitor)
        .collect(),
    )
  }

  #[must_use]
  pub fn nearest_monitor(window: &NativeWindow) -> NativeMonitor {
    native_monitor::nearest_monitor(window.handle)
  }

  pub fn manageable_windows() -> anyhow::Result<Vec<NativeWindow>> {
    Ok(
      native_window::available_windows()?
        .into_iter()
        .filter(|window| window.is_manageable().unwrap_or(false))
        .collect(),
    )
  }

  pub fn start_event_listener(
    config: &ParsedConfig,
  ) -> anyhow::Result<EventListener> {
    EventListener::start(config)
  }

  pub fn new_single_instance() -> anyhow::Result<SingleInstance> {
    SingleInstance::new()
  }

  pub fn root_ancestor(
    window: &NativeWindow,
  ) -> anyhow::Result<NativeWindow> {
    let handle = unsafe { GetAncestor(HWND(window.handle), GA_ROOT) };
    Ok(NativeWindow::new(handle.0))
  }

  pub fn set_cursor_pos(x: i32, y: i32) -> anyhow::Result<()> {
    unsafe {
      SetCursorPos(x, y)?;
    };

    Ok(())
  }

  pub fn window_from_point(point: &Point) -> anyhow::Result<NativeWindow> {
    let point = POINT {
      x: point.x,
      y: point.y,
    };

    let handle = unsafe { WindowFromPoint(point) };
    Ok(NativeWindow::new(handle.0))
  }

  pub fn mouse_position() -> anyhow::Result<Point> {
    let mut point = POINT { x: 0, y: 0 };
    unsafe { GetCursorPos(&raw mut point) }?;

    Ok(Point {
      x: point.x,
      y: point.y,
    })
  }

  pub fn create_message_window(
    window_procedure: WindowProcedure,
  ) -> anyhow::Result<isize> {
    let wnd_class = WNDCLASSW {
      lpszClassName: w!("MessageWindow"),
      style: CS_HREDRAW | CS_VREDRAW,
      lpfnWndProc: window_procedure,
      ..Default::default()
    };

    unsafe { RegisterClassW(&raw const wnd_class) };

    let handle = unsafe {
      CreateWindowExW(
        WINDOW_EX_STYLE::default(),
        w!("MessageWindow"),
        w!("MessageWindow"),
        WS_OVERLAPPEDWINDOW,
        CW_USEDEFAULT,
        CW_USEDEFAULT,
        CW_USEDEFAULT,
        CW_USEDEFAULT,
        None,
        None,
        wnd_class.hInstance,
        None,
      )
    };

    if handle.0 == 0 {
      bail!("Creation of message window failed.");
    }

    Ok(handle.0)
  }

  pub fn run_message_loop() {
    let mut msg = MSG::default();

    loop {
      if unsafe { GetMessageW(&raw mut msg, None, 0, 0) }.as_bool() {
        unsafe {
          TranslateMessage(&raw const msg);
          DispatchMessageW(&raw const msg);
        }
      } else {
        break;
      }
    }
  }

  pub fn run_message_cycle() -> anyhow::Result<()> {
    let mut msg = MSG::default();

    let has_message =
      unsafe { PeekMessageW(&raw mut msg, None, 0, 0, PM_REMOVE) }
        .as_bool();

    if has_message {
      if msg.message == WM_QUIT {
        bail!("Received WM_QUIT message.")
      }

      unsafe {
        TranslateMessage(&raw const msg);
        DispatchMessageW(&raw const msg);
      }
    }

    Ok(())
  }

  pub fn kill_message_loop<T>(
    thread: &JoinHandle<T>,
  ) -> anyhow::Result<()> {
    let handle = thread.as_raw_handle();
    let handle = HANDLE(handle as isize);
    let thread_id = unsafe { GetThreadId(handle) };

    unsafe {
      PostThreadMessageW(
        thread_id,
        WM_QUIT,
        WPARAM::default(),
        LPARAM::default(),
      )
    }?;

    Ok(())
  }

  pub fn window_animations_enabled() -> anyhow::Result<bool> {
    let mut animation_info = ANIMATIONINFO {
      #[allow(clippy::cast_possible_truncation)]
      cbSize: std::mem::size_of::<ANIMATIONINFO>() as u32,
      iMinAnimate: 0,
    };

    unsafe {
      SystemParametersInfoW(
        SPI_GETANIMATION,
        animation_info.cbSize,
        Some(std::ptr::from_mut(&mut animation_info).cast()),
        SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
      )
    }?;

    Ok(animation_info.iMinAnimate != 0)
  }

  pub fn set_window_animations_enabled(
    enable: bool,
  ) -> anyhow::Result<()> {
    let mut animation_info = ANIMATIONINFO {
      #[allow(clippy::cast_possible_truncation)]
      cbSize: std::mem::size_of::<ANIMATIONINFO>() as u32,
      iMinAnimate: i32::from(enable),
    };

    unsafe {
      SystemParametersInfoW(
        SPI_SETANIMATION,
        animation_info.cbSize,
        Some(std::ptr::from_mut(&mut animation_info).cast()),
        SPIF_UPDATEINIFILE | SPIF_SENDCHANGE,
      )
    }?;

    Ok(())
  }

  pub fn open_file_explorer(path: &PathBuf) -> anyhow::Result<()> {
    let normalized_path = std::fs::canonicalize(path)?;

    std::process::Command::new("explorer")
      .arg(normalized_path)
      .spawn()?;

    Ok(())
  }

  pub fn parse_command(command: &str) -> anyhow::Result<(String, String)> {
    let expanded_command = {
      let wide_command = to_wide(command);
      let size = unsafe {
        ExpandEnvironmentStringsW(PCWSTR(wide_command.as_ptr()), None)
      };

      if size == 0 {
        anyhow::bail!(
          "Failed to expand environment strings in command '{}'.",
          command
        );
      }

      let mut buffer = vec![0; size as usize];
      let size = unsafe {
        ExpandEnvironmentStringsW(
          PCWSTR(wide_command.as_ptr()),
          Some(&mut buffer),
        )
      };

      String::from_utf16_lossy(&buffer[..(size - 1) as usize])
    };

    let command_parts: Vec<&str> =
      expanded_command.split_whitespace().collect();

    if command.starts_with('"') {
      let (closing_index, _) =
        command.match_indices('"').nth(2).with_context(|| {
          format!("Command doesn't have an ending `\"`: '{command}'.")
        })?;

      return Ok((
        command[1..closing_index].to_string(),
        command[closing_index + 1..].trim().to_string(),
      ));
    }

    if let Some(first_part) = command_parts.first() {
      if !first_part.contains(&['/', '\\'][..]) {
        let args = command_parts[1..].join(" ");
        return Ok(((*first_part).to_string(), args));
      }
    }

    let mut cumulative_path = Vec::new();

    for (part_index, &part) in command_parts.iter().enumerate() {
      cumulative_path.push(part);

      if Path::new(&cumulative_path.join(" ")).is_file() {
        return Ok((
          cumulative_path.join(" "),
          command_parts[part_index + 1..].join(" "),
        ));
      }
    }

    anyhow::bail!("Program path is not valid for command '{}'.", command)
  }

  pub fn run_command(
    program: &str,
    args: &str,
    hide_window: bool,
  ) -> anyhow::Result<()> {
    let home_dir = home::home_dir()
      .context("Unable to get home directory.")?
      .to_str()
      .context("Invalid home directory.")?
      .to_owned();

    let program_wide = to_wide(program);
    let args_wide = to_wide(args);
    let home_dir_wide = to_wide(&home_dir);

    let mut exec_info = SHELLEXECUTEINFOW {
      #[allow(clippy::cast_possible_truncation)]
      cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
      lpFile: PCWSTR(program_wide.as_ptr()),
      lpParameters: PCWSTR(args_wide.as_ptr()),
      lpDirectory: PCWSTR(home_dir_wide.as_ptr()),
      nShow: if hide_window { SW_HIDE } else { SW_NORMAL }.0 as _,
      fMask: SEE_MASK_NOCLOSEPROCESS | SEE_MASK_NOASYNC,
      ..Default::default()
    };

    unsafe { ShellExecuteExW(&raw mut exec_info) }?;
    Ok(())
  }

  pub fn show_error_dialog(title: &str, message: &str) {
    let title_wide = to_wide(title);
    let message_wide = to_wide(message);

    unsafe {
      MessageBoxW(
        None,
        PCWSTR(message_wide.as_ptr()),
        PCWSTR(title_wide.as_ptr()),
        MB_ICONERROR | MB_OK | MB_SYSTEMMODAL,
      );
    }
  }
}

fn to_wide(string: &str) -> Vec<u16> {
  string.encode_utf16().chain(Some(0)).collect()
}