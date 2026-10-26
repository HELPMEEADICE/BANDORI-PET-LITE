//! The local behavior state machine (spec §2.7).
//!
//! Everything here is deterministic given a seed and a clock: the engine turns
//! cursor position, clicks and elapsed time into [`Command`]s that the pet
//! process executes. No model inference is involved — the original script used an
//! LLM for chat, LITE only needs the local half.
//!
//! * [`head_track`] — 600 px radial clamp, 4 px dead zone, 0.3 s recenter, plus
//!   mutual gaze over peer window centers.
//! * [`idle`] — 5 s ambient tick, 180 s cooldown, morning/sleep/late-night/daze
//!   buckets, random-motion selection.
//! * [`window_motion`] — the six emotion displacements with OutCubic keyframes.
//! * [`poke`] — double-click feedback (motion + expression + shake + bubble).

pub mod head_track;
pub mod idle;
pub mod poke;
pub mod window_motion;

use pet_config::{ClickAction, ClickProfile, ClickRegion};
use pet_input::matching::{choose_auto_motion, resolve_click_action, resolve_motion_by_tag};
use pet_util::Rng;
use tracing::trace;

pub use head_track::{GazeTarget, HeadTracker};
pub use idle::{AmbientKind, IdlePlanner};
pub use poke::PokeState;
pub use window_motion::{WindowMotionKind, WindowMotionPlayer};

/// An action the pet process should perform.
#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    /// Play a motion group by exact name.
    PlayMotion(String),
    /// Play an expression by exact name.
    PlayExpression(String),
    /// Play any non-idle motion (`__random__`).
    PlayRandomMotion,
    /// Set the head/eye drag target in normalized `-1..=1` coordinates.
    SetDrag(f32, f32),
    /// Move the window to `(x, y)` in physical pixels (emotion displacement).
    MoveWindow(i32, i32),
    /// Show a short bubble above the head.
    Bubble(String),
}

/// Everything the behavior machine needs to know this tick.
#[derive(Debug, Clone)]
pub struct BehaviorContext<'a> {
    /// Seconds since the pet process started (monotonic).
    pub now: f64,
    /// Frame delta in seconds.
    pub dt: f32,
    /// Cursor position in this window's physical pixel space, if it is nearby.
    pub cursor_local: Option<(f32, f32)>,
    /// Window size in physical pixels.
    pub surface: (u32, u32),
    /// Seconds since the last user interaction with this role.
    pub idle_seconds: f64,
    /// Local hour (0–23) for the time-of-day buckets.
    pub hour: u32,
    /// Nearest other role's window center in screen coordinates.
    pub peer_center: Option<(f32, f32)>,
    /// This window's center in screen coordinates.
    pub own_center: (f32, f32),
    /// Motion group names of the current model.
    pub motion_names: &'a [String],
    /// Expression names of the current model.
    pub expression_names: &'a [String],
    /// Character key, used by the `<char>_<tag>` matching level.
    pub character: &'a str,
}

/// Config subset that drives behavior (copied from `Config` on change).
#[derive(Debug, Clone, Default)]
pub struct RoleBehaviorConfig {
    /// Dragging disabled.
    pub drag_locked: bool,
    /// Look at the cursor.
    pub head_tracking: bool,
    /// Look at the nearest other role.
    pub mutual_gaze: bool,
    /// Play idle motions.
    pub idle_actions: bool,
    /// Play periodic random motions.
    pub random_actions: bool,
    /// Window-level emotion displacement.
    pub emotion_enabled: bool,
    /// Seven-region click mapping.
    pub click_actions: ClickProfile,
    /// Poke motion (empty = follow the head region).
    pub poke_motion: String,
    /// Poke expression (empty = follow the head region).
    pub poke_expression: String,
    /// Default motion group (empty = model default idle).
    pub default_motion: String,
    /// Default expression (empty = none).
    pub default_expression: String,
}

