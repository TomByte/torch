use std::cmp::Ordering;

use chrono::Utc;
use ordered_float::OrderedFloat;
use serde::{Deserialize, Serialize};
use torch_graphql::billing::{
    AddonCreditAutoReloadStatus, AddonCreditsOption, ServiceAgreement, ServiceAgreementType,
};
pub use torch_graphql::billing::{
    AiCreditsUsageAndCostSubjectType, AiCreditsUsageAndCostType, AiCreditsUsageBucket,
    AiCreditsUsageSource,
};

use super::team::{DiscoverableTeam, MembershipRole, Team};
use crate::auth::UserUid;
use crate::server::ids::ServerId;

#[derive(Clone, Copy, Hash, Debug, PartialEq, Eq)]
pub struct WorkspaceUid(ServerId);
impl From<String> for WorkspaceUid {
    fn from(uid: String) -> Self {
        WorkspaceUid(ServerId::from_string_lossy(uid))
    }
}
impl From<WorkspaceUid> for String {
    fn from(workspace_uid: WorkspaceUid) -> String {
        workspace_uid.0.to_string()
    }
}
impl From<ServerId> for WorkspaceUid {
    fn from(uid: ServerId) -> Self {
        WorkspaceUid(uid)
    }
}

#[derive(Clone, Debug)]
pub struct Workspace {
    pub uid: WorkspaceUid,
    pub name: String,
    pub stripe_customer_id: Option<String>,
    pub teams: Vec<Team>,
    pub open_teams: Vec<DiscoverableTeam>,
    pub billing_metadata: BillingMetadata,
    pub bonus_grants_purchased_this_month: BonusGrantsPurchased,
    pub billing_cycle_usage: Option<BillingCycleUsageData>,
    pub has_billing_history: bool,
    pub settings: WorkspaceSettings,
    pub invite_link_domain_restrictions: Vec<InviteLinkDomainRestriction>,
    pub pending_email_invites: Vec<EmailInvite>,
    // If the team is eligible for discovery, then show toggle for setting discoverability to the team's admin
    pub is_eligible_for_discovery: bool,
    pub members: Vec<WorkspaceMember>,
    pub total_requests_used_since_last_refresh: i32,
}

impl Workspace {
    pub fn from_local_cache(uid: WorkspaceUid, name: String, teams: Option<Vec<Team>>) -> Self {
        // Derive the workspace billing metadata from the first team's cached billing
        // metadata, if available. This ensures the workspace-level billing info is
        // consistent with team-level data loaded from the cache.
        let billing_metadata = teams
            .as_ref()
            .and_then(|t| t.first())
            .map(|team| team.billing_metadata.clone())
            .unwrap_or_default();
        Self {
            uid,
            name,
            stripe_customer_id: Default::default(),
            teams: teams.unwrap_or_default(),
            open_teams: Default::default(),
            billing_metadata,
            bonus_grants_purchased_this_month: Default::default(),
            billing_cycle_usage: None,
            has_billing_history: false,
            settings: Default::default(), // TODO: persistence wrapper instead of default
            invite_link_domain_restrictions: Default::default(),
            pending_email_invites: Default::default(),
            is_eligible_for_discovery: false,
            members: Default::default(),
            total_requests_used_since_last_refresh: 0,
        }
    }

    fn get_member_by_email(&self, email: &str) -> Option<&WorkspaceMember> {
        self.members.iter().find(|member| member.email == email)
    }

    pub fn is_workspace_admin(&self, user_email: &str) -> bool {
        self.get_member_by_email(user_email)
            .is_some_and(|member| member.role.is_admin_or_owner())
    }

    pub fn is_native_workspaces_enabled(&self) -> bool {
        self.billing_metadata
            .tier
            .native_workspaces_policy
            .is_some_and(|policy| policy.enabled)
    }

    pub fn joinable_teams(&self) -> impl Iterator<Item = &DiscoverableTeam> {
        self.open_teams.iter().filter(|open_team| {
            let open_team_uid = ServerId::from_string_lossy(&open_team.team_uid);
            self.teams.iter().all(|team| team.uid != open_team_uid)
        })
    }

    pub fn is_native_workspaces_admin(&self, user_email: &str) -> bool {
        self.is_workspace_admin(user_email) && self.is_native_workspaces_enabled()
    }

    pub fn resolve_usage_visibility(&self, is_admin: bool) -> UsageVisibility {
        let Some(policy) = self.billing_metadata.tier.usage_visibility_policy else {
            return UsageVisibility::default();
        };
        UsageVisibility {
            granularity: if is_admin {
                policy.admin_granularity
            } else {
                UsageVisibilityGranularity::OwnOnly
            },
            max_prior_cycles: policy.max_prior_cycles,
        }
    }

