use std::time::Instant;
use std::collections::HashMap;

use anyhow::Context;
use tokio::sync::mpsc::{self};
use tokio::task::JoinHandle;
use tracing::warn;
use uuid::Uuid;
use wm_common::{
  BindingModeConfig, Direction, Point, WindowState, WmEvent,
};
use wm_platform::{NativeMonitor, NativeWindow, Platform};

use crate::{
  commands::{
    container::set_focused_descendant, general::platform_sync,
    monitor::add_monitor, window::manage_window,
  },
  models::{
    Container, Monitor, RootContainer, WindowContainer, Workspace,
    WorkspaceTarget,
  },
  pending_sync::PendingSync,
  traits::{CommonGetters, PositionGetters, WindowGetters},
  user_config::UserConfig,
};

// [Modified] State for Alt+Drag operation
pub struct DragState {
    pub start_point: Point,
    pub window_id: Uuid,
}

pub struct WmState {
  pub root_container: RootContainer,
  pub pending_sync: PendingSync,
  pub recent_workspace_name: Option<String>,
  pub prev_effects_window: Option<WindowContainer>,
  pub unmanaged_or_minimized_timestamp: Option<Instant>,
  pub binding_modes: Vec<BindingModeConfig>,
  pub ignored_windows: Vec<NativeWindow>,
  pub is_paused: bool,
  pub is_focus_synced: bool,
  pub has_initialized: bool,
  event_tx: mpsc::UnboundedSender<WmEvent>,
  exit_tx: mpsc::UnboundedSender<()>,

  // Map to store active animation tasks
  pub animation_handles: HashMap<isize, JoinHandle<()>>,

  // Drag state for Alt+Drag
  pub drag_state: Option<DragState>,
}

impl WmState {
  pub fn new(
    event_tx: mpsc::UnboundedSender<WmEvent>,
    exit_tx: mpsc::UnboundedSender<()>,
  ) -> Self {
    Self {
      root_container: RootContainer::new(),
      pending_sync: PendingSync::default(),
      prev_effects_window: None,
      recent_workspace_name: None,
      unmanaged_or_minimized_timestamp: None,
      binding_modes: Vec::new(),
      ignored_windows: Vec::new(),
      is_paused: false,
      is_focus_synced: false,
      has_initialized: false,
      event_tx,
      exit_tx,
      animation_handles: HashMap::new(),
      drag_state: None,
    }
  }

  pub fn populate(
    &mut self,
    config: &mut UserConfig,
  ) -> anyhow::Result<()> {
    let foreground_window = Platform::foreground_window();

    for native_monitor in Platform::sorted_monitors()? {
      add_monitor(native_monitor, self, config)?;
    }

    for native_window in Platform::manageable_windows()?.into_iter().rev()
    {
      let nearest_workspace = self
        .nearest_monitor(&native_window)
        .and_then(|m| m.displayed_workspace());

      if let Some(workspace) = nearest_workspace {
        manage_window(
          native_window,
          Some(workspace.into()),
          self,
          config,
        )?;
      }
    }

    let container_to_focus = self
      .window_from_native(&foreground_window)
      .map(|c| c.as_container())
      .or(self.windows().pop().map(Into::into))
      .or(self.workspaces().pop().map(Into::into))
      .context("Failed to get container to focus.")?;

    set_focused_descendant(&container_to_focus, None);
    self.is_focus_synced = true;

    self
      .pending_sync
      .queue_focus_change()
      .queue_all_effects_update();

    for workspace in self.workspaces() {
      self.pending_sync.queue_workspace_to_reorder(workspace);
    }

    platform_sync(self, config)?;
    self.has_initialized = true;

    Ok(())
  }

  pub fn monitors(&self) -> Vec<Monitor> {
    self.root_container.monitors()
  }

  pub fn workspaces(&self) -> Vec<Workspace> {
    self
      .monitors()
      .iter()
      .flat_map(Monitor::workspaces)
      .collect()
  }

