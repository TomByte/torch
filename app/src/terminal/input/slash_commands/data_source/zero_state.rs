use torchui::{Entity, ModelHandle};

use crate::search::SyncDataSource;
use crate::search::data_source::{Query, QueryResult};
use crate::search::mixer::DataSourceRunErrorWrapper;
use crate::terminal::input::slash_commands::{
    AcceptSlashCommandOrSavedPrompt, GuiSlashCommandDataSource, SlashCommandDataSource,
};

pub struct GuiZeroStateDataSource {
    slash_command_data_source: ModelHandle<GuiSlashCommandDataSource>,
}

impl GuiZeroStateDataSource {
    pub fn new(slash_command_data_source: &ModelHandle<GuiSlashCommandDataSource>) -> Self {
        Self {
            slash_command_data_source: slash_command_data_source.clone(),
        }
    }
}

impl Entity for GuiZeroStateDataSource {
    type Event = ();
}

impl SyncDataSource for GuiZeroStateDataSource {
    type Action = AcceptSlashCommandOrSavedPrompt;

    fn run_query(
        &self,
        query: &Query,
        app: &torchui::AppContext,
    ) -> Result<Vec<QueryResult<Self::Action>>, DataSourceRunErrorWrapper> {
        if !query.text.is_empty() {
            return Ok(vec![]);
        }

        let source = self.slash_command_data_source.as_ref(app);

        Ok(source
            .ordered_zero_state_commands(app)
            .into_iter()
            .map(Into::into)
            .collect())
    }
}