    pub fn can_be_deleted(&self, current_user_email: &str) -> bool {
        // Current user needs to be an admin and be the only user remaining
        self.is_workspace_admin(current_user_email)
            && self.members.len() == 1
            && self
                .members
                .first()
                .is_some_and(|m| m.email == current_user_email)
    }

    pub fn are_overages_toggleable(&self) -> bool {
        self.billing_metadata
            .tier
            .usage_based_pricing_policy
            .is_some_and(|policy| policy.toggleable)
    }

    pub fn are_overages_enabled(&self) -> bool {
        self.settings.usage_based_pricing_settings.enabled
    }

    pub fn are_overages_remaining(&self) -> bool {
        if self.settings.usage_based_pricing_settings.enabled
            && let Some(max_spend_cents) = self
                .settings
                .usage_based_pricing_settings
                .max_monthly_spend_cents
        {
            if let Some(ai_overages) = &self.billing_metadata.ai_overages {
                return ai_overages.current_monthly_request_cost_cents < max_spend_cents as i32;
            } else {
                // If they have the setting enabled but no overages usage so far,
                // that means they have no database entry, so they have overages remaining.
                return true;
            }
        }

        false
    }

    /// Returns true if the workspace has reached or exceeded its monthly addon credits spend limit.
    pub fn is_at_addon_credits_monthly_limit(&self) -> bool {
        if let Some(limit) = self.settings.addon_credits_settings.max_monthly_spend_cents {
            self.bonus_grants_purchased_this_month.cents_spent >= limit
        } else {
            false
        }
    }

    /// Returns true if purchasing addon credits at the given price would reach or exceed the monthly limit.
    pub fn would_addon_purchase_reach_limit(&self, price_cents: i32) -> bool {
        if let Some(limit) = self.settings.addon_credits_settings.max_monthly_spend_cents {
            self.bonus_grants_purchased_this_month.cents_spent + price_cents > limit
        } else {
            false
        }
    }

    /// Returns the price in cents for the selected auto-reload pack,
    /// including any plan surcharge (premium plans reload at the premium price).
    /// Returns None if auto-reload is not configured or if the pack can't be found in pricing options.
    pub fn get_auto_reload_price_cents(
        &self,
        addon_credits_options: &[AddonCreditsOption],
    ) -> Option<i32> {
        self.settings
            .addon_credits_settings
            .selected_auto_reload_option(addon_credits_options)
            .map(|option| {
                option.price_usd_cents_with_premium(
                    self.billing_metadata.addon_credits_price_premium_bps(),
                )
            })
    }
}

#[derive(Clone, Eq, PartialEq, Debug)]
pub struct WorkspaceMember {
    pub uid: UserUid,
    pub email: String,
    pub role: MembershipRole,
    pub is_disabled: bool,
    pub usage_info: WorkspaceMemberUsageInfo,
}

#[derive(Clone, Eq, PartialEq, Debug)]
pub struct WorkspaceMemberUsageInfo {
    pub is_unlimited: bool,
    pub request_limit: i32,
    pub requests_used_since_last_refresh: i32,
    /// The member's `includedUsageCents`: their included monthly allowance in US cents, when
    /// the server sends one; never sent when usage is unlimited.
    pub included_usage_cents: Option<OrderedFloat<f64>>,
    /// The member's `usageCentsUsedSinceLastRefresh`: how much of that allowance they have
    /// used, in US cents, when the server sends one; never sent when usage is unlimited.
    pub usage_cents_used_since_last_refresh: Option<OrderedFloat<f64>>,
    pub is_request_limit_prorated: bool,
}