  pub fn sorted_workspaces(&self, config: &UserConfig) -> Vec<Workspace> {
    let mut workspaces = self.workspaces();
    config.sort_workspaces(&mut workspaces);
    workspaces
  }

  pub fn windows(&self) -> Vec<WindowContainer> {
    self
      .root_container
      .descendants()
      .filter_map(|container| container.try_into().ok())
      .collect()
  }

  pub fn nearest_monitor(
    &self,
    native_window: &NativeWindow,
  ) -> Option<Monitor> {
    self
      .monitor_from_native(&Platform::nearest_monitor(native_window))
      .or(self.monitors().first().cloned())
  }

  pub fn monitor_from_native(
    &self,
    native_monitor: &NativeMonitor,
  ) -> Option<Monitor> {
    self
      .monitors()
      .into_iter()
      .find(|monitor| monitor.native() == *native_monitor)
  }

  pub fn monitor_in_direction(
    &self,
    origin_monitor: &Monitor,
    direction: &Direction,
  ) -> anyhow::Result<Option<Monitor>> {
    let origin_rect = origin_monitor.native().rect()?.clone();

    let monitors_with_rect = self
      .monitors()
      .into_iter()
      .map(|monitor| {
        let rect = monitor.native().rect()?.clone();
        anyhow::Ok((monitor, rect))
      })
      .try_collect::<Vec<_>>()?;

    let closest_monitor = monitors_with_rect
      .into_iter()
      .filter(|(_, rect)| match direction {
        Direction::Right => {
          rect.x() > origin_rect.x() && rect.has_overlap_y(&origin_rect)
        }
        Direction::Left => {
          rect.x() < origin_rect.x() && rect.has_overlap_y(&origin_rect)
        }
        Direction::Down => {
          rect.y() > origin_rect.y() && rect.has_overlap_x(&origin_rect)
        }
        Direction::Up => {
          rect.y() < origin_rect.y() && rect.has_overlap_x(&origin_rect)
        }
      })
      .min_by(|(_, rect_a), (_, rect_b)| match direction {
        Direction::Right => rect_a.x().cmp(&rect_b.x()),
        Direction::Left => rect_b.x().cmp(&rect_a.x()),
        Direction::Down => rect_a.y().cmp(&rect_b.y()),
        Direction::Up => rect_b.y().cmp(&rect_a.y()),
      })
      .map(|(monitor, _)| monitor);

    Ok(closest_monitor)
  }

  pub fn window_from_native(
    &self,
    native_window: &NativeWindow,
  ) -> Option<WindowContainer> {
    self
      .windows()
      .into_iter()
      .find(|window| &*window.native() == native_window)
  }

  pub fn workspace_by_name(
    &self,
    workspace_name: &str,
  ) -> Option<Workspace> {
    self
      .workspaces()
      .into_iter()
      .find(|workspace| workspace.config().name == workspace_name)
  }

