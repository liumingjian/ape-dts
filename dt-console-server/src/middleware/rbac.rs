//! RBAC middleware: per-endpoint role checks via the action matrix.
//!
//! Role matrix (per ADR-0003):
//! - admin   → all actions
//! - operator → task.create, task.read, task.start, task.stop, alert.clear, license.read
//! - viewer  → task.read, license.read
//!
//! Every denied (role, action) cell returns 403 `{ code: "FORBIDDEN", details: { required_action: "…" } }`.
//! Anonymous requests to protected endpoints receive 401 UNAUTHENTICATED (not 403).
//! The `require_action` function is the single enforcement point used by all handlers.

use crate::error::{codes, ApiError};
use crate::models::UserContext;

/// Well-known RBAC actions. Each maps to a permission in the role matrix.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RbacAction {
    // Task actions
    TaskCreate,
    TaskRead,
    TaskUpdate,
    TaskDelete,
    TaskStart,
    TaskStop,

    // User management
    UsersList,
    UsersCreate,
    UsersRead,
    UsersUpdate,
    UsersDelete,

    // License
    LicenseRead,
    LicenseActivate,

    // Alerts
    AlertClear,

    // Audit
    OperateLogsList,
    ControlLogsList,

    // Resource groups
    ResourceGroupCreate,
    ResourceGroupRead,
    ResourceGroupUpdate,
    ResourceGroupDelete,

    // Alert rules
    AlertRuleCreate,
    AlertRuleRead,
    AlertRuleUpdate,
    AlertRuleDelete,

    // Alarm channels
    AlarmChannelCreate,
    AlarmChannelRead,
    AlarmChannelUpdate,
    AlarmChannelDelete,

    // Alarm templates
    AlarmTemplateCreate,
    AlarmTemplateRead,
    AlarmTemplateUpdate,
    AlarmTemplateDelete,

    // System hosts
    SystemHostRead,

    // Global params
    GlobalParamRead,
    GlobalParamUpdate,
}

impl std::fmt::Display for RbacAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            RbacAction::TaskCreate => "task.create",
            RbacAction::TaskRead => "task.read",
            RbacAction::TaskUpdate => "task.update",
            RbacAction::TaskDelete => "task.delete",
            RbacAction::TaskStart => "task.start",
            RbacAction::TaskStop => "task.stop",
            RbacAction::UsersList => "users.list",
            RbacAction::UsersCreate => "users.create",
            RbacAction::UsersRead => "users.read",
            RbacAction::UsersUpdate => "users.update",
            RbacAction::UsersDelete => "users.delete",
            RbacAction::LicenseRead => "license.read",
            RbacAction::LicenseActivate => "license.activate",
            RbacAction::AlertClear => "alert.clear",
            RbacAction::OperateLogsList => "operate_logs.list",
            RbacAction::ControlLogsList => "control_logs.list",
            RbacAction::ResourceGroupCreate => "resource_group.create",
            RbacAction::ResourceGroupRead => "resource_group.read",
            RbacAction::ResourceGroupUpdate => "resource_group.update",
            RbacAction::ResourceGroupDelete => "resource_group.delete",
            RbacAction::AlertRuleCreate => "alert_rule.create",
            RbacAction::AlertRuleRead => "alert_rule.read",
            RbacAction::AlertRuleUpdate => "alert_rule.update",
            RbacAction::AlertRuleDelete => "alert_rule.delete",
            RbacAction::AlarmChannelCreate => "alarm_channel.create",
            RbacAction::AlarmChannelRead => "alarm_channel.read",
            RbacAction::AlarmChannelUpdate => "alarm_channel.update",
            RbacAction::AlarmChannelDelete => "alarm_channel.delete",
            RbacAction::AlarmTemplateCreate => "alarm_template.create",
            RbacAction::AlarmTemplateRead => "alarm_template.read",
            RbacAction::AlarmTemplateUpdate => "alarm_template.update",
            RbacAction::AlarmTemplateDelete => "alarm_template.delete",
            RbacAction::SystemHostRead => "system_host.read",
            RbacAction::GlobalParamRead => "global_param.read",
            RbacAction::GlobalParamUpdate => "global_param.update",
        };
        write!(f, "{s}")
    }
}