impl PartialOrd for WorkspaceMember {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for WorkspaceMember {
    fn cmp(&self, other: &Self) -> Ordering {
        self.email.cmp(&other.email)
    }
}

#[derive(Clone, Eq, PartialEq, Debug)]
pub struct EmailInvite {
    pub invitee_email: String,
    pub expired: bool,
    pub team_uid: Option<ServerId>,
}

impl PartialOrd for EmailInvite {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for EmailInvite {
    fn cmp(&self, other: &Self) -> Ordering {
        self.invitee_email.cmp(&other.invitee_email)
    }
}

#[derive(Clone, Eq, PartialEq, Debug)]
pub struct InviteLinkDomainRestriction {
    pub uid: ServerId,
    pub domain: String,
}

impl PartialOrd for InviteLinkDomainRestriction {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for InviteLinkDomainRestriction {
    fn cmp(&self, other: &Self) -> Ordering {
        self.domain.cmp(&other.domain)
    }
}

/// This enum is the rust representation of `CustomerType` from the GraphQL Schema.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub enum CustomerType {
    #[default]
    Free,
    Turbo,
    SelfServe,
    Prosumer,
    Legacy,
    Enterprise,
    Business,
    Lightspeed,
    Build,
    BuildMax,
    Unknown,
}

impl CustomerType {
    pub fn to_display_string(self) -> String {
        match self {
            CustomerType::Free => "Free".to_string(),
            CustomerType::Turbo => "Turbo".to_string(),
            CustomerType::SelfServe => "Team".to_string(),
            CustomerType::Prosumer => "Pro".to_string(),
            CustomerType::Legacy => "Early adopter".to_string(),
            CustomerType::Enterprise => "Enterprise".to_string(),
            CustomerType::Business => "Business".to_string(),
            CustomerType::Lightspeed => "Lightspeed".to_string(),
            CustomerType::Build => "Build".to_string(),
            CustomerType::BuildMax => "Max".to_string(),
            CustomerType::Unknown => "".to_string(),
        }
    }
}

/// This enum is the rust representation of `DelinquencyStatus` from the GraphQL Schema.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub enum DelinquencyStatus {
    #[default]
    NoDelinquency,
    PastDue,
    Unpaid,
    TeamLimitExceeded,
    Unknown,
}

/// Rust representation of feature policies from the GraphQL Schema.
#[derive(Clone, Debug, Copy, Serialize, Deserialize)]
pub struct WarpAiPolicy {
    pub limit: i64,
    pub is_code_suggestions_toggleable: bool,
    pub is_prompt_suggestions_toggleable: bool,
    pub is_next_command_enabled: bool,
    pub is_git_operations_ai_enabled: bool,
    pub is_voice_enabled: bool,
}
#[derive(Clone, Debug, Copy, Serialize, Deserialize)]
pub struct WorkspaceSizePolicy {
    pub is_unlimited: bool,
    pub limit: i64,
}
#[derive(Clone, Debug, Copy, Serialize, Deserialize)]
pub struct SharedNotebooksPolicy {
    pub is_unlimited: bool,
    pub limit: i64,
}
#[derive(Clone, Debug, Copy, Serialize, Deserialize)]
pub struct SharedWorkflowsPolicy {
    pub is_unlimited: bool,
    pub limit: i64,
}

#[derive(Clone, Debug, Copy, Serialize, Deserialize)]
pub struct SessionSharingPolicy {
    pub is_enabled: bool,
    pub max_session_size: u64,
}

#[derive(Clone, Debug, Copy, Serialize, Deserialize)]
pub struct AIAutonomyPolicy {
    pub is_enabled: bool,
    pub toggleable: bool,
}

#[derive(Clone, Debug, Copy, Serialize, Deserialize)]
pub struct TelemetryDataCollectionPolicy {
    pub default: bool,
    pub toggleable: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UgcDataCollectionPolicy {
    pub default_setting: UgcCollectionEnablementSetting,
    pub toggleable: bool,
}

#[derive(Clone, Debug, Copy, Serialize, Deserialize)]
pub struct UsageBasedPricingPolicy {
    pub toggleable: bool,
}

#[derive(Clone, Debug, Copy, Serialize, Deserialize)]
pub struct CodebaseContextPolicy {
    pub toggleable: bool,
    pub index_limit: Option<u32>,
    pub max_files_per_repo: u32,
}

#[derive(Clone, Debug, Copy, Serialize, Deserialize)]
pub struct ByoApiKeyPolicy {
    pub enabled: bool,
}

#[derive(Clone, Debug, Copy, Serialize, Deserialize)]
pub struct ByoEndpointPolicy {
    pub enabled: bool,
}

#[derive(Clone, Debug, Copy, Serialize, Deserialize)]
pub struct ManagedByokByoePolicy {
    pub enabled: bool,
}

#[derive(Clone, Debug, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PurchaseAddOnCreditsPolicy {
    pub enabled: bool,
    /// When `enabled` is false, allows purchasing add-on credit packs at a
    /// `price_premium_bps` surcharge over list price (e.g. on the Free plan).
    #[serde(default)]
    pub premium_enabled: bool,
    /// Surcharge in basis points applied to list prices when purchasing via
    /// the premium path (1000 bps = +10%). 0 for standard purchasing plans.
    #[serde(default)]
    pub price_premium_bps: i32,
}

impl PurchaseAddOnCreditsPolicy {
    /// Whether this plan may purchase add-on credit packs at all, either at
    /// list price (`enabled`) or at a premium surcharge (`premium_enabled`).
    pub fn allows_purchases(&self) -> bool {
        self.enabled || self.premium_enabled
    }