impl From<&pet_config::Config> for RoleBehaviorConfig {
    fn from(config: &pet_config::Config) -> Self {
        Self {
            drag_locked: config.drag_locked,
            head_tracking: config.live2d_head_tracking_enabled,
            mutual_gaze: config.live2d_mutual_gaze_enabled,
            idle_actions: config.live2d_idle_actions_enabled,
            random_actions: config.live2d_random_actions_enabled,
            emotion_enabled: config.emotion_behavior_enabled,
            click_actions: config.click_motion_actions.clone(),
            poke_motion: config.poke_motion.clone(),
            poke_expression: config.poke_expression.clone(),
            default_motion: config
                .active_entry()
                .map(|entry| entry.default_motion.clone())
                .unwrap_or_default(),
            default_expression: config
                .active_entry()
                .map(|entry| entry.default_expression.clone())
                .unwrap_or_default(),
        }
    }
}

/// Timers created by click feedback (spec §3.3: 9000 / 3200 / 5000 ms).
#[derive(Debug, Clone, Default)]
struct FeedbackTimers {
    clear_motion_at: Option<f64>,
    restore_default_at: Option<f64>,
    revert_expression_at: Option<f64>,
}

/// The per-role behavior machine.
pub struct RoleBehavior {
    config: RoleBehaviorConfig,
    rng: Rng,
    head: HeadTracker,
    idle: IdlePlanner,
    window_motion: WindowMotionPlayer,
    poke: PokeState,
    timers: FeedbackTimers,
    last_interaction: f64,
    started: bool,
    default_motion_playing: bool,
}

impl RoleBehavior {
    /// Creates a behavior machine seeded for reproducible tests.
    pub fn new(seed: u64) -> Self {
        Self {
            config: RoleBehaviorConfig::default(),
            rng: Rng::seeded(seed),
            head: HeadTracker::default(),
            idle: IdlePlanner::default(),
            window_motion: WindowMotionPlayer::default(),
            poke: PokeState::default(),
            timers: FeedbackTimers::default(),
            last_interaction: 0.0,
            started: false,
            default_motion_playing: false,
        }
    }

    /// Replaces the behavior config (called whenever settings change).
    pub fn set_config(&mut self, config: RoleBehaviorConfig) {
        if self.config.head_tracking && !config.head_tracking {
            self.head.recenter_immediately();
        }
        self.config = config;
    }

    /// Current config.
    pub fn config(&self) -> &RoleBehaviorConfig {
        &self.config
    }

    /// Whether a window displacement animation is currently running.
    pub fn window_motion_active(&self) -> bool {
        self.window_motion.is_running()
    }

    /// Called when the user interacts with this role (drag, click, wheel…).
    pub fn note_interaction(&mut self, now: f64) {
        self.last_interaction = now;
        self.idle.mark_activity(now);
    }

    /// First frame: play the default motion/expression.
    pub fn start(&mut self, now: f64) -> Vec<Command> {
        self.started = true;
        self.last_interaction = now;
        let mut commands = Vec::new();
        if !self.config.default_motion.is_empty() {
            commands.push(Command::PlayMotion(self.config.default_motion.clone()));
            self.default_motion_playing = true;
        } else if self.config.idle_actions {
            commands.push(Command::PlayMotion(String::new()));
        }
        if !self.config.default_expression.is_empty() {
            commands.push(Command::PlayExpression(
                self.config.default_expression.clone(),
            ));
        }
        commands
    }

