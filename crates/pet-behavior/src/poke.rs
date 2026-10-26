//! Double-click poke feedback (spec §2.5): shake + bubble for 1980 ms.
//!
//! Only the timing state lives here; the bubble is rendered by the pet process as
//! a small layered window.

/// Poke bubble / shake state.
#[derive(Debug, Clone, Default)]
pub struct PokeState {
    bubble_until: Option<f64>,
    bubbles_shown: u64,
}

impl PokeState {
    /// Bubble lifetime in milliseconds (spec: 1980 ms).
    pub const BADGE_MS: f64 = 1980.0;
    /// Shake intensity used by the double-click feedback.
    pub const DOUBLE_CLICK_SHAKE: u8 = 72;
    /// Shake intensity used for remote "someone poked you" events.
    pub const SHAKE_INTENSITY: u8 = 36;
    /// Maximum bubble text length before trimming.
    pub const MAX_TEXT_CHARS: usize = 32;

    /// Marks the bubble as visible from `now`.
    pub fn show_bubble(&mut self, now: f64) {
        self.bubble_until = Some(now + Self::BADGE_MS / 1000.0);
        self.bubbles_shown += 1;
    }

    /// `true` while the bubble should be drawn.
    pub fn bubble_visible(&self, now: f64) -> bool {
        self.bubble_until.is_some_and(|until| now < until)
    }

    /// `true` once, when the bubble just expired.
    pub fn bubble_expired(&mut self, now: f64) -> bool {
        match self.bubble_until {
            Some(until) if now >= until => {
                self.bubble_until = None;
                true
            }
            _ => false,
        }
    }

    /// Number of bubbles shown so far (diagnostics / tests).
    pub fn count(&self) -> u64 {
        self.bubbles_shown
    }

    /// Trims bubble text to 29 characters plus an ellipsis, like the original.
    pub fn trim_text(text: &str) -> String {
        let collapsed = text.split_whitespace().collect::<Vec<_>>().join(" ");
        let count = collapsed.chars().count();
        if count <= Self::MAX_TEXT_CHARS {
            return collapsed;
        }
        let mut trimmed: String = collapsed.chars().take(29).collect();
        while trimmed.ends_with(' ') {
            trimmed.pop();
        }
        trimmed.push_str("...");
        trimmed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bubble_lasts_about_two_seconds() {
        let mut state = PokeState::default();
        state.show_bubble(10.0);
        assert!(state.bubble_visible(11.9));
        assert!(!state.bubble_visible(12.0));
        assert!(state.bubble_expired(12.0));
        assert!(!state.bubble_expired(12.5));
        assert_eq!(state.count(), 1);
    }

    #[test]
    fn text_trimming_matches_the_original() {
        assert_eq!(PokeState::trim_text("  戳了你一下  "), "戳了你一下");
        let long = "a".repeat(40);
        let trimmed = PokeState::trim_text(&long);
        assert_eq!(trimmed.chars().count(), 32);
        assert!(trimmed.ends_with("..."));
    }
}