    /// The surcharge in basis points applied to pack list prices. 0 whenever
    /// standard (list price) purchasing is enabled — standard purchasing
    /// wins if the server ever sends both flags.
    pub fn effective_premium_bps(&self) -> i32 {
        if !self.enabled && self.premium_enabled {
            self.price_premium_bps
        } else {
            0
        }
    }
}

/// The unit a plan charges AI usage in, as the server states it on the tier. The rollout signal
/// for dollars-first displays: while a plan charges in credits, the credit figures stay
/// authoritative even when a response also carries cents.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ChargeUnit {
    #[default]
    Credits,
    /// US cents: usage is billed in dollars.
    Cents,
}

/// Plan terms carried by the user's own tier rather than a workspace's. A teamless user's only
/// workspace is the server's placeholder, which the client drops, so this is the only place
/// their plan terms survive.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct UserTier {
    pub purchase_policy: Option<PurchaseAddOnCreditsPolicy>,
    pub charge_unit: ChargeUnit,
}

#[derive(Clone, Debug, Copy, Serialize, Deserialize)]
pub struct EnterprisePayAsYouGoPolicy {
    pub enabled: bool,
}

#[derive(Clone, Debug, Copy, Serialize, Deserialize)]
pub struct EnterpriseCreditsAutoReloadPolicy {
    pub enabled: bool,
}

#[derive(Clone, Debug, Copy, Serialize, Deserialize)]
pub struct MultiAdminPolicy {
    pub enabled: bool,
}

#[derive(Clone, Debug, Copy, Serialize, Deserialize)]
pub struct NativeWorkspacesPolicy {
    pub enabled: bool,
}

#[derive(Clone, Debug, Copy, Serialize, Deserialize)]
pub struct AmbientAgentsPolicy {
    pub max_concurrent_agents: i32,
    pub instance_shape: Option<InstanceShape>,
}

#[derive(Clone, Debug, Copy, Serialize, Deserialize)]
pub struct InstanceShape {
    pub vcpus: i32,
    pub memory_gb: i32,
}

/// Granularity at which a viewer can see AI usage across their team.
/// Non-admins always collapse to `OwnOnly` regardless of tier.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum UsageVisibilityGranularity {
    #[default]
    OwnOnly,
    TeamAggregate,
    PerUserTotals,
    FullBreakdown,
}

/// Number of prior billing cycles a viewer can scroll back through, in
/// addition to the always-visible current cycle. Plan-wide; applies to
/// admins and non-admins alike.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum MaxPriorCycles {
    #[default]
    None,
    /// Current cycle plus `n` prior cycles (`n >= 1`).
    Limited(u32),
    Unlimited,
}

/// Rust representation of the `UsageVisibilityPolicy` tier policy from the
/// GraphQL schema.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
pub struct UsageVisibilityPolicy {
    pub admin_granularity: UsageVisibilityGranularity,
    pub max_prior_cycles: MaxPriorCycles,
}

/// Effective per-viewer visibility, after combining the tier's
/// `UsageVisibilityPolicy` with the viewer's admin status. Non-admins always
/// collapse to `granularity == OwnOnly`; `max_prior_cycles` is plan-wide and
/// applies to admins and non-admins alike. Built by
/// [`Workspace::resolve_usage_visibility`].
#[derive(Clone, Copy, Debug, Default)]
pub struct UsageVisibility {
    pub granularity: UsageVisibilityGranularity,
    pub max_prior_cycles: MaxPriorCycles,
}

/// This struct is the rust representation of `Tier` from the GraphQL Schema.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Tier {
    pub name: String,
    pub description: String,
    pub charge_unit: ChargeUnit,
    pub warp_ai_policy: Option<WarpAiPolicy>,
    pub workspace_size_policy: Option<WorkspaceSizePolicy>,
    pub shared_notebooks_policy: Option<SharedNotebooksPolicy>,
    pub shared_workflows_policy: Option<SharedWorkflowsPolicy>,
    pub session_sharing_policy: Option<SessionSharingPolicy>,
    pub ai_autonomy_policy: Option<AIAutonomyPolicy>,
    pub telemetry_data_collection_policy: Option<TelemetryDataCollectionPolicy>,
    pub ugc_data_collection_policy: Option<UgcDataCollectionPolicy>,
    pub usage_based_pricing_policy: Option<UsageBasedPricingPolicy>,
    pub codebase_context_policy: Option<CodebaseContextPolicy>,
    pub byo_api_key_policy: Option<ByoApiKeyPolicy>,
    pub byo_endpoint_policy: Option<ByoEndpointPolicy>,
    pub managed_byok_byoe_policy: Option<ManagedByokByoePolicy>,
    pub purchase_add_on_credits_policy: Option<PurchaseAddOnCreditsPolicy>,
    pub enterprise_pay_as_you_go_policy: Option<EnterprisePayAsYouGoPolicy>,
    pub enterprise_credits_auto_reload_policy: Option<EnterpriseCreditsAutoReloadPolicy>,
    pub multi_admin_policy: Option<MultiAdminPolicy>,
    pub native_workspaces_policy: Option<NativeWorkspacesPolicy>,
    pub ambient_agents_policy: Option<AmbientAgentsPolicy>,
    pub usage_visibility_policy: Option<UsageVisibilityPolicy>,
}

