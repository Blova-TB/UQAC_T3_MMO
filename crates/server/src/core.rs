use crate::events::BrokerEvent;
use bevy::prelude::*;
use std::collections::HashMap;

use custom_id::custom_id::CustomId;

pub struct CorePlugin;

impl Plugin for CorePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ClientEntities>()
            .add_message::<BrokerEvent>()
            .add_message::<crate::events::BrokerCommand>();
    }
}

#[derive(Resource)]
pub struct AssignedShard(pub CustomId);

#[derive(Resource, Default)]
pub struct ClientEntities(pub HashMap<u32, Entity>);