  #[allow(clippy::too_many_lines)]
  pub fn workspace_by_target(
    &self,
    origin_workspace: &Workspace,
    target: WorkspaceTarget,
    config: &UserConfig,
  ) -> anyhow::Result<(Option<String>, Option<Workspace>)> {
    let (name, workspace) = match target {
      WorkspaceTarget::Name(name) => {
        #[allow(clippy::match_bool)]
        match origin_workspace.config().name == name {
          false => (Some(name.clone()), self.workspace_by_name(&name)),
          true if config.value.general.toggle_workspace_on_refocus => (
            self.recent_workspace_name.clone(),
            self
              .recent_workspace_name
              .as_ref()
              .and_then(|name| self.workspace_by_name(name)),
          ),
          true => (None, None),
        }
      }
      WorkspaceTarget::Recent => (
        self.recent_workspace_name.clone(),
        self
          .recent_workspace_name
          .as_ref()
          .and_then(|name| self.workspace_by_name(name)),
      ),
      WorkspaceTarget::NextActive => {
        let active_workspaces = self.sorted_workspaces(config);
        let origin_index = active_workspaces
          .iter()
          .position(|workspace| workspace.id() == origin_workspace.id())
          .context("Failed to get index of given workspace.")?;

        let next_active_workspace = active_workspaces
          .get(origin_index + 1)
          .or_else(|| active_workspaces.first());

        (
          next_active_workspace.map(|workspace| workspace.config().name),
          next_active_workspace.cloned(),
        )
      }
      WorkspaceTarget::PreviousActive => {
        let active_workspaces = self.sorted_workspaces(config);
        let origin_index = active_workspaces
          .iter()
          .position(|workspace| workspace.id() == origin_workspace.id())
          .context("Failed to get index of given workspace.")?;

        let prev_active_workspace = active_workspaces.get(
          origin_index
            .checked_sub(1)
            .unwrap_or(active_workspaces.len() - 1),
        );

        (
          prev_active_workspace.map(|workspace| workspace.config().name),
          prev_active_workspace.cloned(),
        )
      }
      WorkspaceTarget::NextActiveInMonitor => {
        let monitor = origin_workspace
          .monitor()
          .context("No monitor in workspace")?;

        let mut workspace_in_monitor = monitor.workspaces();
        config.sort_workspaces(&mut workspace_in_monitor);

        let origin_index = workspace_in_monitor
          .iter()
          .position(|workspace| workspace.id() == origin_workspace.id())
          .context("Failed to get index of give workspace")?;

        let next_active_workspace_in_monitor = workspace_in_monitor
          .get(origin_index + 1)
          .or_else(|| workspace_in_monitor.first());

        (
          next_active_workspace_in_monitor
            .map(|workspace| workspace.config().name),
          next_active_workspace_in_monitor.cloned(),
        )
      }
      WorkspaceTarget::PreviousActiveInMonitor => {
        let monitor = origin_workspace
          .monitor()
          .context("No monitor in workspace")?;

        let mut workspace_in_monitor = monitor.workspaces();
        config.sort_workspaces(&mut workspace_in_monitor);

        let origin_index = workspace_in_monitor
          .iter()
          .position(|workspace| workspace.id() == origin_workspace.id())
          .context("Failed to get index of give workspace")?;

        let prev_active_workspace_in_monitor = workspace_in_monitor.get(
          origin_index
            .checked_sub(1)
            .unwrap_or(workspace_in_monitor.len() - 1),
        );

        (
          prev_active_workspace_in_monitor
            .map(|workspace| workspace.config().name),
          prev_active_workspace_in_monitor.cloned(),
        )
      }
      WorkspaceTarget::Next => {
        let workspaces = &config.value.workspaces;
        let origin_name = origin_workspace.config().name.clone();
        let origin_index = workspaces
          .iter()
          .position(|workspace| workspace.name == origin_name)
          .context("Failed to get index of given workspace.")?;

        let next_workspace_config = workspaces
          .get(origin_index + 1)
          .or_else(|| workspaces.first());

        let next_workspace_name =
          next_workspace_config.map(|config| config.name.clone());

        let next_workspace = next_workspace_name
          .as_ref()
          .and_then(|name| self.workspace_by_name(name));

        (next_workspace_name, next_workspace)
      }
      WorkspaceTarget::Previous => {
        let workspaces = &config.value.workspaces;
        let origin_name = origin_workspace.config().name.clone();
        let origin_index = workspaces
          .iter()
          .position(|workspace| workspace.name == origin_name)
          .context("Failed to get index of given workspace.")?;

        let previous_workspace_config = workspaces.get(
          origin_index.checked_sub(1).unwrap_or(workspaces.len() - 1),
        );

        let previous_workspace_name =
          previous_workspace_config.map(|config| config.name.clone());

        let previous_workspace = previous_workspace_name
          .as_ref()
          .and_then(|name| self.workspace_by_name(name));

        (previous_workspace_name, previous_workspace)
      }

      WorkspaceTarget::Direction(direction) => {
        let origin_monitor =
          origin_workspace.monitor().context("No focused monitor.")?;

        let target_workspace = self
          .monitor_in_direction(&origin_monitor, &direction)?
          .and_then(|monitor| monitor.displayed_workspace());

        (
          target_workspace
            .as_ref()
            .map(|workspace| workspace.config().name),
          target_workspace,
        )
      }
    };

    Ok((name, workspace))
  }

