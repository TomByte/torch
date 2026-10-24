/// Used for event tracking purposes, matches up with GraphQL enum of the same name.
#[derive(Copy, Default, Clone, Debug, Eq, PartialEq)]
pub enum CloudObjectEventEntrypoint {
    TeamSettings,
    ResourceCenter,
    UniversalSearch,
    ManagementUI,
    Blocklist,
    ImportModal,
    Onboarding,
    #[default]
    Unknown,
}

impl From<CloudObjectEventEntrypoint> for torch_graphql::object::CloudObjectEventEntrypoint {
    fn from(entrypoint: CloudObjectEventEntrypoint) -> Self {
        use torch_graphql::object::CloudObjectEventEntrypoint as GraphQLEntrypoint;
        match entrypoint {
            CloudObjectEventEntrypoint::TeamSettings => GraphQLEntrypoint::TeamSettings,
            CloudObjectEventEntrypoint::ResourceCenter => GraphQLEntrypoint::ResourceCenter,
            CloudObjectEventEntrypoint::UniversalSearch => GraphQLEntrypoint::UniversalSearch,
            CloudObjectEventEntrypoint::ManagementUI => GraphQLEntrypoint::DriveIndex,
            CloudObjectEventEntrypoint::Blocklist => GraphQLEntrypoint::Blocklist,
            CloudObjectEventEntrypoint::ImportModal => GraphQLEntrypoint::ImportModal,
            CloudObjectEventEntrypoint::Onboarding => GraphQLEntrypoint::Onboarding,
            CloudObjectEventEntrypoint::Unknown => GraphQLEntrypoint::Unknown,
        }
    }
}
