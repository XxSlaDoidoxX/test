use std::time::Duration;
use anyhow::Context;
use tokio::task;
use tracing::{info, warn};
use wm_common::{
  CornerStyle, CursorJumpTrigger, DisplayState, HideMethod, OpacityValue,
  UniqueExt, WindowEffectConfig, WindowState, WmEvent,
};
use wm_platform::{Platform, ZOrder};

use crate::{
  models::{Container, WindowContainer},
  traits::{CommonGetters, PositionGetters, WindowGetters},
  user_config::UserConfig,
  wm_state::WmState,
};

pub fn platform_sync(
  state: &mut WmState,
  config: &UserConfig,
) -> anyhow::Result<()> {
  let focused_container =
    state.focused_container().context("No focused container.")?;

  if state.pending_sync.needs_focus_update() {
    sync_focus(&focused_container, state)?;
  }

  if !state.pending_sync.containers_to_redraw().is_empty()
    || !state.pending_sync.workspaces_to_reorder().is_empty()
  {
    redraw_containers(&focused_container, state, config)?;
  }

  if state.pending_sync.needs_cursor_jump()
    && config.value.general.cursor_jump.enabled
  {
    jump_cursor(focused_container.clone(), state, config)?;
  }

  if state.pending_sync.needs_focused_effect_update()
    || state.pending_sync.needs_all_effects_update()
  {
    let prev_effects_window = state.prev_effects_window.clone();

    if let Ok(window) = focused_container.as_window_container() {
      apply_window_effects(&window, true, config);
      state.prev_effects_window = Some(window.clone());
    } else {
      state.prev_effects_window = None;
    }

    let unfocused_windows =
      if state.pending_sync.needs_all_effects_update() {
        state.windows()
      } else {
        prev_effects_window.into_iter().collect()
      }
      .into_iter()
      .filter(|window| window.id() != focused_container.id());

    for window in unfocused_windows {
      apply_window_effects(&window, false, config);
    }
  }

  state.pending_sync.clear();

  Ok(())
}

fn sync_focus(
  focused_container: &Container,
  state: &mut WmState,
) -> anyhow::Result<()> {
  let native_window = match focused_container.as_window_container() {
    Ok(window) => window.native().clone(),
    _ => Platform::desktop_window(),
  };

  if Platform::foreground_window() != native_window {
    if let Ok(window) = focused_container.as_window_container() {
      info!("Setting focus to window: {window}");
    } else {
      info!("Setting focus to the desktop window.");
    }

    if let Err(err) = native_window.set_foreground() {
      warn!("Failed to set foreground window: {}", err);
    }
  }

  state.emit_event(WmEvent::FocusChanged {
    focused_container: focused_container.to_dto()?,
  });

  Ok(())
}

fn windows_to_bring_to_front(
  focused_container: &Container,
  state: &WmState,
) -> anyhow::Result<Vec<WindowContainer>> {
  let focused_workspace =
    focused_container.workspace().context("No workspace.")?;

  let workspaces_to_reorder = state
    .pending_sync
    .workspaces_to_reorder()
    .iter()
    .chain(
      state
        .pending_sync
        .needs_focus_update()
        .then_some(&focused_workspace),
    )
    .unique_by(|workspace| workspace.id());

  let windows_to_bring_to_front = workspaces_to_reorder
    .flat_map(|workspace| {
      let focused_descendant = workspace
        .descendant_focus_order()
        .next()
        .and_then(|container| container.as_window_container().ok());

      match focused_descendant {
        Some(focused_descendant) => workspace
          .descendants()
          .filter_map(|descendant| descendant.as_window_container().ok())
          .filter(|window| {
            let is_floating_or_tiling = matches!(
              window.state(),
              WindowState::Floating(_) | WindowState::Tiling
            );

            is_floating_or_tiling
              && window.state().is_same_state(&focused_descendant.state())
          })
          .collect(),
        None => vec![],
      }
    })
    .collect::<Vec<_>>();

  Ok(windows_to_bring_to_front)
}