  pub fn windows_to_redraw(&self) -> Vec<WindowContainer> {
    self
      .pending_sync
      .containers_to_redraw()
      .values()
      .flat_map(CommonGetters::self_and_descendants)
      .filter(|container| !container.is_detached())
      .filter_map(|container| container.try_into().ok())
      .collect()
  }

  pub fn focused_container(&self) -> Option<Container> {
    self.root_container.descendant_focus_order().next()
  }

  pub fn emit_event(&self, event: WmEvent) {
    if self.has_initialized
      && (!self.is_paused || matches!(event, WmEvent::PauseChanged { .. }))
    {
      if let Err(err) = self.event_tx.send(event) {
        warn!("Failed to send event: {}", err);
      }
    }
  }

  pub fn emit_exit(&self) -> anyhow::Result<()> {
    self.exit_tx.send(())?;
    Ok(())
  }

  pub fn container_by_id(&self, id: Uuid) -> Option<Container> {
    self
      .root_container
      .self_and_descendants()
      .find(|container| container.id() == id)
  }

  pub fn focus_target_after_removal(
    &self,
    removed_window: &WindowContainer,
  ) -> Option<Container> {
    if self.focused_container() != Some(removed_window.clone().into()) {
      return None;
    }

    let workspace = removed_window.workspace()?;
    let descendant_focus_order = workspace
      .descendant_focus_order()
      .filter(|descendant| descendant.id() != removed_window.id())
      .collect::<Vec<_>>();

    let focus_target_of_type = descendant_focus_order
      .iter()
      .filter_map(|descendant| descendant.as_window_container().ok())
      .find(|descendant| {
        matches!(
          (descendant.state(), removed_window.state()),
          (WindowState::Tiling, WindowState::Tiling)
            | (WindowState::Floating(_), WindowState::Floating(_))
            | (WindowState::Fullscreen(_), WindowState::Fullscreen(_))
        )
      })
      .map(Into::into);

    if focus_target_of_type.is_some() {
      return focus_target_of_type;
    }

    let non_minimized_focus_target = descendant_focus_order
      .iter()
      .filter_map(|descendant| descendant.as_window_container().ok())
      .find(|descendant| descendant.state() != WindowState::Minimized)
      .map(Into::into);

    non_minimized_focus_target
      .or(descendant_focus_order.first().cloned())
      .or(Some(workspace.into()))
  }

  #[allow(clippy::unused_self)]
  pub fn containers_at_point(
    &self,
    origin_container: &Container,
    point: &Point,
  ) -> Vec<Container> {
    origin_container
      .descendants()
      .filter(|descendant| {
        descendant
          .to_rect()
          .is_ok_and(|rect| rect.contains_point(point))
      })
      .collect()
  }

  pub fn monitor_at_point(&self, point: &Point) -> Option<Monitor> {
    self
      .monitors()
      .iter()
      .find(|monitor| {
        monitor
          .to_rect()
          .is_ok_and(|rect| rect.contains_point(point))
      })
      .cloned()
  }
}

impl Drop for WmState {
  fn drop(&mut self) {
    let managed_windows = self
      .windows()
      .into_iter()
      .map(|window| window.native().clone())
      .collect::<Vec<_>>();

    for window in managed_windows {
      window.cleanup();
    }
  }
}