    /// Handles a single click on one of the seven regions.
    pub fn on_click(
        &mut self,
        region: ClickRegion,
        now: f64,
        ctx: &BehaviorContext<'_>,
    ) -> Vec<Command> {
        self.note_interaction(now);
        let entry = ctx
            .motion_names
            .iter()
            .position(|_| false)
            .unwrap_or(usize::MAX);
        let _ = entry;

        let action = self
            .config
            .click_actions
            .get(region.key())
            .cloned()
            .unwrap_or_else(ClickAction::auto);
        let resolved = resolve_click_action(
            region,
            &action,
            ctx.motion_names,
            ctx.expression_names,
            ctx.character,
            &mut self.rng,
        );
        let Some((motion, expression)) = resolved else {
            trace!(region = region.key(), "click region configured to do nothing");
            return Vec::new();
        };

        let mut commands = Vec::new();
        if motion.is_empty() {
            commands.push(Command::PlayRandomMotion);
        } else {
            commands.push(Command::PlayMotion(motion));
        }
        if !expression.is_empty() {
            commands.push(Command::PlayExpression(expression));
            self.timers.revert_expression_at = Some(now + 5.0);
        }
        self.timers.clear_motion_at = Some(now + 9.0);
        self.timers.restore_default_at = Some(now + 3.2);
        commands
    }

    /// Handles a double click (poke).
    pub fn on_double_click(
        &mut self,
        region: ClickRegion,
        now: f64,
        ctx: &BehaviorContext<'_>,
    ) -> Vec<Command> {
        self.note_interaction(now);
        let mut commands = Vec::new();

        let motion = if !self.config.poke_motion.is_empty() {
            self.config.poke_motion.clone()
        } else {
            self.head_region_motion(region, ctx)
        };
        if !motion.is_empty() {
            commands.push(Command::PlayMotion(motion));
        }

        let expression = if !self.config.poke_expression.is_empty() {
            self.config.poke_expression.clone()
        } else {
            self.config
                .click_actions
                .get(ClickRegion::Head.key())
                .map(|action| action.expression.clone())
                .unwrap_or_default()
        };
        if !expression.is_empty() {
            commands.push(Command::PlayExpression(expression));
            self.timers.revert_expression_at = Some(now + 5.0);
        }

        if self.config.emotion_enabled {
            self.window_motion.start(
                WindowMotionKind::Shake,
                PokeState::DOUBLE_CLICK_SHAKE,
                now,
                (0.0, -1.0),
            );
        }
        self.poke.show_bubble(now);
        commands.push(Command::Bubble(localized_poke_text()));
        commands
    }

    fn head_region_motion(&self, region: ClickRegion, ctx: &BehaviorContext<'_>) -> String {
        let action = self
            .config
            .click_actions
            .get(region.key())
            .cloned()
            .unwrap_or_else(ClickAction::auto);
        let motion = match action.motion.as_str() {
            ClickAction::NONE | ClickAction::AUTO => choose_auto_motion(
                ClickRegion::Head,
                ctx.motion_names,
                ctx.character,
                &mut Rng::seeded(1),
            ),
            ClickAction::RANDOM => String::new(),
            other => other.to_owned(),
        };
        if motion.is_empty() {
            choose_auto_motion(
                ClickRegion::Head,
                ctx.motion_names,
                ctx.character,
                &mut Rng::seeded(2),
            )
        } else {
            motion
        }
    }