#[allow(clippy::too_many_lines)]
fn redraw_containers(
  focused_container: &Container,
  state: &mut WmState,
  config: &UserConfig,
) -> anyhow::Result<()> {
  let windows_to_redraw = state.windows_to_redraw();
  let windows_to_bring_to_front =
    windows_to_bring_to_front(focused_container, state)?;

  let windows_to_update = {
    let mut windows = windows_to_redraw
      .iter()
      .chain(&windows_to_bring_to_front)
      .unique_by(|window| window.id())
      .collect::<Vec<_>>();

    let descendant_focus_order = state
      .root_container
      .descendant_focus_order()
      .collect::<Vec<_>>();

    windows.sort_by_key(|window| {
      descendant_focus_order
        .iter()
        .position(|order| order.id() == window.id())
    });

    windows
  };

  for window in windows_to_update.iter().rev() {
    let should_bring_to_front = windows_to_bring_to_front.contains(window);

    let workspace =
      window.workspace().context("Window has no workspace.")?;

    let z_order = match window.state() {
      WindowState::Floating(config) if config.shown_on_top => {
        ZOrder::TopMost
      }
      WindowState::Fullscreen(config) if config.shown_on_top => {
        ZOrder::TopMost
      }
      _ if should_bring_to_front => {
        let focused_descendant = workspace
          .descendant_focus_order()
          .next()
          .and_then(|container| container.as_window_container().ok());

        if let Some(focused_descendant) = focused_descendant {
          if window.id() == focused_descendant.id() {
            ZOrder::Normal
          } else {
            ZOrder::AfterWindow(focused_descendant.native().handle)
          }
        } else {
          ZOrder::Normal
        }
      }
      _ => ZOrder::Normal,
    };

    if should_bring_to_front && !windows_to_redraw.contains(window) {
      info!("Updating window z-order: {window}");
      if let Err(err) = window.native().set_z_order(&z_order) {
        warn!("Failed to set window z-order: {}", err);
      }
      continue;
    }

    window.set_display_state(
      match (window.display_state(), workspace.is_displayed()) {
        (DisplayState::Hidden | DisplayState::Hiding, true) => {
          DisplayState::Showing
        }
        (DisplayState::Shown | DisplayState::Showing, false) => {
          DisplayState::Hiding
        }
        _ => window.display_state(),
      },
    );

    let rect = window
      .to_rect()?
      .apply_delta(&window.total_border_delta()?, None);

    let is_visible = matches!(
      window.display_state(),
      DisplayState::Showing | DisplayState::Shown
    );

    let hide_method = config.value.general.hide_method.clone();
    let has_pending_dpi = window.has_pending_dpi_adjustment();
    let window_state = window.state().clone();
    let native_window = window.native().clone();
    
    if config.value.general.animations.enabled && is_visible {
        if let Some(handle) = state.animation_handles.remove(&native_window.handle) {
            handle.abort();
        }

        let animation_config = config.value.general.animations.clone();
        
        let task = task::spawn(async move {
             let start_rect = match native_window.frame_position() {
                 Ok(r) => r,
                 Err(_) => rect.clone()
             };
             
             let end_rect = rect;
             
             if (start_rect.x() - end_rect.x()).abs() < 2 && 
                (start_rect.y() - end_rect.y()).abs() < 2 &&
                (start_rect.width() - end_rect.width()).abs() < 2 &&
                (start_rect.height() - end_rect.height()).abs() < 2 {
                 
                 let _ = native_window.set_position(
                    &window_state,
                    &end_rect,
                    &z_order,
                    is_visible,
                    &hide_method,
                    has_pending_dpi,
                 );
                 return;
             }

             let _duration = Duration::from_millis(animation_config.duration_ms);
             let fps = animation_config.fps;
             let interval_ms = 1000 / fps;
             #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss, clippy::cast_precision_loss)]
             let steps = (animation_config.duration_ms as f64 / interval_ms as f64) as u32;
             
             let mut interval = tokio::time::interval(Duration::from_millis(interval_ms));
             
             for i in 1..=steps {
                 interval.tick().await;
                 #[allow(clippy::cast_precision_loss)]
                 let t = i as f32 / steps as f32;
                 // Easing: Cubic Out (1 - (1-t)^3)
                 let t = 1.0 - (1.0 - t).powi(3); 
                 
                 #[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
                 let cur_rect = wm_common::Rect::from_ltrb(
                     (start_rect.left as f32 + (end_rect.left as f32 - start_rect.left as f32) * t) as i32,
                     (start_rect.top as f32 + (end_rect.top as f32 - start_rect.top as f32) * t) as i32,
                     (start_rect.right as f32 + (end_rect.right as f32 - start_rect.right as f32) * t) as i32,
                     (start_rect.bottom as f32 + (end_rect.bottom as f32 - start_rect.bottom as f32) * t) as i32,
                 );

                 let _ = native_window.set_position(
                    &window_state,
                    &cur_rect,
                    &z_order,
                    is_visible,
                    &hide_method,
                    has_pending_dpi,
                 );
             }
             
             let _ = native_window.set_position(
                &window_state,
                &end_rect,
                &z_order,
                is_visible,
                &hide_method,
                has_pending_dpi,
             );
        });
        
        state.animation_handles.insert(window.native().handle, task);
        
    } else if let Err(err) = native_window.set_position(
          &window.state(),
          &rect,
          &z_order,
          is_visible,
          &hide_method,
          window.has_pending_dpi_adjustment(),
        ) {
          warn!("Failed to set window position: {}", err);
    }

    let is_transitioning_fullscreen =
      match (window.prev_state(), window.state()) {
        (Some(_), WindowState::Fullscreen(s)) if !s.maximized => true,
        (Some(WindowState::Fullscreen(_)), _) => true,
        _ => false,
      };

    if is_transitioning_fullscreen {
      if let Err(err) = window.native().mark_fullscreen(matches!(
        window.state(),
        WindowState::Fullscreen(_)
      )) {
        warn!("Failed to mark window as fullscreen: {}", err);
      }
    }

    if config.value.general.hide_method == HideMethod::Cloak
      && !config.value.general.show_all_in_taskbar
      && matches!(
        window.display_state(),
        DisplayState::Showing | DisplayState::Hiding
      )
    {
      if let Err(err) = window.native().set_taskbar_visibility(is_visible)
      {
        warn!("Failed to set taskbar visibility: {}", err);
      }
    }
  }

  Ok(())
}

