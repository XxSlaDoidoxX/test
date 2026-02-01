use anyhow::Context;
use wm_platform::{MouseMoveEvent, Platform, NativeWindow};
use wm_common::{Point, WindowState, InvokeCommand};
use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_MENU, VK_LBUTTON};

use crate::{
  commands::{
      container::set_focused_descendant, 
      window::{set_window_position, update_window_state, WindowPositionTarget},
  },
  models::{Container, WindowContainer},
  traits::{CommonGetters, PositionGetters, WindowGetters},
  user_config::UserConfig, 
  wm_state::{WmState, DragState},
};

pub fn handle_mouse_move(
  event: &MouseMoveEvent,
  state: &mut WmState,
  config: &UserConfig,
) -> anyhow::Result<()> {
  
  // [Added] Alt + Drag Implementation
  // Check if Alt is held down (high bit set if key is down)
  let alt_down = unsafe { GetAsyncKeyState(VK_MENU.0 as i32) } as i16 & 0x8000 != 0;
  
  // Initialize drag state if Alt+Click just started
  if alt_down && event.is_mouse_down && state.drag_state.is_none() {
      if let Ok(window) = Platform::window_from_point(&event.point)
        .and_then(|w| Platform::root_ancestor(&w)) 
        .map(|root| state.window_from_native(&root)) 
        .transpose() 
      {
         if let Some(win_container) = window {
             state.drag_state = Some(DragState {
                 start_point: event.point,
                 window_id: win_container.id(),
                 is_dragging: true
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
  }

  // Handle Dragging
  if let Some(drag_state) = &state.drag_state {
      if !event.is_mouse_down {
          // Mouse released, clear drag state
          state.drag_state = None;
      } else {
          // Dragging logic
          if let Some(container) = state.container_by_id(drag_state.window_id) {
              if let Ok(window) = container.as_window_container() {
                   // Calculate delta
                   // Since we receive absolute points, we can just update position
                   // Note: We might want smoother delta tracking, but setting absolute pos follows cursor best
                   
                   // Get current window rect to calculate offset from original click
                   // A simpler approach for "Move" is just centering the window on cursor or maintaining offset
                   // For now, let's just move the window based on delta from *last* event, 
                   // but we only have current event point.
                   
                   // We need the window's current position to apply the delta from the *previous* frame.
                   // But `event.point` is absolute. 
                   // Let's rely on standard logic: Current Pos = Window Pos + (Current Mouse - Last Mouse)
                   // Since we don't store "Last Mouse" easily without more state, 
                   // we can just set the window position to (Mouse Pos - Offset).
                   // Let's assume user wants to drag from the clicked point.
                   
                   // Implementation:
                   // 1. Get window rect.
                   // 2. We don't have the initial offset stored in DragState. 
                   //    Let's stick to a simple "follow cursor" or rely on the user dragging.
                   //    Actually, standard drag is: NewWinPos = OldWinPos + (MouseDelta).
                   //    We need `last_mouse_pos`.
                   
                   // Hack: We can use the drag_state.start_point, but that snaps window to start.
                   // Better: Use static or store last_pos in DragState.
                   // Since I can't easily change DragState definition *again* without rewriting wm_state, 
                   // let's assume we update start_point every frame.
              }
          }
          
          // Update start point for next delta
          if let Some(ds) = &mut state.drag_state {
               let delta_x = event.point.x - ds.start_point.x;
               let delta_y = event.point.y - ds.start_point.y;
               
               if let Some(container) = state.container_by_id(ds.window_id) {
                   if let Ok(window) = container.as_window_container() {
                       if let Ok(rect) = window.to_rect() {
                           let new_x = rect.x() + delta_x;
                           let new_y = rect.y() + delta_y;
                           
                           set_window_position(
                               window, 
                               &WindowPositionTarget::Coordinates(new_x, new_y), 
                               state
                           )?;
                       }
                   }
               }
               ds.start_point = event.point;
          }
          return Ok(()); // Swallow event if dragging
      }
  }

  // Original Logic
  if event.is_mouse_down
    || !state.is_focus_synced
    || !config.value.general.focus_follows_cursor
  {
    return Ok(());
  }

  let window_under_cursor = Platform::window_from_point(&event.point)
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
      .monitor_at_point(&event.point)
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