    /// Per-frame tick: head tracking, ambient motions, window displacement.
    pub fn tick(&mut self, ctx: &BehaviorContext<'_>, base_position: (i32, i32)) -> Vec<Command> {
        let mut commands = Vec::new();

        // 1. Head tracking / mutual gaze.
        if let Some((x, y)) = self.compute_drag(ctx) {
            commands.push(Command::SetDrag(x, y));
        }

        // 2. Feedback timers.
        if let Some(at) = self.timers.revert_expression_at {
            if ctx.now >= at {
                self.timers.revert_expression_at = None;
                if self.config.default_expression.is_empty() {
                    commands.push(Command::PlayExpression(String::new()));
                } else {
                    commands.push(Command::PlayExpression(self.config.default_expression.clone()));
                }
            }
        }
        if let Some(at) = self.timers.clear_motion_at {
            if ctx.now >= at {
                self.timers.clear_motion_at = None;
                commands.push(Command::PlayMotion(String::new()));
                self.default_motion_playing = true;
            }
        } else if let Some(at) = self.timers.restore_default_at {
            if ctx.now >= at {
                self.timers.restore_default_at = None;
                if !self.config.default_motion.is_empty() {
                    commands.push(Command::PlayMotion(self.config.default_motion.clone()));
                }
            }
        }

        // 3. Ambient random / idle motions.
        if self.config.random_actions || self.config.idle_actions {
            if let Some(plan) = self.idle.tick(
                ctx,
                &mut self.rng,
                self.config.random_actions,
                self.config.idle_actions,
            ) {
                if let Some(motion) = plan.motion {
                    commands.push(Command::PlayMotion(motion));
                }
                if let Some(expression) = plan.expression {
                    commands.push(Command::PlayExpression(expression));
                    self.timers.revert_expression_at = Some(ctx.now + 6.0);
                }
                self.timers.clear_motion_at = Some(ctx.now + 9.0);
                self.timers.restore_default_at = Some(ctx.now + 1.8);
            }
        }

        // 4. Window displacement animation.
        if self.config.emotion_enabled {
            if let Some(offset) = self.window_motion.tick(ctx.now) {
                commands.push(Command::MoveWindow(
                    base_position.0 + offset.0,
                    base_position.1 + offset.1,
                ));
            } else if self.window_motion.just_finished() {
                commands.push(Command::MoveWindow(base_position.0, base_position.1));
            }
        }

        // 5. Bubble expiry.
        if self.poke.bubble_expired(ctx.now) {
            trace!("poke bubble expired");
        }

        commands
    }

    /// Plays a random non-idle motion (radial menu "动作").
    pub fn random_motion(&mut self, ctx: &BehaviorContext<'_>, now: f64) -> Option<Command> {
        self.note_interaction(now);
        let motion = choose_auto_motion(
            ClickRegion::UpperBodyCenter,
            ctx.motion_names,
            ctx.character,
            &mut self.rng,
        );
        if motion.is_empty() {
            None
        } else {
            self.timers.clear_motion_at = Some(now + 8.0);
            Some(Command::PlayMotion(motion))
        }
    }

    /// Triggers a window displacement (used by the tray / tests).
    pub fn trigger_window_motion(&mut self, kind: WindowMotionKind, intensity: u8, now: f64) {
        if self.config.emotion_enabled {
            self.window_motion.start(kind, intensity, now, (0.0, -1.0));
        }
    }

    fn compute_drag(&mut self, ctx: &BehaviorContext<'_>) -> Option<(f32, f32)> {
        if self.config.mutual_gaze {
            if let Some(peer) = ctx.peer_center {
                let target = GazeTarget::Screen(peer);
                return self.head.gaze(ctx, target, ctx.own_center);
            }
        }
        if self.config.head_tracking {
            let cursor = ctx.cursor_local?;
            return self.head.track_local(ctx, cursor);
        }
        if !self.config.head_tracking {
            return self.head.recenter(ctx.dt);
        }
        None
    }

    /// Seconds since the last user interaction with this role.
    pub fn idle_seconds(&self, now: f64) -> f64 {
        (now - self.last_interaction).max(0.0)
    }
}

/// Poke bubble text (zh_CN copy from the original client).
pub fn localized_poke_text() -> String {
    "戳了你一下".to_owned()
}