/// Check whether a role is allowed to perform a given action.
///
/// Returns `true` if allowed, `false` if denied.
pub fn is_allowed(role: &str, action: RbacAction) -> bool {
    match role {
        "admin" => true, // admin can do everything
        "operator" => matches!(
            action,
            RbacAction::TaskCreate
                | RbacAction::TaskRead
                | RbacAction::TaskUpdate
                | RbacAction::TaskStart
                | RbacAction::TaskStop
                | RbacAction::AlertClear
                | RbacAction::LicenseRead
                | RbacAction::ResourceGroupRead
                | RbacAction::AlertRuleRead
                | RbacAction::AlarmChannelRead
                | RbacAction::AlarmTemplateRead
                | RbacAction::SystemHostRead
                | RbacAction::GlobalParamRead
        ),
        "viewer" => matches!(
            action,
            RbacAction::TaskRead
                | RbacAction::LicenseRead
                | RbacAction::ResourceGroupRead
                | RbacAction::AlertRuleRead
                | RbacAction::AlarmChannelRead
                | RbacAction::AlarmTemplateRead
                | RbacAction::SystemHostRead
                | RbacAction::GlobalParamRead
        ),
        _ => false, // unknown role → deny all
    }
}

/// Require that the authenticated user has permission for the given action.
///
/// Returns `Ok(())` if allowed.
/// Returns `Err(ApiError)` with 403 FORBIDDEN and `required_action` in details
/// if the user's role does not permit the action.
///
/// This function does NOT handle the 401 case — that is handled by the
/// `UserContext` extractor, which returns 401 for anonymous requests.
/// Call this in handlers after extracting `UserContext`.
pub fn require_action(user: &UserContext, action: RbacAction) -> Result<(), ApiError> {
    if is_allowed(&user.role, action) {
        Ok(())
    } else {
        Err(ApiError::with_details(
            codes::FORBIDDEN,
            "Insufficient permissions",
            serde_json::json!({ "required_action": action.to_string() }),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn user_context(role: &str) -> UserContext {
        UserContext {
            user_id: "test-id".to_string(),
            username: "testuser".to_string(),
            display_name: "Test User".to_string(),
            role: role.to_string(),
            disabled: false,
            resource_group_id: None,
        }
    }

    // ─── Admin can perform every action ────────────────────────────────

    #[test]
    fn admin_can_perform_every_action() {
        let admin = user_context("admin");
        for action in [
            RbacAction::TaskCreate,
            RbacAction::TaskRead,
            RbacAction::TaskUpdate,
            RbacAction::TaskDelete,
            RbacAction::TaskStart,
            RbacAction::TaskStop,
            RbacAction::UsersList,
            RbacAction::UsersCreate,
            RbacAction::UsersRead,
            RbacAction::UsersUpdate,
            RbacAction::UsersDelete,
            RbacAction::LicenseRead,
            RbacAction::LicenseActivate,
            RbacAction::AlertClear,
            RbacAction::OperateLogsList,
            RbacAction::ControlLogsList,
            RbacAction::ResourceGroupCreate,
            RbacAction::ResourceGroupRead,
            RbacAction::ResourceGroupUpdate,
            RbacAction::ResourceGroupDelete,
            RbacAction::AlertRuleCreate,
            RbacAction::AlertRuleRead,
            RbacAction::AlertRuleUpdate,
            RbacAction::AlertRuleDelete,
            RbacAction::AlarmChannelCreate,
            RbacAction::AlarmChannelRead,
            RbacAction::AlarmChannelUpdate,
            RbacAction::AlarmChannelDelete,
            RbacAction::AlarmTemplateCreate,
            RbacAction::AlarmTemplateRead,
            RbacAction::AlarmTemplateUpdate,
            RbacAction::AlarmTemplateDelete,
            RbacAction::SystemHostRead,
            RbacAction::GlobalParamRead,
            RbacAction::GlobalParamUpdate,
        ] {
            assert!(
                is_allowed("admin", action),
                "admin should be allowed {:?}",
                action
            );
            assert!(
                require_action(&admin, action).is_ok(),
                "admin require_action should succeed for {:?}",
                action
            );
        }
    }

    // ─── Operator can perform their allowed actions ────────────────────

    #[test]
    fn operator_allowed_actions() {
        let allowed = [
            RbacAction::TaskCreate,
            RbacAction::TaskRead,
            RbacAction::TaskUpdate,
            RbacAction::TaskStart,
            RbacAction::TaskStop,
            RbacAction::AlertClear,
            RbacAction::LicenseRead,
            RbacAction::ResourceGroupRead,
            RbacAction::AlertRuleRead,
            RbacAction::AlarmChannelRead,
            RbacAction::AlarmTemplateRead,
            RbacAction::SystemHostRead,
            RbacAction::GlobalParamRead,
        ];
        for action in &allowed {
            assert!(
                is_allowed("operator", *action),
                "operator should be allowed {:?}",
                action
            );
        }
    }

    #[test]
    fn operator_denied_actions() {
        let denied = [
            RbacAction::TaskDelete,
            RbacAction::UsersList,
            RbacAction::UsersCreate,
            RbacAction::UsersRead,
            RbacAction::UsersUpdate,
            RbacAction::UsersDelete,
            RbacAction::LicenseActivate,
            RbacAction::OperateLogsList,
            RbacAction::ControlLogsList,
            RbacAction::ResourceGroupCreate,
            RbacAction::ResourceGroupUpdate,
            RbacAction::ResourceGroupDelete,
            RbacAction::AlertRuleCreate,
            RbacAction::AlertRuleUpdate,
            RbacAction::AlertRuleDelete,
            RbacAction::AlarmChannelCreate,
            RbacAction::AlarmChannelUpdate,
            RbacAction::AlarmChannelDelete,
            RbacAction::AlarmTemplateCreate,
            RbacAction::AlarmTemplateUpdate,
            RbacAction::AlarmTemplateDelete,
            RbacAction::GlobalParamUpdate,
        ];
        let op = user_context("operator");
        for action in &denied {
            assert!(
                !is_allowed("operator", *action),
                "operator should be denied {:?}",
                action
            );
            let result = require_action(&op, *action);
            assert!(
                result.is_err(),
                "operator require_action should fail for {:?}",
                action
            );
            let err = result.unwrap_err();
            assert_eq!(err.code, codes::FORBIDDEN);
            assert!(err.details.is_some());
            let details = err.details.unwrap();
            assert_eq!(details["required_action"], action.to_string());
        }
    }

    // ─── Viewer can only read tasks ────────────────────────────────────

    #[test]
    fn viewer_allowed_actions() {
        let allowed = [
            RbacAction::TaskRead,
            RbacAction::LicenseRead,
            RbacAction::ResourceGroupRead,
            RbacAction::AlertRuleRead,
            RbacAction::AlarmChannelRead,
            RbacAction::AlarmTemplateRead,
            RbacAction::SystemHostRead,
            RbacAction::GlobalParamRead,
        ];
        for action in &allowed {
            assert!(
                is_allowed("viewer", *action),
                "viewer should be allowed {:?}",
                action
            );
        }
    }

    #[test]
    fn viewer_denied_actions() {
        let denied = [
            RbacAction::TaskCreate,
            RbacAction::TaskUpdate,
            RbacAction::TaskDelete,
            RbacAction::TaskStart,
            RbacAction::TaskStop,
            RbacAction::UsersList,
            RbacAction::UsersCreate,
            RbacAction::UsersRead,
            RbacAction::UsersUpdate,
            RbacAction::UsersDelete,
            RbacAction::LicenseActivate,
            RbacAction::AlertClear,
            RbacAction::OperateLogsList,
            RbacAction::ControlLogsList,
            RbacAction::ResourceGroupCreate,
            RbacAction::ResourceGroupUpdate,
            RbacAction::ResourceGroupDelete,
            RbacAction::AlertRuleCreate,
            RbacAction::AlertRuleUpdate,
            RbacAction::AlertRuleDelete,
            RbacAction::AlarmChannelCreate,
            RbacAction::AlarmChannelUpdate,
            RbacAction::AlarmChannelDelete,
            RbacAction::AlarmTemplateCreate,
            RbacAction::AlarmTemplateUpdate,
            RbacAction::AlarmTemplateDelete,
            RbacAction::GlobalParamUpdate,
        ];
        let viewer = user_context("viewer");
        for action in &denied {
            assert!(
                !is_allowed("viewer", *action),
                "viewer should be denied {:?}",
                action
            );
            let result = require_action(&viewer, *action);
            assert!(
                result.is_err(),
                "viewer require_action should fail for {:?}",
                action
            );
            let err = result.unwrap_err();
            assert_eq!(err.code, codes::FORBIDDEN);
            assert!(err.details.is_some());
        }
    }

    // ─── Unknown role is denied everything ──────────────────────────────

    #[test]
    fn unknown_role_denied_all() {
        for action in [
            RbacAction::TaskCreate,
            RbacAction::TaskRead,
            RbacAction::TaskDelete,
            RbacAction::UsersList,
            RbacAction::LicenseActivate,
            RbacAction::AlertClear,
            RbacAction::OperateLogsList,
        ] {
            assert!(
                !is_allowed("guest", action),
                "unknown role should be denied {:?}",
                action
            );
        }
    }

    // ─── All roles can read tasks ──────────────────────────────────────

    #[test]
    fn all_roles_can_read_tasks() {
        for role in &["admin", "operator", "viewer"] {
            assert!(
                is_allowed(role, RbacAction::TaskRead),
                "{role} should be allowed task.read"
            );
        }
    }

    // ─── All roles can read license ────────────────────────────────────

    #[test]
    fn all_roles_can_read_license() {
        for role in &["admin", "operator", "viewer"] {
            assert!(
                is_allowed(role, RbacAction::LicenseRead),
                "{role} should be allowed license.read"
            );
        }
    }

    // ─── All roles can read resource groups ───────────────────────────

    #[test]
    fn all_roles_can_read_resource_groups() {
        for role in &["admin", "operator", "viewer"] {
            assert!(
                is_allowed(role, RbacAction::ResourceGroupRead),
                "{role} should be allowed resource_group.read"
            );
        }
    }

    // ─── Action name serialization ─────────────────────────────────────

    #[test]
    fn action_names_snake_case() {
        assert_eq!(RbacAction::TaskCreate.to_string(), "task.create");
        assert_eq!(RbacAction::TaskRead.to_string(), "task.read");
        assert_eq!(RbacAction::TaskStart.to_string(), "task.start");
        assert_eq!(RbacAction::TaskStop.to_string(), "task.stop");
        assert_eq!(RbacAction::TaskDelete.to_string(), "task.delete");
        assert_eq!(RbacAction::UsersList.to_string(), "users.list");
        assert_eq!(RbacAction::LicenseActivate.to_string(), "license.activate");
        assert_eq!(RbacAction::AlertClear.to_string(), "alert.clear");
        assert_eq!(RbacAction::OperateLogsList.to_string(), "operate_logs.list");
        assert_eq!(
            RbacAction::ResourceGroupCreate.to_string(),
            "resource_group.create"
        );
        assert_eq!(
            RbacAction::ResourceGroupRead.to_string(),
            "resource_group.read"
        );
        assert_eq!(
            RbacAction::ResourceGroupUpdate.to_string(),
            "resource_group.update"
        );
        assert_eq!(
            RbacAction::ResourceGroupDelete.to_string(),
            "resource_group.delete"
        );
    }

    // ─── Full (role, action) matrix test ───────────────────────────────

    #[test]
    fn rbac_matrix_comprehensive() {
        let matrix: &[(&str, RbacAction, bool)] = &[
            // Task actions
            ("admin", RbacAction::TaskCreate, true),
            ("admin", RbacAction::TaskRead, true),
            ("admin", RbacAction::TaskUpdate, true),
            ("admin", RbacAction::TaskDelete, true),
            ("admin", RbacAction::TaskStart, true),
            ("admin", RbacAction::TaskStop, true),
            ("operator", RbacAction::TaskCreate, true),
            ("operator", RbacAction::TaskRead, true),
            ("operator", RbacAction::TaskUpdate, true),
            ("operator", RbacAction::TaskDelete, false),
            ("operator", RbacAction::TaskStart, true),
            ("operator", RbacAction::TaskStop, true),
            ("viewer", RbacAction::TaskCreate, false),
            ("viewer", RbacAction::TaskRead, true),
            ("viewer", RbacAction::TaskUpdate, false),
            ("viewer", RbacAction::TaskDelete, false),
            ("viewer", RbacAction::TaskStart, false),
            ("viewer", RbacAction::TaskStop, false),
            // User management
            ("admin", RbacAction::UsersList, true),
            ("admin", RbacAction::UsersCreate, true),
            ("admin", RbacAction::UsersRead, true),
            ("admin", RbacAction::UsersUpdate, true),
            ("admin", RbacAction::UsersDelete, true),
            ("operator", RbacAction::UsersList, false),
            ("operator", RbacAction::UsersCreate, false),
            ("operator", RbacAction::UsersRead, false),
            ("operator", RbacAction::UsersUpdate, false),
            ("operator", RbacAction::UsersDelete, false),
            ("viewer", RbacAction::UsersList, false),
            ("viewer", RbacAction::UsersCreate, false),
            ("viewer", RbacAction::UsersRead, false),
            ("viewer", RbacAction::UsersUpdate, false),
            ("viewer", RbacAction::UsersDelete, false),
            // License
            ("admin", RbacAction::LicenseRead, true),
            ("admin", RbacAction::LicenseActivate, true),
            ("operator", RbacAction::LicenseRead, true),
            ("operator", RbacAction::LicenseActivate, false),
            ("viewer", RbacAction::LicenseRead, true),
            ("viewer", RbacAction::LicenseActivate, false),
            // Alerts
            ("admin", RbacAction::AlertClear, true),
            ("operator", RbacAction::AlertClear, true),
            ("viewer", RbacAction::AlertClear, false),
            // Audit
            ("admin", RbacAction::OperateLogsList, true),
            ("operator", RbacAction::OperateLogsList, false),
            ("viewer", RbacAction::OperateLogsList, false),
            ("admin", RbacAction::ControlLogsList, true),
            ("operator", RbacAction::ControlLogsList, false),
            ("viewer", RbacAction::ControlLogsList, false),
            // Resource groups
            ("admin", RbacAction::ResourceGroupCreate, true),
            ("admin", RbacAction::ResourceGroupRead, true),
            ("admin", RbacAction::ResourceGroupUpdate, true),
            ("admin", RbacAction::ResourceGroupDelete, true),
            ("operator", RbacAction::ResourceGroupCreate, false),
            ("operator", RbacAction::ResourceGroupRead, true),
            ("operator", RbacAction::ResourceGroupUpdate, false),
            ("operator", RbacAction::ResourceGroupDelete, false),
            ("viewer", RbacAction::ResourceGroupCreate, false),
            ("viewer", RbacAction::ResourceGroupRead, true),
            ("viewer", RbacAction::ResourceGroupUpdate, false),
            ("viewer", RbacAction::ResourceGroupDelete, false),
            // Alert rules
            ("admin", RbacAction::AlertRuleCreate, true),
            ("admin", RbacAction::AlertRuleRead, true),
            ("admin", RbacAction::AlertRuleUpdate, true),
            ("admin", RbacAction::AlertRuleDelete, true),
            ("operator", RbacAction::AlertRuleCreate, false),
            ("operator", RbacAction::AlertRuleRead, true),
            ("operator", RbacAction::AlertRuleUpdate, false),
            ("operator", RbacAction::AlertRuleDelete, false),
            ("viewer", RbacAction::AlertRuleCreate, false),
            ("viewer", RbacAction::AlertRuleRead, true),
            ("viewer", RbacAction::AlertRuleUpdate, false),
            ("viewer", RbacAction::AlertRuleDelete, false),
            // Alarm channels
            ("admin", RbacAction::AlarmChannelCreate, true),
            ("admin", RbacAction::AlarmChannelRead, true),
            ("admin", RbacAction::AlarmChannelUpdate, true),
            ("admin", RbacAction::AlarmChannelDelete, true),
            ("operator", RbacAction::AlarmChannelCreate, false),
            ("operator", RbacAction::AlarmChannelRead, true),
            ("operator", RbacAction::AlarmChannelUpdate, false),
            ("operator", RbacAction::AlarmChannelDelete, false),
            ("viewer", RbacAction::AlarmChannelCreate, false),
            ("viewer", RbacAction::AlarmChannelRead, true),
            ("viewer", RbacAction::AlarmChannelUpdate, false),
            ("viewer", RbacAction::AlarmChannelDelete, false),
            // Alarm templates
            ("admin", RbacAction::AlarmTemplateCreate, true),
            ("admin", RbacAction::AlarmTemplateRead, true),
            ("admin", RbacAction::AlarmTemplateUpdate, true),
            ("admin", RbacAction::AlarmTemplateDelete, true),
            ("operator", RbacAction::AlarmTemplateCreate, false),
            ("operator", RbacAction::AlarmTemplateRead, true),
            ("operator", RbacAction::AlarmTemplateUpdate, false),
            ("operator", RbacAction::AlarmTemplateDelete, false),
            ("viewer", RbacAction::AlarmTemplateCreate, false),
            ("viewer", RbacAction::AlarmTemplateRead, true),
            ("viewer", RbacAction::AlarmTemplateUpdate, false),
            ("viewer", RbacAction::AlarmTemplateDelete, false),
            // System hosts
            ("admin", RbacAction::SystemHostRead, true),
            ("operator", RbacAction::SystemHostRead, true),
            ("viewer", RbacAction::SystemHostRead, true),
            // Global params
            ("admin", RbacAction::GlobalParamRead, true),
            ("admin", RbacAction::GlobalParamUpdate, true),
            ("operator", RbacAction::GlobalParamRead, true),
            ("operator", RbacAction::GlobalParamUpdate, false),
            ("viewer", RbacAction::GlobalParamRead, true),
            ("viewer", RbacAction::GlobalParamUpdate, false),
        ];

        for (role, action, expected) in matrix {
            assert_eq!(
                is_allowed(role, *action),
                *expected,
                "is_allowed({role}, {action:?}) should be {expected}"
            );
        }
    }
}
