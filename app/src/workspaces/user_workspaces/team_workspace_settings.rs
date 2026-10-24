//! Team-scoped reads of workspace settings, plus the [`TeamScope`] types that name which team a
//! read is for.
//!
//! Two layers gate what a member may do, at different granularities:
//! - Plan entitlements are **workspace-level**: billing metadata is workspace-owned, so they hold
//!   regardless of which team a window has selected, and read `current_workspace()` (e.g.
//!   [`UserWorkspaces::is_managed_byok_byoe_enabled`]).
//! - Admin policies are **team-scoped**: they narrow an entitlement for one team, so they take a
//!   [`TeamScope`] and read that team (e.g. [`UserWorkspaces::team_byo_for_scope`]).
//!
//! A team-scoped policy only bites once its workspace-level entitlement turns it on -- a plan that
//! does not manage credentials centrally has no `team_byo` to enforce, so members fall back to the
//! plan's own BYO entitlement.

use std::rc::Rc;

use torchui::{AppContext, Entity, SingletonEntity, ViewContext, WeakViewHandle, WindowId};

use super::UserWorkspaces;
use crate::server::ids::ServerId;

mod sealed {
    pub trait Sealed {}
}

/// Reads a [`TeamContextForOperation`] or [`TeamContext`]'s team.
///
/// Application code obtains a [`TeamContext`] or [`TeamContextForOperation`] from a view-bound
/// context, handle, or window. Neither type can be copied or cloned. `TeamContext` is borrow-bound
/// to an immediate read, while `TeamContextForOperation` is owned so one operation can move it
/// across an asynchronous boundary without re-resolving against a different window team.
///
/// Sealed: only this module implements [`sealed::Sealed`], so a scope can never be minted
/// outside [`UserWorkspaces`].
#[allow(private_bounds)]
pub trait TeamScope: sealed::Sealed {
    fn team_uid(&self) -> Option<ServerId>;
}

/// The team selected when a view-scoped operation starts.
pub struct TeamContextForOperation {
    team_uid: Option<ServerId>,
}

impl sealed::Sealed for TeamContextForOperation {}

impl TeamScope for TeamContextForOperation {
    fn team_uid(&self) -> Option<ServerId> {
        self.team_uid
    }
}

#[cfg(test)]
impl TeamContextForOperation {}

/// The team a view renders as, borrowed for the duration of a single read.
///
/// It is resolved at the point of use so policy reads follow the view between windows.
pub struct TeamContext<'a> {
    team_uid: Option<&'a ServerId>,
}

impl sealed::Sealed for TeamContext<'_> {}

impl TeamScope for TeamContext<'_> {
    fn team_uid(&self) -> Option<ServerId> {
        self.team_uid.copied()
    }
}

/// Resolves a [`TeamContext`] on demand from a view captured up front. See
/// [`UserWorkspaces::team_context_resolver`].
pub type TeamContextResolver = Rc<dyn for<'a> Fn(&'a AppContext) -> TeamContext<'a>>;

impl UserWorkspaces {
    /// Captures the team selected in `ctx`'s window as an operation's
    /// [`TeamContextForOperation`]. Always succeeds -- a window with no team selected still yields
    /// a scope whose `team_uid()` is `None`.
    pub fn team_context_for_operation<T: Entity>(
        &self,
        ctx: &ViewContext<T>,
    ) -> TeamContextForOperation {
        self.team_context_for_window_operation(ctx.window_id())
    }
    /// Captures the team selected in a headless frontend's window.
    pub fn team_context_for_window_operation(
        &self,
        window_id: WindowId,
    ) -> TeamContextForOperation {
        TeamContextForOperation {
            team_uid: self.team_uid_for_window(window_id),
        }
    }

    pub(crate) fn team_context<'a, T: Entity>(
        &'a self,
        view: &WeakViewHandle<T>,
        app: &AppContext,
    ) -> TeamContext<'a> {
        let team_uid = self.team_for_view_handle(view, app).map(|team| &team.uid);
        TeamContext { team_uid }
    }

    /// Captures `view` as a reusable source of [`TeamContext`], for consumers that cannot name
    /// a view at the boundaries where they need one.
    pub fn team_context_resolver<T: Entity>(view: WeakViewHandle<T>) -> TeamContextResolver {
        Rc::new(move |app| Self::as_ref(app).team_context(&view, app))
    }

    /// A resolver for tests that build a model without a window to resolve against.
    #[cfg(any(test, feature = "test-util"))]
    pub fn teamless_context_resolver_for_test() -> TeamContextResolver {
        Rc::new(|_| TeamContext { team_uid: None })
    }
    #[cfg(any(test, feature = "test-util"))]
    pub fn teamless_context_for_operation_for_test() -> TeamContextForOperation {
        TeamContextForOperation { team_uid: None }
    }

    fn team_context_for_window_id(&self, window_id: WindowId) -> TeamContext<'_> {
        TeamContext {
            team_uid: self
                .team_uid_for_window(window_id)
                .and_then(|team_uid| self.team_from_uid(team_uid))
                .map(|team| &team.uid),
        }
    }

    pub fn team_context_for_window(&self, window_id: WindowId) -> TeamContext<'_> {
        self.team_context_for_window_id(window_id)
    }
}