/// This struct is the rust representation of `BillingMetadata` from the GraphQL Schema.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct BillingMetadata {
    pub tier: Tier,
    pub customer_type: CustomerType,
    pub delinquency_status: DelinquencyStatus,
    #[serde(skip)]
    pub service_agreements: Vec<ServiceAgreement>,
    #[serde(skip)]
    pub ai_overages: Option<AiOverages>,
}

#[derive(Clone, Debug, Default)]
pub struct BonusGrantsPurchased {
    pub total_credits_purchased: i32,
    pub cents_spent: i32,
}

#[derive(Clone, Debug)]
pub struct AiOverages {
    pub current_monthly_request_cost_cents: i32,
    pub current_monthly_requests_used: i32,
    pub current_period_end: chrono::DateTime<chrono::Utc>,
}

/// A single redacted usage entry from `Workspace.billingCycleUsageHistory`.
///
/// The shape of this entry depends on the viewer's resolved `UsageVisibility`:
/// * `OwnOnly` viewers receive only their own entries with real `cost_type` /
///   `usage_bucket` / `usage_source` values.
/// * `TeamAggregate` viewers receive exactly one synthetic `TEAM` row per cycle
///   carrying `Aggregate` sentinels for all three categorical fields.
/// * `PerUserTotals` viewers receive one row per user / service account per
///   cycle, also with `Aggregate` sentinels on the categorical fields.
/// * `FullBreakdown` viewers receive every real row, one per
///   `(subject, cost_type, bucket, source)` tuple. Categorical fields always
///   carry real values — the server does **not** synthesize an aggregate team
///   total at this granularity. Compute team-wide sums client-side if needed.
#[derive(Clone, Debug)]
pub struct BillingCycleUsageEntry {
    pub subject_type: AiCreditsUsageAndCostSubjectType,
    pub subject_uid: Option<String>,
    pub subject_display_name: Option<String>,
    pub cost_type: AiCreditsUsageAndCostType,
    pub usage_bucket: AiCreditsUsageBucket,
    pub usage_source: AiCreditsUsageSource,
    pub credits_used: f64,
    pub cost_cents: f64,
    /// Uid of the team this usage is attributed to. `billingCycleUsageHistory`
    /// is workspace-wide, so this is what scopes an entry to a single team.
    /// `None` for rows written before usage attribution shipped and for the
    /// synthetic aggregate rows the server emits below `FullBreakdown`
    /// visibility.
    pub attributed_team_uid: Option<String>,
}

/// Per-cycle bucket of redacted usage entries with explicit period bounds.
/// `period_end` is exclusive (e.g. a summary covering May 2026 has
/// `period_end = 2026-06-01T00:00:00Z`).
#[derive(Clone, Debug)]
pub struct BillingCycleUsageSummary {
    pub period_start: chrono::DateTime<chrono::Utc>,
    pub period_end: chrono::DateTime<chrono::Utc>,
    pub entries: Vec<BillingCycleUsageEntry>,
}

/// The full per-cycle usage history for a workspace, as redacted by the
/// server's `USAGE_VISIBILITY` policy. `current_period_start` /
/// `current_period_end` mark the cycle that's currently active; older
/// summaries cover prior cycles and the number of them retained is governed
/// by the policy's `max_prior_cycles`.
#[derive(Clone, Debug)]
pub struct BillingCycleUsageData {
    pub current_period_start: chrono::DateTime<chrono::Utc>,
    pub current_period_end: chrono::DateTime<chrono::Utc>,
    pub summaries: Vec<BillingCycleUsageSummary>,
}

impl BillingMetadata {
    /// Returns whether the current tier has a usage-based pricing policy that can be toggled.
    pub fn is_usage_based_pricing_toggleable(&self) -> bool {
        self.tier
            .usage_based_pricing_policy
            .as_ref()
            .is_some_and(|policy| policy.toggleable)
    }

    /**
     * Returns whether customer can upgrade to the Build plan based on their current tier.
     */
    pub fn can_upgrade_to_build_plan(&self) -> bool {
        match self.customer_type {
            CustomerType::Unknown
            | CustomerType::Business
            | CustomerType::Enterprise
            | CustomerType::Build
            | CustomerType::BuildMax => false,
            CustomerType::Free
            | CustomerType::Legacy
            | CustomerType::Prosumer
            | CustomerType::Turbo
            | CustomerType::SelfServe
            | CustomerType::Lightspeed => true,
        }
    }

