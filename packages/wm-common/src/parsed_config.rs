use serde::{Deserialize, Serialize};

use crate::{
  app_command::InvokeCommand, Color, LengthValue, OpacityValue, RectDelta,
};

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default, rename_all(serialize = "camelCase"))]
pub struct ParsedConfig {
  pub binding_modes: Vec<BindingModeConfig>,
  pub gaps: GapsConfig,
  pub general: GeneralConfig,
  pub keybindings: Vec<KeybindingConfig>,
  pub window_behavior: WindowBehaviorConfig,
  pub window_effects: WindowEffectsConfig,
  pub window_rules: Vec<WindowRuleConfig>,
  pub workspaces: Vec<WorkspaceConfig>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all(serialize = "camelCase"))]
pub struct BindingModeConfig {
  pub name: String,
  #[serde(default)]
  pub display_name: Option<String>,
  #[serde(default)]
  pub keybindings: Vec<KeybindingConfig>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, rename_all(serialize = "camelCase"))]
pub struct GapsConfig {
  pub scale_with_dpi: bool,
  pub inner_gap: LengthValue,
  pub outer_gap: RectDelta,
  pub single_window_outer_gap: Option<RectDelta>,
}

impl Default for GapsConfig {
  fn default() -> Self {
    GapsConfig {
      scale_with_dpi: true,
      inner_gap: LengthValue::from_px(0),
      outer_gap: RectDelta::new(
        LengthValue::from_px(0),
        LengthValue::from_px(0),
        LengthValue::from_px(0),
        LengthValue::from_px(0),
      ),
      single_window_outer_gap: None,
    }
  }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, rename_all(serialize = "camelCase"))]
pub struct GeneralConfig {
  pub cursor_jump: CursorJumpConfig,
  pub focus_follows_cursor: bool,
  pub toggle_workspace_on_refocus: bool,
  pub startup_commands: Vec<InvokeCommand>,
  pub shutdown_commands: Vec<InvokeCommand>,
  pub config_reload_commands: Vec<InvokeCommand>,
  pub hide_method: HideMethod,
  pub show_all_in_taskbar: bool,
  // [Added] Animation configuration
  pub animations: AnimationConfig,
}

impl Default for GeneralConfig {
  fn default() -> Self {
    GeneralConfig {
      cursor_jump: CursorJumpConfig::default(),
      focus_follows_cursor: false,
      toggle_workspace_on_refocus: true,
      startup_commands: vec![],
      shutdown_commands: vec![],
      config_reload_commands: vec![],
      hide_method: HideMethod::Cloak,
      show_all_in_taskbar: false,
      animations: AnimationConfig::default(),
    }
  }
}

// [Added] Struct for animation settings
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, rename_all(serialize = "camelCase"))]
pub struct AnimationConfig {
    pub enabled: bool,
    pub duration_ms: u64,
    pub fps: u64,
}

impl Default for AnimationConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            duration_ms: 150, // Default to a snappy but smooth feel
            fps: 144, // High refresh rate by default
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default, rename_all(serialize = "camelCase"))]
pub struct CursorJumpConfig {
  pub enabled: bool,
  pub trigger: CursorJumpTrigger,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CursorJumpTrigger {
  #[default]
  MonitorFocus,
  WindowFocus,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HideMethod {
  Hide,
  #[default]
  Cloak,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default, rename_all(serialize = "camelCase"))]
pub struct KeybindingConfig {
  pub bindings: Vec<String>,
  pub commands: Vec<InvokeCommand>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default, rename_all(serialize = "camelCase"))]
pub struct WindowBehaviorConfig {
  pub initial_state: InitialWindowState,
  pub state_defaults: WindowStateDefaultsConfig,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum InitialWindowState {
  #[default]
  Tiling,
  Floating,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default, rename_all(serialize = "camelCase"))]
pub struct WindowStateDefaultsConfig {
  pub floating: FloatingStateConfig,
  pub fullscreen: FullscreenStateConfig,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(default, rename_all(serialize = "camelCase"))]
