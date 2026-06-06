use crate::events::BrokerEvent;
use bevy::prelude::*;
use shared::custom_id::CustomId;
use std::collections::HashMap;

pub struct CorePlugin;

impl Plugin for CorePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ClientEntities>()
            .add_message::<BrokerEvent>();
    }
}

#[derive(Resource)]
pub struct AssignedShard(pub CustomId);

#[derive(Resource, Default)]
pub struct ClientEntities(pub HashMap<u32, Entity>);