    /**
     * Returns whether customer can upgrade to the Build Max plan based on their current tier.
     * Users on Build can upgrade to Build Max.
     */
    pub fn can_upgrade_to_build_max_plan(&self) -> bool {
        self.can_upgrade_to_build_plan() || self.customer_type == CustomerType::Build
    }

    /**
     * Returns whether customer can upgrade to a higher tier based on their current tier.
     */
    pub fn can_upgrade_to_higher_tier_plan(&self) -> bool {
        self.can_upgrade_to_build_plan()
    }

    pub fn is_stripe_paid_plan(customer_type: CustomerType) -> bool {
        match customer_type {
            CustomerType::Turbo
            | CustomerType::SelfServe
            | CustomerType::Prosumer
            | CustomerType::Business
            | CustomerType::Lightspeed
            | CustomerType::Build
            | CustomerType::BuildMax => true,
            CustomerType::Free
            | CustomerType::Enterprise
            | CustomerType::Legacy
            | CustomerType::Unknown => false,
        }
    }

    pub fn is_user_on_paid_plan(&self) -> bool {
        match self.customer_type {
            CustomerType::Turbo
            | CustomerType::SelfServe
            | CustomerType::Prosumer
            | CustomerType::Business
            | CustomerType::Lightspeed
            | CustomerType::Enterprise
            | CustomerType::Legacy
            | CustomerType::Build
            | CustomerType::BuildMax => true,
            CustomerType::Free | CustomerType::Unknown => false,
        }
    }

    pub fn is_on_stripe_paid_plan(&self) -> bool {
        BillingMetadata::is_stripe_paid_plan(self.customer_type)
    }

    pub fn is_on_build_plan(&self) -> bool {
        self.customer_type == CustomerType::Build
    }

    pub fn is_on_build_max_plan(&self) -> bool {
        self.customer_type == CustomerType::BuildMax
    }

    pub fn is_on_build_business_plan(&self) -> bool {
        self.customer_type == CustomerType::Business
            && matches!(
                self.service_agreements.first().map(|sa| &sa.type_),
                Some(ServiceAgreementType::SelfServe)
            )
    }

    pub fn is_on_legacy_business_plan(&self) -> bool {
        self.customer_type == CustomerType::Business && !self.is_on_build_business_plan()
    }

    pub fn is_enterprise_plan(&self) -> bool {
        self.customer_type == CustomerType::Enterprise
    }

    pub fn is_free_plan(&self) -> bool {
        self.customer_type == CustomerType::Free
    }

    pub fn is_on_legacy_paid_plan(&self) -> bool {
        match self.customer_type {
            CustomerType::Prosumer
            | CustomerType::Turbo
            | CustomerType::Lightspeed
            | CustomerType::SelfServe => true,
            CustomerType::Business => self.is_on_legacy_business_plan(),
            CustomerType::Free
            | CustomerType::Legacy
            | CustomerType::Enterprise
            | CustomerType::Build
            | CustomerType::BuildMax
            | CustomerType::Unknown => false,
        }
    }

    pub fn is_delinquent_due_to_payment_issue(&self) -> bool {
        self.delinquency_status == DelinquencyStatus::PastDue
            || self.delinquency_status == DelinquencyStatus::Unpaid
    }

    // Whether the enterprise customer is our Stable Torch Enterprise team (internal team of Warpers).
    pub fn is_warp_plan(&self) -> bool {
        self.tier.name == "Torch Plan"
    }

    pub fn has_active_subscription(&self) -> bool {
        if let Some(newest_service_agreement) = self.service_agreements.first() {
            let not_expired = Utc::now() < newest_service_agreement.current_period_end.utc();
            let not_delinquent = !self.is_delinquent_due_to_payment_issue();
            not_expired && not_delinquent
        } else {
            false
        }
    }

    pub fn is_byo_api_key_enabled(&self) -> bool {
        self.tier
            .byo_api_key_policy
            .is_some_and(|policy| policy.enabled)
    }

    pub fn is_byo_endpoint_enabled(&self) -> bool {
        self.tier
            .byo_endpoint_policy
            .is_some_and(|policy| policy.enabled)
    }

    pub fn is_managed_byok_byoe_enabled(&self) -> bool {
        self.tier
            .managed_byok_byoe_policy
            .is_some_and(|policy| policy.enabled)
    }

    pub fn has_overages_used(&self) -> bool {
        self.ai_overages
            .as_ref()
            .is_some_and(|ai_overages| ai_overages.current_monthly_requests_used > 0)
    }

