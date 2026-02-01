use anyhow::Context;
use tracing::info;
use wm_common::{
  try_warn, LengthValue, RectDelta, WindowRuleEvent, WindowState, WmEvent, TilingDirection
};
use wm_platform::{NativeWindow, Platform};

use crate::{
  commands::{
    container::{attach_container, set_focused_descendant, wrap_in_split_container, set_tiling_direction},
    window::run_window_rules,
  },
  models::{
    Container, Monitor, NonTilingWindow, TilingWindow, WindowContainer, SplitContainer
  },
  traits::{CommonGetters, PositionGetters, WindowGetters, TilingDirectionGetters},
  user_config::UserConfig,
  wm_state::WmState,
};

pub fn manage_window(
  native_window: NativeWindow,
  target_parent: Option<Container>,
  state: &mut WmState,
  config: &mut UserConfig,
) -> anyhow::Result<()> {
  let window =
    try_warn!(create_window(native_window, target_parent, state, config));

  set_focused_descendant(&window.clone().into(), None);

  let updated_window = run_window_rules(
    window.clone(),
    &WindowRuleEvent::Manage,
    state,
    config,
  )?;

  if let Some(window) = updated_window {
    info!("New window managed: {window}");

    state.emit_event(WmEvent::WindowManaged {
      managed_window: window.to_dto()?,
    });

    state.pending_sync.queue_focus_change();

    state.pending_sync.queue_focused_effect_update();
    state.pending_sync.queue_workspace_to_reorder(
      window.workspace().context("No workspace.")?,
    );

    state.pending_sync.queue_container_to_redraw(
      if window.state() == WindowState::Tiling {
        window.parent().context("No parent.")?
      } else {
        window.into()
      },
    );
  }

  Ok(())
}

fn create_window(
  native_window: NativeWindow,
  target_parent: Option<Container>,
  state: &mut WmState,
  config: &UserConfig,
) -> anyhow::Result<WindowContainer> {
  let nearest_monitor = state
    .nearest_monitor(&native_window)
    .context("No nearest monitor.")?;

  let nearest_workspace = nearest_monitor
    .displayed_workspace()
    .context("No nearest workspace.")?;

  let gaps_config = config.value.gaps.clone();
  let window_state =
    window_state_to_create(&native_window, &nearest_monitor, config)?;

  let (target_parent, target_index) = match target_parent {
    Some(parent) => (parent, 0),
    None => insertion_target(&window_state, state, config)?,
  };

  let target_workspace =
    target_parent.workspace().context("No target workspace.")?;

  let prefers_centered = config
    .value
    .window_behavior
    .state_defaults
    .floating
    .centered;

  let is_same_workspace = nearest_workspace.id() == target_workspace.id();
  let floating_placement = {
    let placement = if !is_same_workspace || prefers_centered {
      native_window
        .frame_position()?
        .translate_to_center(&target_workspace.to_rect()?)
    } else {
      native_window.frame_position()?
    };

    #[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation)]
    placement.clamp_size(
      (target_workspace.to_rect()?.width() as f32 * 0.9) as i32,
      (target_workspace.to_rect()?.height() as f32 * 0.9) as i32,
    )
  };

  let border_delta = RectDelta::new(
    LengthValue::from_px(0),
    LengthValue::from_px(0),
    LengthValue::from_px(0),
    LengthValue::from_px(0),
  );

  let window_container: WindowContainer = match window_state {
    WindowState::Tiling => TilingWindow::new(
      None,
      native_window,
      None,
      border_delta,
      floating_placement,
      false,
      gaps_config,
      Vec::new(),
      None,
    )
    .into(),
    _ => NonTilingWindow::new(
      None,
      native_window,
      window_state,
      None,
      border_delta,
      None,
      floating_placement,
      false,
      Vec::new(),
      None,
    )
    .into(),
  };

  attach_container(
    &window_container.clone().into(),
    &target_parent,
    Some(target_index),
  )?;

  if nearest_monitor
    .has_dpi_difference(&window_container.clone().into())?
  {
    window_container.set_has_pending_dpi_adjustment(true);
  }

  Ok(window_container)
}

