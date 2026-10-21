//! Native platform adapter for explicit dictation insertion.
//!
//! The only currently implemented adapter targets macOS. Other targets expose
//! the same type with explicit errors so Linux and Windows CI prove that the
//! core contracts compile without pretending insertion is available.

pub mod contract;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(not(target_os = "macos"))]
mod unsupported;

#[cfg(target_os = "macos")]
pub use macos::MacPlatform;
#[cfg(not(target_os = "macos"))]
pub use unsupported::MacPlatform;

#[cfg(test)]
mod tests {
    #[cfg(not(target_os = "macos"))]
    #[test]
    fn unsupported_platform_reports_an_explicit_error_instead_of_a_capability() {
        use crate::dictation::{PermissionKind, Platform};

        let platform = super::MacPlatform::new_for_test();
        assert!(platform.permissions().is_err());
        assert!(platform
            .request_permission(PermissionKind::Accessibility)
            .is_err());
        assert!(platform.capture_target().is_err());
        assert!(platform
            .insert("target", "request", "transcript", true)
            .is_err());
        platform.release_target("target");
    }
}