    pub fn has_failed_addon_credit_auto_reload_status(&self) -> bool {
        self.service_agreements
            .first()
            .and_then(|sa| sa.addon_credit_auto_reload_status)
            .is_some_and(|status| matches!(status, AddonCreditAutoReloadStatus::Failed))
    }

    pub fn is_enterprise_pay_as_you_go_enabled(&self) -> bool {
        self.customer_type == CustomerType::Enterprise
            && self
                .tier
                .enterprise_pay_as_you_go_policy
                .is_some_and(|policy| policy.enabled)
    }

    pub fn is_enterprise_auto_reload_enabled(&self) -> bool {
        self.customer_type == CustomerType::Enterprise
            && self
                .tier
                .enterprise_credits_auto_reload_policy
                .is_some_and(|policy| policy.enabled)
    }

    /// Whether this plan may purchase add-on credit packs at all, either at
    /// list price (`enabled`) or at a premium surcharge (`premium_enabled`).
    pub fn is_purchase_add_on_credits_policy_enabled(&self) -> bool {
        self.tier
            .purchase_add_on_credits_policy
            .is_some_and(|policy| policy.allows_purchases())
    }

    /// Whether add-on credit purchases on this plan go through the premium
    /// (surcharged) path rather than standard list-price purchasing.
    pub fn is_premium_addon_credits_purchase(&self) -> bool {
        self.tier
            .purchase_add_on_credits_policy
            .is_some_and(|policy| !policy.enabled && policy.premium_enabled)
    }