fn window_state_to_create(
  native_window: &NativeWindow,
  nearest_monitor: &Monitor,
  config: &UserConfig,
) -> anyhow::Result<WindowState> {
  if native_window.is_minimized()? {
    return Ok(WindowState::Minimized);
  }

  let nearest_workspace = nearest_monitor
    .displayed_workspace()
    .context("No Workspace.")?;

  let monitor_rect = if config
    .outer_gaps_for_workspace(&nearest_workspace)
    .is_significant()
  {
    nearest_monitor.native().working_rect()?.clone()
  } else {
    nearest_monitor.to_rect()?
  };

  if native_window.is_fullscreen(&monitor_rect)? {
    return Ok(WindowState::Fullscreen(
      config
        .value
        .window_behavior
        .state_defaults
        .fullscreen
        .clone(),
    ));
  }

  if !native_window.is_resizable() {
    return Ok(WindowState::Floating(
      config.value.window_behavior.state_defaults.floating.clone(),
    ));
  }

  Ok(WindowState::default_from_config(&config.value))
}

// [Modified] Dynamic Tiling Logic
fn insertion_target(
  window_state: &WindowState,
  state: &mut WmState,
  config: &UserConfig,
) -> anyhow::Result<(Container, usize)> {
  let focused_container =
    state.focused_container().context("No focused container.")?;

  let focused_workspace =
    focused_container.workspace().context("No workspace.")?;

  if *window_state == WindowState::Tiling {
      // Hyprland-style: Check mouse position relative to focused window
      if let Ok(focused_tiling) = focused_container.as_tiling_container() {
          // Get mouse position
          if let Ok(mouse_pos) = Platform::mouse_position() {
              if let Ok(rect) = focused_tiling.to_rect() {
                  if rect.contains_point(&mouse_pos) {
                       let center = rect.center_point();
                       let delta_x = (mouse_pos.x - center.x) as f32;
                       let delta_y = (mouse_pos.y - center.y) as f32;
                       let width = rect.width() as f32;
                       let height = rect.height() as f32;

                       // Determine desired split based on quadrant
                       // If horizontal distance (normalized) > vertical distance (normalized) -> Horizontal split
                       let desired_dir = if (delta_x.abs() / width) > (delta_y.abs() / height) {
                           TilingDirection::Horizontal
                       } else {
                           TilingDirection::Vertical
                       };
                       
                       // Determine insertion index (Before or After)
                       let insert_after = match desired_dir {
                           TilingDirection::Horizontal => delta_x > 0.0,
                           TilingDirection::Vertical => delta_y > 0.0,
                       };

                       let parent = focused_tiling.parent().context("No parent")?;
                       let current_dir = parent.tiling_direction();

                       if current_dir == desired_dir {
                           // Same direction, just insert next to it
                           let index = focused_tiling.index();
                           return Ok((parent, if insert_after { index + 1 } else { index }));
                       } else {
                           // Different direction, need to wrap focused window
                           // If the parent only has 1 child (the focused one), we can just change the direction!
                           if parent.child_count() == 1 {
                                set_tiling_direction(&parent, state, config, &desired_dir)?;
                                return Ok((parent, if insert_after { 1 } else { 0 }));
                           } 

                           // Else, wrap in new split container
                           let split = SplitContainer::new(
                               None,
                               desired_dir,
                               None,
                               Vec::new(),
                               None
                           );
                           
                           // Wrap focused window
                           wrap_in_split_container(
                               &split, 
                               &parent, 
                               &[focused_tiling.clone()]
                           )?;

                           // Return the new split container as parent
                           return Ok((split.into(), if insert_after { 1 } else { 0 }));
                       }
                  }
              }
          }
      }

    // Fallback logic
    let sibling = match focused_container {
      Container::TilingWindow(_) => Some(focused_container),
      _ => focused_workspace
        .descendant_focus_order()
        .find(Container::is_tiling_window),
    };

    if let Some(sibling) = sibling {
      return Ok((
        sibling.parent().context("No parent.")?,
        sibling.index() + 1,
      ));
    }
  }

  Ok((
    focused_workspace.clone().into(),
    focused_workspace.child_count(),
  ))
}
