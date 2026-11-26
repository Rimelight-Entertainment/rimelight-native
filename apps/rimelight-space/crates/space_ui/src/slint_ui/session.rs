//! Sign-in and session rules (SA.4; PROJECT.md section 4, slice SA). Plain Rust, always compiled: the text
//! the sign-in screen shows for each problem, and the sign-in messages' wording. The screen is `@rimelight_shell/components/sign-in-screen.slint`,
//! the status indicator is in `@rimelight_shell/components/status-bar.slint`. The desktop app (`apps/desktop/src/session.rs`) runs the client
//! and sets the window's properties from these rules.

/// Why a sign-in did not finish. The desktop app maps the client's errors to these.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SignInProblem {
    /// The sign-in was refused or cancelled in the browser.
    Denied,
    /// The account is not provisioned for Rimelight Space (an admin must add it).
    NotProvisioned,
    /// No callback arrived before the deadline.
    Timeout,
    /// The server could not be reached.
    Unreachable,
    /// The stored session was rejected while the app was running (HTTP 401 on the periodic check).
    SessionEnded,
    /// Any other failure. The text is shown after "Sign-in failed:".
    Other(String),
}

/// The error line the sign-in screen shows under its button.
pub fn sign_in_error_text(problem: &SignInProblem) -> String {
    match problem {
        SignInProblem::Denied => "Sign-in was cancelled.".to_string(),
        SignInProblem::NotProvisioned => {
            "Your account does not have access to Rimelight Space. Ask an admin.".to_string()
        }
        SignInProblem::Timeout => "Sign-in timed out. Try again.".to_string(),
        SignInProblem::Unreachable => {
            "Cannot reach the Rimelight server. Check your connection and try again.".to_string()
        }
        SignInProblem::SessionEnded => "Your session has ended. Sign in again.".to_string(),
        SignInProblem::Other(message) => format!("Sign-in failed: {message}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn not_provisioned_message_is_the_agreed_text() {
        assert_eq!(
            sign_in_error_text(&SignInProblem::NotProvisioned),
            "Your account does not have access to Rimelight Space. Ask an admin."
        );
    }

    #[test]
    fn backend_codes_have_their_messages() {
        assert_eq!(
            sign_in_error_text(&SignInProblem::Denied),
            "Sign-in was cancelled."
        );
        assert_eq!(
            sign_in_error_text(&SignInProblem::NotProvisioned),
            "Your account does not have access to Rimelight Space. Ask an admin."
        );
    }

    #[test]
    fn other_failures_show_their_message() {
        assert_eq!(
            sign_in_error_text(&SignInProblem::Other("not supported yet".into())),
            "Sign-in failed: not supported yet"
        );
    }
}
