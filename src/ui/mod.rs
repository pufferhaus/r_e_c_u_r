//! In-app screen system.
//!
//! Screens are a stack: while the stack is non-empty, actions are routed to
//! the top screen. Each screen owns its local UI state (cursor, scroll). The
//! currently-visible grid is rendered into a `TextGrid` on each frame.

use crate::action::Action;
use crate::state::SharedState;
use crate::status::TextGrid;

/// Result returned by a screen after handling an action.
pub enum ScreenResult {
    /// Stay open; render again on the next frame.
    Continue,
    /// Close this screen (pop one level). If the stack becomes empty, normal
    /// status compose is restored.
    Pop,
    /// Push a new screen on top. The old screen is preserved underneath and
    /// becomes active again once the new one pops.
    Push(Box<dyn Screen>),
    /// Synthesise an action for the apply loop. Use this when a screen wants to
    /// produce an Action without directly mutating state (so the central
    /// apply.rs path still gets to enforce invariants).
    Action(crate::action::Action),
}

/// A menu screen that can paint itself into a `TextGrid` and respond to
/// `Action` events.
pub trait Screen: Send {
    /// Paint the screen into `grid`. The grid has already been cleared.
    fn render(&self, state: &SharedState, grid: &mut TextGrid);
    /// Handle an action. Returns how the screen stack should respond.
    fn handle(&mut self, action: Action, state: &mut SharedState) -> ScreenResult;
}

/// A stack of `Screen` objects. The top screen owns input; lower screens are
/// preserved in order.
#[derive(Default)]
pub struct ScreenStack {
    stack: Vec<Box<dyn Screen>>,
}

impl ScreenStack {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_open(&self) -> bool {
        !self.stack.is_empty()
    }

    pub fn depth(&self) -> usize {
        self.stack.len()
    }

    pub fn push(&mut self, screen: Box<dyn Screen>) {
        self.stack.push(screen);
    }

    /// Close all screens (e.g. on global panic key).
    pub fn close_all(&mut self) {
        self.stack.clear();
    }

    /// Read-only view of the top screen, if any.
    pub fn top(&self) -> Option<&dyn Screen> {
        self.stack.last().map(|s| s.as_ref())
    }

    /// Dispatch an action to the top screen and apply the resulting
    /// `ScreenResult`. Returns `Some(synth_action)` if the screen wants the
    /// apply loop to run another action; `None` otherwise (or if no screen is open).
    pub fn dispatch(&mut self, action: Action, state: &mut SharedState) -> Option<Action> {
        if self.stack.is_empty() {
            return None;
        }
        let top = self.stack.last_mut()?;
        let result = top.handle(action, state);
        match result {
            ScreenResult::Continue => None,
            ScreenResult::Pop => {
                self.stack.pop();
                None
            }
            ScreenResult::Push(s) => {
                self.stack.push(s);
                None
            }
            ScreenResult::Action(a) => Some(a),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::action::Action;
    use crate::state::SharedState;

    struct Dummy {
        pops_after: u32,
        count: u32,
    }

    impl Screen for Dummy {
        fn render(&self, _state: &SharedState, grid: &mut TextGrid) {
            grid.write_row(0, "DUMMY");
        }
        fn handle(&mut self, _action: Action, _state: &mut SharedState) -> ScreenResult {
            self.count += 1;
            if self.count >= self.pops_after {
                ScreenResult::Pop
            } else {
                ScreenResult::Continue
            }
        }
    }

    #[test]
    fn empty_stack_not_open() {
        let s = ScreenStack::new();
        assert!(!s.is_open());
        assert_eq!(s.depth(), 0);
        assert!(s.top().is_none());
    }

    #[test]
    fn dispatch_on_empty_returns_none() {
        let mut s = ScreenStack::new();
        let mut state = SharedState::new();
        assert_eq!(s.dispatch(Action::Back, &mut state), None);
    }

    #[test]
    fn push_makes_stack_open() {
        let mut s = ScreenStack::new();
        s.push(Box::new(Dummy {
            pops_after: 99,
            count: 0,
        }));
        assert!(s.is_open());
        assert_eq!(s.depth(), 1);
    }

    #[test]
    fn pop_result_closes_screen() {
        let mut s = ScreenStack::new();
        s.push(Box::new(Dummy {
            pops_after: 1,
            count: 0,
        }));
        let mut state = SharedState::new();
        let synth = s.dispatch(Action::Back, &mut state);
        assert_eq!(synth, None);
        assert!(!s.is_open());
    }

    #[test]
    fn push_result_adds_to_stack() {
        struct Pusher;
        impl Screen for Pusher {
            fn render(&self, _: &SharedState, _: &mut TextGrid) {}
            fn handle(&mut self, _: Action, _: &mut SharedState) -> ScreenResult {
                ScreenResult::Push(Box::new(Dummy {
                    pops_after: 99,
                    count: 0,
                }))
            }
        }
        let mut s = ScreenStack::new();
        s.push(Box::new(Pusher));
        let mut state = SharedState::new();
        s.dispatch(Action::Enter, &mut state);
        assert_eq!(s.depth(), 2);
    }

    #[test]
    fn close_all_empties_stack() {
        let mut s = ScreenStack::new();
        s.push(Box::new(Dummy {
            pops_after: 99,
            count: 0,
        }));
        s.push(Box::new(Dummy {
            pops_after: 99,
            count: 0,
        }));
        s.close_all();
        assert!(!s.is_open());
    }
}