    /// The surcharge in basis points applied to add-on credit pack list
    /// prices for this plan. 0 whenever standard purchasing is enabled.
    pub fn addon_credits_price_premium_bps(&self) -> i32 {
        self.tier
            .purchase_add_on_credits_policy
            .map_or(0, |policy| policy.effective_premium_bps())
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct TelemetrySettings {
    pub force_enabled: bool,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub enum UgcCollectionEnablementSetting {
    Disable,
    Enable,
    #[default]
    RespectUserSetting,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct UgcCollectionSettings {
    pub setting: UgcCollectionEnablementSetting,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub enum AdminEnablementSetting {
    Disable,
    Enable,
    #[default]
    RespectUserSetting,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct CloudConversationStorageSettings {
    pub setting: AdminEnablementSetting,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct LinkSharingSettings {
    pub anyone_with_link_sharing_enabled: bool,
    pub direct_link_sharing_enabled: bool,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct EnterpriseSecretRegex {
    pub pattern: String,
    #[serde(default)]
    pub name: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct SecretRedactionSettings {
    pub enabled: bool,
    pub regexes: Vec<EnterpriseSecretRegex>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct UsageBasedPricingSettings {
    pub enabled: bool,
    pub max_monthly_spend_cents: Option<u32>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct AddonCreditsSettings {
    pub auto_reload_enabled: bool,
    pub max_monthly_spend_cents: Option<i32>,
    pub selected_auto_reload_credit_denomination: Option<i32>,
    /// The list price of the selected auto-reload pack, which is the usage it buys for a
    /// workspace billed in dollars. `None` when auto-reload is not configured or the
    /// workspace is billed in credits.
    #[serde(default)]
    pub selected_auto_reload_usage_cents: Option<i32>,
}

impl AddonCreditsSettings {
    /// Whether `option` is the catalog pack auto-reload is configured to buy. A pack sold in
    /// dollars is matched by its list price, which survives the credit count a pack is listed
    /// under changing; otherwise by credit denomination.
    fn is_selected_auto_reload_option(&self, option: &AddonCreditsOption) -> bool {
        match (self.selected_auto_reload_usage_cents, option.usage_cents) {
            (Some(selected_usage_cents), Some(usage_cents)) => selected_usage_cents == usage_cents,
            (None, _) | (_, None) => self
                .selected_auto_reload_credit_denomination
                .is_some_and(|credits| credits == option.credits),
        }
    }

    pub fn selected_auto_reload_option<'a>(
        &self,
        options: &'a [AddonCreditsOption],
    ) -> Option<&'a AddonCreditsOption> {
        options
            .iter()
            .find(|option| self.is_selected_auto_reload_option(option))
    }

    pub fn selected_auto_reload_option_index(
        &self,
        options: &[AddonCreditsOption],
    ) -> Option<usize> {
        options
            .iter()
            .position(|option| self.is_selected_auto_reload_option(option))
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct CodebaseContextSettings {
    pub setting: AdminEnablementSetting,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct WorkspaceSettings {
    pub telemetry_settings: TelemetrySettings,
    pub ugc_collection_settings: UgcCollectionSettings,
    pub cloud_conversation_storage_settings: CloudConversationStorageSettings,
    pub link_sharing_settings: LinkSharingSettings,
    pub secret_redaction_settings: SecretRedactionSettings,
    pub is_invite_link_enabled: bool,
    pub is_discoverable: bool,
    pub usage_based_pricing_settings: UsageBasedPricingSettings,
    pub addon_credits_settings: AddonCreditsSettings,
    pub codebase_context_settings: CodebaseContextSettings,
    /// The team-level agent attribution setting. When `Enable` or `Disable`, the
    /// user toggle is locked. When `RespectUserSetting` (or absent), the user can choose.
    #[serde(default)]
    pub enable_warp_attribution: AdminEnablementSetting,
    #[serde(default)]
    pub default_host_slug: Option<String>,
}

/// A workspace-governable setting carried on [`TeamSettings`]: the effective
/// `value` plus whether the workspace layer enforces it (mirrors the server's
/// `*SettingInfo` wrappers). The enforcement bit is preserved so future admin UI
/// can distinguish workspace-enforced values from team-owned ones.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct EnforceableSetting<T> {
    pub value: T,
    #[serde(default)]
    pub is_enforced_by_workspace: bool,
}

/// A list setting split by the layer that contributed each entry (mirrors the
/// server's `StringListSettingInfo` / `SecretRedactionRegexListInfo`). `values`
/// is the authoritative merged result; `workspace_entries` / `team_entries` are
/// preserved so future admin UI can present the layers separately.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct SplitListSetting<T> {
    pub values: Vec<T>,
    #[serde(default)]
    pub workspace_entries: Vec<T>,
    #[serde(default)]
    pub team_entries: Vec<T>,
}

impl<T> SplitListSetting<T> {
    /// Whether an admin layer configured this list.
    ///
    /// Empty `values` does not answer that: the two allowlists merge by intersection
    /// server-side, so layers configuring `["ls"]` and `["git status"]` yield empty `values`
    /// with both entry lists populated. Reading `values` alone would call that unconfigured
    /// and fall back to the user's own profile allowlist, granting exemptions neither admin
    /// granted. The denylists and regex lists merge by union, where empty `values` already
    /// implies empty entries, so the entry checks are inert for them.
    ///
    /// Clearing a list stores it as unset rather than empty -- that is how an admin reverts
    /// to no constraint -- so "configured but empty" does not arise. If that changes,
    /// `StringListSettingInfo.isConfigured` answers this directly, once the vendored schema
    /// copy is re-pulled to include it.
    pub fn is_configured(&self) -> bool {
        !(self.values.is_empty()
            && self.workspace_entries.is_empty()
            && self.team_entries.is_empty())
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct TeamSecretRedactionSettings {
    pub enabled: EnforceableSetting<bool>,
    pub regexes: SplitListSetting<EnterpriseSecretRegex>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct TeamLinkSharingSettings {
    pub anyone_with_link_sharing_enabled: EnforceableSetting<bool>,
    pub direct_link_sharing_enabled: EnforceableSetting<bool>,
}

/// The effective settings that apply to a team, combining the workspace layer
/// with the team's own configuration.
///
/// This is intentionally a distinct type from [`WorkspaceSettings`] rather than
/// an alias: it is sourced from the server's effective `Team.settings`. Each
/// workspace-governable group keeps both its effective value **and** the
/// `is_enforced_by_workspace` / workspace-vs-team split metadata (via
/// [`EnforceableSetting`] / [`SplitListSetting`]) so future admin UI can recover
/// those details. Unlike `WorkspaceSettings`, it does not carry the
/// workspace-scoped `is_invite_link_enabled` / `is_discoverable` flags (those
/// remain on [`WorkspaceSettings`] and are read from the current workspace).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct TeamSettings {
    pub ugc_collection: EnforceableSetting<UgcCollectionEnablementSetting>,
    pub cloud_conversation_storage: EnforceableSetting<AdminEnablementSetting>,
    pub codebase_context: EnforceableSetting<AdminEnablementSetting>,
    pub secret_redaction: TeamSecretRedactionSettings,
    pub link_sharing: TeamLinkSharingSettings,
    pub telemetry_settings: TelemetrySettings,
    pub usage_based_pricing_settings: UsageBasedPricingSettings,
    pub addon_credits_settings: AddonCreditsSettings,
    /// The team-level agent attribution setting. When `Enable` or `Disable`, the
    /// user toggle is locked. When `RespectUserSetting` (or absent), the user can choose.
    #[serde(default)]
    pub enable_warp_attribution: AdminEnablementSetting,
    #[serde(default)]
    pub default_host_slug: Option<String>,
}