fn jump_cursor(
  focused_container: Container,
  state: &WmState,
  config: &UserConfig,
) -> anyhow::Result<()> {
  let cursor_jump = &config.value.general.cursor_jump;

  let jump_target = match cursor_jump.trigger {
    CursorJumpTrigger::WindowFocus => Some(focused_container),
    CursorJumpTrigger::MonitorFocus => {
      let target_monitor =
        focused_container.monitor().context("No monitor.")?;

      let cursor_monitor = Platform::mouse_position()
        .ok()
        .and_then(|pos| state.monitor_at_point(&pos));

      cursor_monitor
        .filter(|monitor| monitor.id() != target_monitor.id())
        .map(|_| target_monitor.into())
    }
  };

  if let Some(jump_target) = jump_target {
    let center = jump_target.to_rect()?.center_point();

    if let Err(err) = Platform::set_cursor_pos(center.x, center.y) {
      warn!("Failed to set cursor position: {}", err);
    }
  }

  Ok(())
}

fn apply_window_effects(
  window: &WindowContainer,
  is_focused: bool,
  config: &UserConfig,
) {
  let window_effects = &config.value.window_effects;

  let effect_config = if is_focused {
    &window_effects.focused_window
  } else {
    &window_effects.other_windows
  };

  if window_effects.focused_window.border.enabled
    || window_effects.other_windows.border.enabled
  {
    apply_border_effect(window, effect_config);
  }

  if window_effects.focused_window.hide_title_bar.enabled
    || window_effects.other_windows.hide_title_bar.enabled
  {
    apply_hide_title_bar_effect(window, effect_config);
  }

  if window_effects.focused_window.corner_style.enabled
    || window_effects.other_windows.corner_style.enabled
  {
    apply_corner_effect(window, effect_config);
  }

  if window_effects.focused_window.transparency.enabled
    || window_effects.other_windows.transparency.enabled
  {
    apply_transparency_effect(window, effect_config);
  }
}

fn apply_border_effect(
  window: &WindowContainer,
  effect_config: &WindowEffectConfig,
) {
  let border_color = if effect_config.border.enabled {
    Some(&effect_config.border.color)
  } else {
    None
  };

  _ = window.native().set_border_color(border_color);

  let native = window.native().clone();
  let border_color = border_color.cloned();

  task::spawn(async move {
    tokio::time::sleep(Duration::from_millis(50)).await;
    _ = native.set_border_color(border_color.as_ref());
  });
}

fn apply_hide_title_bar_effect(
  window: &WindowContainer,
  effect_config: &WindowEffectConfig,
) {
  _ = window
    .native()
    .set_title_bar_visibility(!effect_config.hide_title_bar.enabled);
}

fn apply_corner_effect(
  window: &WindowContainer,
  effect_config: &WindowEffectConfig,
) {
  let corner_style = if effect_config.corner_style.enabled {
    &effect_config.corner_style.style
  } else {
    &CornerStyle::Default
  };

  _ = window.native().set_corner_style(corner_style);
}

fn apply_transparency_effect(
  window: &WindowContainer,
  effect_config: &WindowEffectConfig,
) {
  let transparency = if effect_config.transparency.enabled {
    &effect_config.transparency.opacity
  } else {
    &OpacityValue::from_alpha(u8::MAX)
  };

  _ = window.native().set_transparency(transparency);
}