pub struct FloatingStateConfig {
  pub centered: bool,
  pub shown_on_top: bool,
}

impl Default for FloatingStateConfig {
  fn default() -> Self {
    FloatingStateConfig {
      centered: true,
      shown_on_top: false,
    }
  }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(default, rename_all(serialize = "camelCase"))]
pub struct FullscreenStateConfig {
  pub maximized: bool,
  pub shown_on_top: bool,
}

impl Default for FullscreenStateConfig {
  fn default() -> Self {
    FullscreenStateConfig {
      maximized: true,
      shown_on_top: false,
    }
  }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default, rename_all(serialize = "camelCase"))]
pub struct WindowEffectsConfig {
  pub focused_window: WindowEffectConfig,
  pub other_windows: WindowEffectConfig,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default, rename_all(serialize = "camelCase"))]
pub struct WindowEffectConfig {
  pub border: BorderEffectConfig,
  pub hide_title_bar: HideTitleBarEffectConfig,
  pub corner_style: CornerEffectConfig,
  pub transparency: TransparencyEffectConfig,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, rename_all(serialize = "camelCase"))]
pub struct BorderEffectConfig {
  pub enabled: bool,
  pub color: Color,
}

impl Default for BorderEffectConfig {
  fn default() -> Self {
    BorderEffectConfig {
      enabled: false,
      color: Color {
        r: 140,
        g: 190,
        b: 255,
        a: 255,
      },
    }
  }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default, rename_all(serialize = "camelCase"))]
pub struct HideTitleBarEffectConfig {
  pub enabled: bool,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default, rename_all(serialize = "camelCase"))]
pub struct CornerEffectConfig {
  pub enabled: bool,
  pub style: CornerStyle,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CornerStyle {
  #[default]
  Default,
  Square,
  Rounded,
  SmallRounded,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default, rename_all(serialize = "camelCase"))]
pub struct TransparencyEffectConfig {
  pub enabled: bool,
  pub opacity: OpacityValue,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all(serialize = "camelCase"))]
pub struct WindowRuleConfig {
  pub commands: Vec<InvokeCommand>,
  #[serde(rename = "match")]
  pub match_window: Vec<WindowMatchConfig>,
  #[serde(default = "default_window_rule_on")]
  pub on: Vec<WindowRuleEvent>,
  #[serde(default = "default_bool::<true>")]
  pub run_once: bool,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(default, rename_all(serialize = "camelCase"))]
pub struct WindowMatchConfig {
  pub window_process: Option<MatchType>,
  pub window_class: Option<MatchType>,
  pub window_title: Option<MatchType>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(untagged)]
pub enum MatchType {
  Equals { equals: String },
  Includes { includes: String },
  Regex { regex: String },
  NotEquals { not_equals: String },
  NotRegex { not_regex: String },
}

impl MatchType {
  pub fn is_match(&self, value: &str) -> bool {
    match self {
      MatchType::Equals { equals } => value == equals,
      MatchType::Includes { includes } => value.contains(includes),
      MatchType::Regex { regex } => regex::Regex::new(regex)
        .map(|re| re.is_match(value))
        .unwrap_or(false),
      MatchType::NotEquals { not_equals } => value != not_equals,
      MatchType::NotRegex { not_regex } => regex::Regex::new(not_regex)
        .map(|re| !re.is_match(value))
        .unwrap_or(false),
    }
  }
}

#[derive(Clone, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WindowRuleEvent {
  Focus,
  Manage,
  TitleChange,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all(serialize = "camelCase"))]
pub struct WorkspaceConfig {
  pub name: String,
  #[serde(default)]
  pub display_name: Option<String>,
  #[serde(default)]
  pub bind_to_monitor: Option<u32>,
  #[serde(default = "default_bool::<false>")]
  pub keep_alive: bool,
}

const fn default_bool<const V: bool>() -> bool { V }
fn default_window_rule_on() -> Vec<WindowRuleEvent> {
  vec![WindowRuleEvent::Manage, WindowRuleEvent::TitleChange]
}