/// Resolves a stored click action's motion against the current model (settings preview).
pub fn preview_click_motion(
    action: &ClickAction,
    region: ClickRegion,
    motion_names: &[String],
    character: &str,
    rng: &mut Rng,
) -> String {
    match action.motion.as_str() {
        ClickAction::NONE => String::new(),
        ClickAction::AUTO => choose_auto_motion(region, motion_names, character, rng),
        ClickAction::RANDOM => rng.pick(motion_names).cloned().unwrap_or_default(),
        other => resolve_motion_by_tag(other, motion_names, character, rng),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pet_config::ClickAction;

    fn ctx<'a>(now: f64, motions: &'a [String], expressions: &'a [String]) -> BehaviorContext<'a> {
        BehaviorContext {
            now,
            dt: 1.0 / 60.0,
            cursor_local: None,
            surface: (400, 500),
            idle_seconds: 0.0,
            hour: 12,
            peer_center: None,
            own_center: (200.0, 250.0),
            motion_names: motions,
            expression_names: expressions,
            character: "kasumi",
        }
    }

    #[test]
    fn click_plays_configured_motion_and_expression() {
        let motions = vec!["smile01".to_owned(), "idle01".to_owned()];
        let expressions = vec!["f01".to_owned()];
        let mut behavior = RoleBehavior::new(7);
        let mut config = RoleBehaviorConfig::default();
        config.click_actions.insert(
            "head".to_owned(),
            ClickAction::new("smile01", "f01"),
        );
        behavior.set_config(config);

        let commands = behavior.on_click(ClickRegion::Head, 1.0, &ctx(1.0, &motions, &expressions));
        assert!(commands.contains(&Command::PlayMotion("smile01".to_owned())));
        assert!(commands.contains(&Command::PlayExpression("f01".to_owned())));
    }

    #[test]
    fn none_action_emits_nothing() {
        let motions = vec!["smile01".to_owned()];
        let expressions = vec!["f01".to_owned()];
        let mut behavior = RoleBehavior::new(8);
        let mut config = RoleBehaviorConfig::default();
        config
            .click_actions
            .insert("head".to_owned(), ClickAction::new(ClickAction::NONE, ""));
        behavior.set_config(config);
        let commands = behavior.on_click(ClickRegion::Head, 1.0, &ctx(1.0, &motions, &expressions));
        assert!(commands.is_empty(), "{commands:?}");
    }

    #[test]
    fn double_click_pokes_with_shake_and_bubble() {
        let motions = vec!["smile01".to_owned()];
        let expressions = vec!["f01".to_owned()];
        let mut behavior = RoleBehavior::new(9);
        behavior.set_config(RoleBehaviorConfig {
            emotion_enabled: true,
            ..RoleBehaviorConfig::default()
        });
        let commands = behavior.on_double_click(ClickRegion::Head, 2.0, &ctx(2.0, &motions, &expressions));
        assert!(commands.contains(&Command::Bubble("戳了你一下".to_owned())));
        assert!(behavior.window_motion_active());
    }

    #[test]
    fn motion_is_cleared_after_the_feedback_window() {
        let motions = vec!["smile01".to_owned(), "idle01".to_owned()];
        let expressions = vec![String::new()];
        let mut behavior = RoleBehavior::new(10);
        let mut config = RoleBehaviorConfig::default();
        config
            .click_actions
            .insert("head".to_owned(), ClickAction::new("smile01", ""));
        behavior.set_config(config);
        behavior.on_click(ClickRegion::Head, 0.0, &ctx(0.0, &motions, &expressions));
        let commands = behavior.tick(&ctx(9.5, &motions, &expressions), (0, 0));
        assert!(commands.contains(&Command::PlayMotion(String::new())));
    }

    #[test]
    fn head_tracking_emits_drag() {
        let motions = vec!["idle01".to_owned()];
        let expressions = vec!["f01".to_owned()];
        let mut behavior = RoleBehavior::new(11);
        behavior.set_config(RoleBehaviorConfig {
            head_tracking: true,
            ..RoleBehaviorConfig::default()
        });
        let mut context = ctx(1.0, &motions, &expressions);
        context.cursor_local = Some((380.0, 40.0));
        let commands = behavior.tick(&context, (0, 0));
        assert!(
            commands.iter().any(|command| matches!(command, Command::SetDrag(_, _))),
            "{commands:?}"
        );
    }
}


