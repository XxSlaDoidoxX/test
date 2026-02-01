use anyhow::Context;
use wm_platform::{MouseMoveEvent, Platform};
use wm_common::{WindowState};

use crate::{
  commands::{
      container::set_focused_descendant, 
      window::{set_window_position, update_window_state, WindowPositionTarget},
  },
  traits::{CommonGetters, PositionGetters, WindowGetters},
  user_config::UserConfig, 
  wm_state::{WmState, DragState},
};

pub fn handle_mouse_move(
  event: &MouseMoveEvent,
  state: &mut WmState,
  config: &UserConfig,
) -> anyhow::Result<()> {
  
  // Use Platform helper for robust key detection
  let alt_down = Platform::is_alt_down();
  let lbutton_down = Platform::is_lbutton_down();
  
  // Initialize drag state if Alt+Click just started and we aren't dragging yet
  if alt_down && lbutton_down && state.drag_state.is_none() {
      // Platform::window_from_point -> Result<NativeWindow>
      if let Some(Ok(win_container)) = Platform::window_from_point(&event.point.clone())
        .and_then(|w| Platform::root_ancestor(&w)) 
        .map(|root| state.window_from_native(&root)) 
        .transpose() 
      {
         state.drag_state = Some(DragState {
             start_point: event.point.clone(),
             window_id: win_container.id(),
         });
         
         // If tiling, float it immediately to allow dragging
         if matches!(win_container.state(), WindowState::Tiling) {
             update_window_state(
                 win_container, 
                 WindowState::Floating(config.value.window_behavior.state_defaults.floating.clone()), 
                 state, 
                 config
             )?;
         }
      }
  }

  // Handle Dragging
  if let Some(drag_state) = &state.drag_state {
      // We check if LButton is still down to continue dragging
      if lbutton_down {
          let delta_x = event.point.x - drag_state.start_point.x;
          let delta_y = event.point.y - drag_state.start_point.y;
          let window_id = drag_state.window_id;

          if let Some(container) = state.container_by_id(window_id) {
               if let Ok(window) = container.as_window_container() {
                   if let Ok(rect) = window.to_rect() {
                       let new_x = rect.x() + delta_x;
                       let new_y = rect.y() + delta_y;
                       
                       set_window_position(
                           window, 
                           &WindowPositionTarget::Coordinates(Some(new_x), Some(new_y)), 
                           state
                       )?;
                   }
               }
           }
           
           // Update the stored start point to current point
           if let Some(ds) = &mut state.drag_state {
               ds.start_point = event.point.clone();
           }

          return Ok(());
      } 
      
      // Mouse released, clear drag state
      state.drag_state = None;
  }

  if event.is_mouse_down
    || !state.is_focus_synced
    || !config.value.general.focus_follows_cursor
  {
    return Ok(());
  }

  let window_under_cursor = Platform::window_from_point(&event.point.clone())
    .and_then(|window| Platform::root_ancestor(&window))
    .map(|root| state.window_from_native(&root))?;

  if let Some(window) = window_under_cursor {
    let focused_container =
      state.focused_container().context("No focused container.")?;

    if focused_container.id() != window.id() {
      set_focused_descendant(&window.as_container(), None);
      state.pending_sync.queue_focus_change();
    }
  } else {
    let cursor_monitor = state
      .monitor_at_point(&event.point.clone())
      .context("No monitor under cursor.")?;

    let focused_monitor = state
      .focused_container()
      .context("No focused container.")?
      .monitor()
      .context("Focused container has no monitor.")?;

    if cursor_monitor.id() != focused_monitor.id() {
      set_focused_descendant(&cursor_monitor.as_container(), None);
      state.pending_sync.queue_focus_change();
    }
  }

  Ok(())
}
